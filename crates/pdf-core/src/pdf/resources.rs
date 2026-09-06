//! Unused-resource cleanup — remove `/Resources` entries (fonts, XObjects)
//! that no content stream in the owning tree actually references.
//! 未引用资源清理 — 删除内容流树中从未引用的 `/Resources` 条目（字体、XObject）。
//!
//! Mirrors `mutool clean -ggg`-style dead-weight removal, but every step is
//! deliberately fail-closed: the moment a page shows any pattern we cannot
//! prove safe (annotations with appearance streams, tiling patterns, Type3
//! fonts, a name that does not resolve in the immediate resource dictionary,
//! undecodable content), that page keeps all of its resources. Removed
//! entries orphan their target objects, which the save-time `prune_objects`
//! pass then drops.
//! 对标 `mutool clean -ggg` 式的冗余清理，但每一步都保守失败：只要页面出现
//! 无法证明安全的模式（带外观流的注解、平铺图案、Type3 字体、无法在当前
//! 资源字典解析的名字、无法解码的内容），该页面就原样保留全部资源。
//! 被删除条目的目标对象成为孤儿，由保存时的 `prune_objects` 兜底清除。

use std::collections::{HashMap, HashSet};

use lopdf::content::Content;
use lopdf::{Dictionary, Document, Object, ObjectId};

/// Identifies which object's `/Resources` dictionary a usage set belongs to.
/// Pages with an inline (direct) resources dictionary own their entry; pages
/// sharing one indirect resources object are grouped under that object; a
/// page-tree node's inline dictionary is the owner for every page that
/// inherits it (spec 7.7.2 — `/Resources` is inheritable).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum ResourcesOwner {
    Page(ObjectId),
    PageTree(ObjectId),
    SharedResources(ObjectId),
    Form(ObjectId),
}

/// Where a page's `/Resources` actually lives after inheritance.
#[derive(Debug, Clone, Copy)]
enum PageResources {
    /// An indirect resources object — every referencing page shares it.
    Shared(ObjectId),
    /// A dictionary carried inline by `holder`: the page itself, or the
    /// ancestor page-tree node an inherited entry was found on.
    Inline { holder: ObjectId },
}

/// Resolve the `/Resources` a page's names resolve against, climbing `/Parent`
/// for inherited entries (mirrors `fonts::effective_page_resources`). Pages
/// whose chain resolves to nothing are not resource users at all.
fn resolve_page_resources(document: &Document, page_id: ObjectId) -> Option<PageResources> {
    let mut current = page_id;
    let mut hops = 0;
    loop {
        let Object::Dictionary(page_dict) = document.objects.get(&current)? else {
            return None;
        };
        match page_dict.get(b"Resources") {
            Ok(Object::Reference(resources_id)) => {
                return Some(PageResources::Shared(*resources_id));
            }
            Ok(Object::Dictionary(_)) => {
                return Some(PageResources::Inline { holder: current });
            }
            _ => {
                let parent = page_dict
                    .get(b"Parent")
                    .ok()
                    .and_then(|entry| entry.as_reference().ok())?;
                current = parent;
                hops += 1;
                if hops > 64 {
                    return None;
                }
            }
        }
    }
}

/// Names referenced by `Tf` / `Do` operators within one resource context.
#[derive(Default)]
struct UsedNames {
    fonts: HashSet<Vec<u8>>,
    xobjects: HashSet<Vec<u8>>,
}

/// Remove `/Font` and `/XObject` entries never referenced by their content
/// trees. Returns how many entries were removed. Pages (and forms) that fail
/// any safety probe are left untouched.
pub(crate) fn remove_unused_resources(document: &mut Document) -> usize {
    let pages = document.get_pages();

    // owner → union of used names across every page mapping to that owner.
    let mut usage_by_owner: HashMap<ResourcesOwner, UsedNames> = HashMap::new();
    // Which owners are safe to clean, and how to reach their dictionary.
    let mut cleanable: Vec<ResourcesOwner> = Vec::new();
    // Pages that passed every probe and completed their content walk.
    let mut processed_pages: HashSet<ObjectId> = HashSet::new();

    // Pages sharing one resources owner stand or fall together: a sibling
    // whose usage cannot be proven (failed safety probe, undecodable
    // content, …) must veto cleaning for the whole group — its untracked
    // references resolve against the same dictionary. Owners keyed by the
    // resolved location, so inherited resources count every inheriting page.
    let mut pages_by_owner: HashMap<ResourcesOwner, Vec<ObjectId>> = HashMap::new();

    // Resources owners that a not-provably-safe page reached — directly or
    // through forms — must keep everything.
    let mut blocked_owners: HashSet<ResourcesOwner> = HashSet::new();

    for page_id in pages.values().copied() {
        let Some(Object::Dictionary(page_dict)) = document.objects.get(&page_id) else {
            continue;
        };

        // Inheritable `/Resources`: a page without its own entry resolves
        // against the nearest ancestor's (spec 7.7.2). Such a page is a full
        // user of that dictionary — its walk must run, and a failed walk must
        // veto cleaning exactly like a referencing sibling's.
        let (owner, resources_dict) = match resolve_page_resources(document, page_id) {
            Some(PageResources::Shared(resources_id)) => {
                match document.objects.get(&resources_id) {
                    Some(Object::Dictionary(dict)) => {
                        (ResourcesOwner::SharedResources(resources_id), dict)
                    }
                    _ => continue,
                }
            }
            Some(PageResources::Inline { holder }) => {
                let Some(Object::Dictionary(holder_dict)) = document.objects.get(&holder) else {
                    continue;
                };
                let Ok(Object::Dictionary(dict)) = holder_dict.get(b"Resources") else {
                    continue;
                };
                (
                    if holder == page_id {
                        ResourcesOwner::Page(page_id)
                    } else {
                        ResourcesOwner::PageTree(holder)
                    },
                    dict,
                )
            }
            None => continue,
        };
        pages_by_owner.entry(owner).or_default().push(page_id);

        if !page_is_safe(document, page_dict, resources_dict) {
            blocked_owners.insert(owner);
            block_page_form_resources(document, resources_dict, &mut blocked_owners);
            continue;
        }

        // A page can only be cleaned when its whole content tree resolves.
        let mut page_usage = UsedNames::default();
        let mut visited_forms: HashSet<ObjectId> = HashSet::new();
        let mut form_usage: HashMap<ResourcesOwner, UsedNames> = HashMap::new();
        let content = match document.get_and_decode_page_content(page_id) {
            Ok(content) => content,
            Err(_) => {
                blocked_owners.insert(owner);
                block_page_form_resources(document, resources_dict, &mut blocked_owners);
                continue;
            }
        };

        if !collect_used_names(
            document,
            &content,
            resources_dict,
            &mut page_usage,
            &mut visited_forms,
            &mut form_usage,
        ) {
            blocked_owners.insert(owner);
            block_page_form_resources(document, resources_dict, &mut blocked_owners);
            continue;
        }

        // Merge the page and every successfully walked form into their
        // actual resource-dictionary owners — two forms sharing one
        // /Resources object must union their names there, not clean it in
        // isolation twice (which would delete each other's entries).
        merge_usage(&mut usage_by_owner, owner, &page_usage);
        for (form_owner, usage) in form_usage {
            merge_usage(&mut usage_by_owner, form_owner, &usage);
            cleanable.push(form_owner);
        }
        processed_pages.insert(page_id);
        cleanable.push(owner);
    }

    // Cleanable owners may repeat (several pages sharing one resources
    // object, or one form walked from several pages) — dedupe before the
    // mutable pass, and only clean owners every visitor proved safe.
    cleanable.sort_unstable_by_key(|owner| match owner {
        ResourcesOwner::Page(id)
        | ResourcesOwner::PageTree(id)
        | ResourcesOwner::SharedResources(id)
        | ResourcesOwner::Form(id) => *id,
    });
    cleanable.dedup();
    // A page-resource owner is cleaned only when every page using it —
    // referencing or inheriting — was fully processed; one unproven page (or
    // a form only that page reaches) vetoes the group. Form owners entered
    // `cleanable` only through a completed walk of their own content, and an
    // inline dictionary nothing walked is never cleaned in the first place.
    cleanable.retain(|owner| {
        if blocked_owners.contains(owner) {
            return false;
        }
        match owner {
            ResourcesOwner::Form(_) => true,
            _ => pages_by_owner
                .get(owner)
                .is_some_and(|ids| ids.iter().all(|id| processed_pages.contains(id))),
        }
    });

    let mut removed_entries = 0;
    for owner in cleanable {
        let Some(usage) = usage_by_owner.get(&owner) else {
            continue;
        };
        let usage = UsedNames {
            fonts: usage.fonts.clone(),
            xobjects: usage.xobjects.clone(),
        };
        removed_entries += clean_owner_resources(document, owner, &usage);
    }

    removed_entries
}

fn merge_usage(target: &mut HashMap<ResourcesOwner, UsedNames>, owner: ResourcesOwner, usage: &UsedNames) {
    let entry = target.entry(owner).or_default();
    entry.fonts.extend(usage.fonts.iter().cloned());
    entry.xobjects.extend(usage.xobjects.iter().cloned());
}

/// Block the shared resources objects reachable from a page that could not
/// be proven safe: every form in its `/XObject` category may draw names the
/// failed walk never recorded, and so may every form those forms draw — the
/// walk recurses, because an unwalked chain at any depth can resolve against
/// a dictionary another page wants to clean.
fn block_page_form_resources(
    document: &Document,
    resources_dict: &Dictionary,
    blocked: &mut HashSet<ResourcesOwner>,
) {
    let Some(xobject_dict) = resources_dict
        .get(b"XObject")
        .ok()
        .and_then(|entry| dereference_dictionary(document, entry))
    else {
        return;
    };
    let mut visited: HashSet<ObjectId> = HashSet::new();
    for entry in xobject_dict.iter() {
        let Object::Reference(form_id) = entry.1 else {
            continue;
        };
        block_form_resources(document, *form_id, blocked, &mut visited);
    }
}

/// Block the shared resources of one form and everything it can reach.
fn block_form_resources(
    document: &Document,
    form_id: ObjectId,
    blocked: &mut HashSet<ResourcesOwner>,
    visited: &mut HashSet<ObjectId>,
) {
    if !visited.insert(form_id) {
        return;
    }
    let Some(Object::Stream(form)) = document.objects.get(&form_id) else {
        return;
    };
    if !matches!(form.dict.get(b"Subtype"), Ok(Object::Name(subtype)) if subtype.as_slice() == b"Form")
    {
        return;
    }
    let Some(form_resources) = form
        .dict
        .get(b"Resources")
        .ok()
        .and_then(|entry| dereference_dictionary(document, entry))
    else {
        return;
    };
    if let Ok(Object::Reference(resources_id)) = form.dict.get(b"Resources") {
        blocked.insert(ResourcesOwner::SharedResources(*resources_id));
    }
    // An inline form dictionary only becomes cleanable through a completed
    // walk of its own content, so there is nothing to block for it here.
    let Some(xobject_dict) = form_resources
        .get(b"XObject")
        .ok()
        .and_then(|entry| dereference_dictionary(document, entry))
    else {
        return;
    };
    for entry in xobject_dict.iter() {
        let Object::Reference(nested_id) = entry.1 else {
            continue;
        };
        block_form_resources(document, *nested_id, blocked, visited);
    }
}

/// Whole-page safety probes for patterns whose resource usage cannot be
/// derived from the page content tree alone. `resources_dict` is the
/// page's effective (possibly inherited) resources dictionary.
fn page_is_safe(document: &Document, page_dict: &Dictionary, resources_dict: &Dictionary) -> bool {
    // Annotation appearance streams carry their own content that may fall
    // back to the page's resources — annotations with /AP opt the page out.
    if let Ok(Object::Array(annotations)) = page_dict.get(b"Annots") {
        for annotation in annotations {
            let annotation_dict = match annotation {
                Object::Reference(annotation_id) => {
                    match document.objects.get(annotation_id) {
                        Some(Object::Dictionary(dict)) => Some(dict),
                        _ => None,
                    }
                }
                Object::Dictionary(dict) => Some(dict),
                _ => None,
            };
            if annotation_dict.is_some_and(|dict| dict.get(b"AP").is_ok()) {
                return false;
            }
        }
    }

    // Tiling patterns are content streams too; their resource usage is out
    // of scope here, so any pattern resource opts the page out.
    if let Some(pattern_dict) = resources_dict
        .get(b"Pattern")
        .ok()
        .and_then(|entry| dereference_dictionary(document, entry))
    {
        if !pattern_dict.is_empty() {
            return false;
        }
    }

    resources_prove_safe(document, resources_dict)
}

/// Probes shared by the page gate and the form walk: usage patterns the
/// `Tf`/`Do` interpretation below cannot see.
fn resources_prove_safe(document: &Document, resources_dict: &Dictionary) -> bool {
    // ExtGState `/Font` entries select fonts by direct object reference
    // (PDF 8.4.5) — a font used only through `gs` never appears as a `Tf`
    // operand, so its resource entry must not be judged unused. Out of
    // scope, opt out.
    if let Some(extgstate_dict) = resources_dict
        .get(b"ExtGState")
        .ok()
        .and_then(|entry| dereference_dictionary(document, entry))
    {
        for entry in extgstate_dict.iter() {
            let Some(state_dict) = dereference_dictionary(document, entry.1) else {
                continue;
            };
            if state_dict.get(b"Font").is_ok() {
                return false;
            }
        }
    }

    true
}

fn dereference<'a>(document: &'a Document, object: &'a Object) -> Option<&'a Object> {
    match object {
        Object::Reference(object_id) => document.objects.get(object_id),
        _ => Some(object),
    }
}

fn dereference_dictionary<'a>(document: &'a Document, object: &'a Object) -> Option<&'a Dictionary> {
    match dereference(document, object)? {
        Object::Dictionary(dict) => Some(dict),
        _ => None,
    }
}

/// Walk one decoded content stream, recording the names it uses against
/// `usage`. `Do` recursion into form XObjects records against the form's
/// actual resource-dictionary owner — the shared object when the form's
/// `/Resources` is a reference (so sibling forms union there), the form
/// itself when the dict is inline. Returns false when anything is off —
/// the caller then keeps this tree untouched.
fn collect_used_names(
    document: &Document,
    content: &Content,
    resources_dict: &Dictionary,
    usage: &mut UsedNames,
    visited_forms: &mut HashSet<ObjectId>,
    form_usage: &mut HashMap<ResourcesOwner, UsedNames>,
) -> bool {
    let font_dict = resources_dict
        .get(b"Font")
        .ok()
        .and_then(|entry| dereference_dictionary(document, entry));
    let xobject_dict = resources_dict
        .get(b"XObject")
        .ok()
        .and_then(|entry| dereference_dictionary(document, entry));

    // The same ExtGState/tiling-pattern probes as the page gate: a form's
    // resources may carry them just as well.
    if !resources_prove_safe(document, resources_dict) {
        return false;
    }

    for operation in &content.operations {
        match operation.operator.as_str() {
            "Tf" => {
                let Some(Object::Name(name)) = operation.operands.first() else {
                    continue;
                };
                let Some(font_dict) = font_dict else {
                    return false;
                };
                let Some(font_entry) = font_dict.get(name.as_slice()).ok() else {
                    // A font name that does not resolve here may rely on an
                    // inheritance fallback — keep everything.
                    return false;
                };
                // Type3 glyph procedures are content streams that may use the
                // page's resources — out of scope, opt out.
                let font_object = match font_entry {
                    Object::Reference(font_id) => document.objects.get(font_id),
                    Object::Dictionary(_) => Some(font_entry),
                    _ => None,
                };
                if matches!(
                    font_object,
                    Some(Object::Dictionary(font))
                        if matches!(font.get(b"Subtype"), Ok(Object::Name(subtype)) if subtype.as_slice() == b"Type3")
                ) {
                    return false;
                }
                usage.fonts.insert(name.clone());
            }
            "Do" => {
                let Some(Object::Name(name)) = operation.operands.first() else {
                    continue;
                };
                let Some(xobject_dict) = xobject_dict else {
                    return false;
                };
                let Some(xobject_entry) = xobject_dict.get(name.as_slice()).ok() else {
                    return false;
                };
                usage.xobjects.insert(name.clone());

                // Recurse into form XObjects — they carry their own resource
                // context. Anything else (images, PostScript) is terminal.
                let Object::Reference(form_id) = xobject_entry else {
                    continue;
                };
                if !visited_forms.insert(*form_id) {
                    continue;
                };
                let Some(Object::Stream(form)) = document.objects.get(form_id) else {
                    return false;
                };
                if !matches!(form.dict.get(b"Subtype"), Ok(Object::Name(subtype)) if subtype.as_slice() == b"Form")
                {
                    continue;
                }
                // A form without its own /Resources may fall back to the
                // page's — its content decides nothing here.
                let Some(form_resources) = form
                    .dict
                    .get(b"Resources")
                    .ok()
                    .and_then(|entry| dereference_dictionary(document, entry))
                else {
                    return false;
                };
                let form_content = match decode_form_content(document, *form_id) {
                    Some(decoded) => decoded,
                    None => return false,
                };
                let mut nested = UsedNames::default();
                if !collect_used_names(
                    document,
                    &form_content,
                    form_resources,
                    &mut nested,
                    visited_forms,
                    form_usage,
                ) {
                    return false;
                }
                // Attribution key: the dictionary this form's names resolve
                // against — the shared object when referenced (sibling forms
                // union into the same bucket), the form itself when inline.
                let form_owner = match form.dict.get(b"Resources") {
                    Ok(Object::Reference(resources_id)) => {
                        ResourcesOwner::SharedResources(*resources_id)
                    }
                    _ => ResourcesOwner::Form(*form_id),
                };
                let accumulated = form_usage.entry(form_owner).or_default();
                accumulated.fonts.extend(nested.fonts);
                accumulated.xobjects.extend(nested.xobjects);
            }
            _ => {}
        }
    }

    true
}

/// Decode a form XObject's content. Mirrors what
/// `get_and_decode_page_content` does for pages, but forms are plain streams
/// here — decompress and parse directly.
fn decode_form_content(document: &Document, form_id: ObjectId) -> Option<Content> {
    let Object::Stream(form) = document.objects.get(&form_id)? else {
        return None;
    };
    let bytes = form.get_plain_content().ok()?;
    Content::decode(&bytes).ok()
}

/// Remove unused `/Font` and `/XObject` entries from one owner's resources.
fn clean_owner_resources(document: &mut Document, owner: ResourcesOwner, usage: &UsedNames) -> usize {
    // Where the owner's dictionary lives: an indirect resources object, or
    // an inline entry carried by a holder (the page itself, a page-tree
    // node for inherited resources, or a form XObject).
    enum DictionaryLocation {
        Shared(ObjectId),
        Inline(ObjectId),
    }

    let location = match owner {
        ResourcesOwner::Page(page_id) | ResourcesOwner::PageTree(page_id) => {
            let Some(Object::Dictionary(holder_dict)) = document.objects.get(&page_id) else {
                return 0;
            };
            match holder_dict.get(b"Resources") {
                Ok(Object::Reference(resources_id)) => DictionaryLocation::Shared(*resources_id),
                Ok(Object::Dictionary(_)) => DictionaryLocation::Inline(page_id),
                _ => return 0,
            }
        }
        ResourcesOwner::SharedResources(resources_id) => {
            DictionaryLocation::Shared(resources_id)
        }
        ResourcesOwner::Form(form_id) => {
            let Some(Object::Stream(form)) = document.objects.get(&form_id) else {
                return 0;
            };
            match form.dict.get(b"Resources") {
                Ok(Object::Reference(resources_id)) => DictionaryLocation::Shared(*resources_id),
                Ok(Object::Dictionary(_)) => DictionaryLocation::Inline(form_id),
                _ => return 0,
            }
        }
    };

    match location {
        DictionaryLocation::Shared(resources_id) => {
            let Some(Object::Dictionary(resources_dict)) = document.objects.get_mut(&resources_id)
            else {
                return 0;
            };
            clean_resource_dictionary(resources_dict, usage)
        }
        // Inline resources: mutate through the object carrying the entry.
        DictionaryLocation::Inline(holder_id) => match document.objects.get_mut(&holder_id) {
            Some(Object::Dictionary(holder_dict)) => {
                let Ok(Object::Dictionary(resources_dict)) = holder_dict.get_mut(b"Resources")
                else {
                    return 0;
                };
                clean_resource_dictionary(resources_dict, usage)
            }
            Some(Object::Stream(form)) => {
                let Ok(Object::Dictionary(resources_dict)) = form.dict.get_mut(b"Resources")
                else {
                    return 0;
                };
                clean_resource_dictionary(resources_dict, usage)
            }
            _ => 0,
        },
    }
}

fn clean_resource_dictionary(resources_dict: &mut Dictionary, usage: &UsedNames) -> usize {
    let mut removed = 0;

    for key in [b"Font".as_slice(), b"XObject".as_slice()] {
        let Ok(Object::Dictionary(category)) = resources_dict.get_mut(key) else {
            continue;
        };
        let unused: Vec<Vec<u8>> = category
            .iter()
            .filter(|(name, _)| {
                let used = match key {
                    b"Font" => usage.fonts.contains(name.as_slice()),
                    _ => usage.xobjects.contains(name.as_slice()),
                };
                !used
            })
            .map(|(name, _)| name.clone())
            .collect();
        for name in unused {
            if category.remove(&name).is_some() {
                removed += 1;
            }
        }
    }

    removed
}
