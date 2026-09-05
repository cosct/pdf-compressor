//! Font subsetting — shrink embedded `Type0` fonts with TrueType or CFF
//! outlines to the glyphs the content streams actually draw.
//! 字体子集化 — 将内嵌 `Type0`（TrueType 或 CFF 轮廓）字体裁剪到内容流实际
//! 绘制的字形。
//!
//! The typst `subsetter` crate retains glyphs by id and strips `cmap`, so a
//! subset can only be consumed as a CID font. That is exactly what we have:
//! content strings keep their original 2-byte CIDs untouched. For
//! CIDFontType2 the new glyph numbering is bridged with a generated
//! `/CIDToGIDMap` stream; for CIDFontType0 (CFF outlines) there is no such
//! key — the font's own charset IS the CID→glyph mapping — so the subset
//! gets a freshly appended format-0 charset that hands every kept glyph its
//! original CID back (see `cff::bridge_subset_charset`). `/W` is left
//! untouched in both cases: widths are keyed by CID (ISO 32000, table 115)
//! and the CIDs never change. Nothing outside the font object tree is
//! rewritten. Fail-closed: any font with an unusual shape (non-Identity
//! encoding, unreadable program, a CID the font cannot resolve,
//! subsetting failure, or no size win) keeps its original program; an
//! annotation appearance stream, a Type3 font, a font program shared with a
//! non-candidate font, or one ambiguous `Tf` name anywhere aborts the whole
//! pass.
//! typst 的 `subsetter` 按字形 id 保留并剥离 `cmap`，子集只能作 CID 字体使用——
//! 这正合适：内容流的 2 字节 CID 原样保留。CIDFontType2 用生成的 `/CIDToGIDMap`
//! 流桥接新字形编号；CIDFontType0（CFF 轮廓）没有这个键——字体自身的 charset
//! 就是 CID→字形映射——因此给子集追加一张 format-0 charset，把每个保留字形的
//! 原 CID 还给它（见 `cff::bridge_subset_charset`）。两种情况下 `/W` 都不做任何
//! 改写（宽度以 CID 为键，见 ISO 32000 表 115，CID 未变则原数组仍然正确），
//! 字体对象树之外零改写。任何异常形态（非 Identity 编码、程序不可读、字体解析
//! 不出的 CID、子集化失败、无体积收益）都保留原字体；注解外观流、Type3 字体、
//! 与非候选字体共享的字体程序、或任一处无法解析的 `Tf` 名字，都会整体放弃本轮。

use std::collections::{HashMap, HashSet};
use std::sync::{atomic::AtomicBool, Arc};

use lopdf::content::Content;
use lopdf::{dictionary, Dictionary, Document, Object, ObjectId, Stream};

use super::cff;
use super::ensure_not_cancelled;
use crate::error::AppError;

/// Outcome of one subsetting pass.
#[derive(Debug, Default)]
pub(crate) struct FontSubsetOutcome {
    pub fonts_subsetted: usize,
    pub bytes_saved: u64,
}

/// How a content-stream CID becomes a glyph id inside the embedded program.
#[derive(Debug)]
enum CidMapping {
    /// CIDFontType2 with `/CIDToGIDMap /Identity` (or absent).
    Identity,
    /// CIDFontType2 with a custom `/CIDToGIDMap` stream's bytes.
    Stream(Vec<u8>),
    /// CIDFontType0: the CFF charset, parsed up front. `cids_by_gid[i]` is
    /// the CID of glyph `i`; `gid_by_cid` is the reverse (a font with
    /// duplicate CIDs is rejected at parse time).
    CffCharset {
        cids_by_gid: Vec<u16>,
        gid_by_cid: HashMap<u16, u16>,
    },
}

/// A `Type0 → CIDFontType0/2 → FontFile2/3` chain eligible for subsetting.
#[derive(Debug)]
struct Type0Candidate {
    descendant_id: ObjectId,
    font_file_id: ObjectId,
    mapping: CidMapping,
}

pub(crate) fn subset_embedded_fonts(
    document: &mut Document,
    cancel_flag: &Arc<AtomicBool>,
    task_id: &str,
) -> Result<FontSubsetOutcome, AppError> {
    let pages = document.get_pages();

    // --- Phase 1: index the eligible font chains ---
    let candidates: HashMap<ObjectId, Type0Candidate> = index_candidates(document);

    // --- Phase 2: collect the CIDs each content tree draws ---
    // One unresolvable `Tf` name could be a resource-inheritance fallback
    // against a font we are about to subset — abort the whole pass instead.
    let mut used_cids: HashMap<ObjectId, HashSet<u16>> = HashMap::new();
    let mut visited_forms: HashSet<ObjectId> = HashSet::new();
    for page_id in pages.values().copied() {
        // Annotation appearance streams are content streams of their own
        // that this walk never sees — one /AP anywhere aborts the pass.
        if let Some(Object::Dictionary(page_dict)) = document.objects.get(&page_id) {
            if page_has_appearance_streams(document, page_dict) {
                return Ok(FontSubsetOutcome::default());
            }
        }
        let resources = match effective_page_resources(document, page_id) {
            Some(resources) => resources,
            None => continue,
        };
        let content = match document.get_and_decode_page_content(page_id) {
            Ok(content) => content,
            Err(_) => continue,
        };
        if !collect_context_cids(
            document,
            &content,
            resources,
            &candidates,
            &mut used_cids,
            &mut visited_forms,
        )? {
            return Ok(FontSubsetOutcome::default());
        }
    }

    // --- Phase 3: subset per font file (candidates may share one program) ---
    let mut files: HashMap<ObjectId, Vec<ObjectId>> = HashMap::new();
    for (type0_id, candidate) in &candidates {
        files.entry(candidate.font_file_id).or_default().push(*type0_id);
    }
    let consumers = font_file_consumers(document);

    let mut outcome = FontSubsetOutcome::default();
    for (font_file_id, type0_ids) in files {
        ensure_not_cancelled(cancel_flag, task_id)?;

        // A program shared with any font outside the candidate set keeps its
        // glyphs — the other consumer's numbering would break under the subset.
        let candidate_descendants: HashSet<ObjectId> = type0_ids
            .iter()
            .filter_map(|id| candidates.get(id))
            .map(|candidate| candidate.descendant_id)
            .collect();
        if consumers.get(&font_file_id).is_some_and(|ids| {
            ids.iter().any(|id| !candidate_descendants.contains(id))
        }) {
            continue;
        }

        // Union of the glyph ids every sharing font draws. A CID the CFF
        // charset cannot resolve dooms the whole program — that CID must
        // keep working against the original font.
        let mut old_gids: HashSet<u16> = HashSet::new();
        old_gids.insert(0); // .notdef must survive
        let mut cff_charset: Option<&Vec<u16>> = None;
        for type0_id in &type0_ids {
            let Some(candidate) = candidates.get(type0_id) else {
                continue;
            };
            if let CidMapping::CffCharset { cids_by_gid, .. } = &candidate.mapping {
                cff_charset = Some(cids_by_gid);
            }
            let Some(cids) = used_cids.get(type0_id) else {
                continue;
            };
            for cid in cids {
                match cid_to_gid(candidate, *cid) {
                    Some(gid) => {
                        old_gids.insert(gid);
                    }
                    None => {
                        old_gids.clear();
                        break;
                    }
                }
            }
            if old_gids.is_empty() {
                break;
            }
        }
        if old_gids.is_empty() {
            continue;
        }

        let Some(Object::Stream(font_file)) = document.objects.get(&font_file_id) else {
            continue;
        };
        let original = font_file.get_plain_content().unwrap_or_default();
        if original.is_empty() {
            continue;
        }

        let mut remapper = subsetter::GlyphRemapper::new();
        for gid in &old_gids {
            remapper.remap(*gid);
        }
        // The subsetter only consumes OpenType data; a bare CFF program
        // (the in-spec FontFile3 form) travels wrapped. TrueType programs
        // pass through borrowed, as before.
        let wrapped_cff = match cff_charset {
            Some(_) if !original.starts_with(b"OTTO") => Some(cff::wrap_bare_cff(&original)),
            _ => None,
        };
        let subset_input: &[u8] = wrapped_cff.as_deref().unwrap_or(&original);
        let subset = match subsetter::subset(subset_input, 0, &remapper) {
            Ok(subset) => subset,
            Err(_) => continue,
        };
        if subset.len() >= original.len() {
            continue;
        }

        let final_program: Vec<u8> = match cff_charset {
            // CIDFontType0: extract the CFF table, hand every kept glyph its
            // original CID back through the bridging charset, and re-embed
            // as a bare CIDFontType0C program.
            Some(cids_by_gid) => {
                let Some(bare) = cff::extract_cff_table(&subset) else {
                    continue;
                };
                let mut bare = bare.to_vec();
                let cids_by_new_gid: Vec<u16> = remapper
                    .remapped_gids()
                    .map(|old_gid| {
                        cids_by_gid
                            .get(usize::from(old_gid))
                            .copied()
                            .unwrap_or_default()
                    })
                    .collect();
                if !cff::bridge_subset_charset(&mut bare, &cids_by_new_gid) {
                    continue;
                }
                if bare.len() >= original.len() {
                    continue;
                }
                bare
            }
            None => subset,
        };

        // Replace the font program in place (same object id).
        let is_cff = cff_charset.is_some();
        let saved = (original.len() - final_program.len()) as u64;
        let Some(Object::Stream(font_file)) = document.objects.get_mut(&font_file_id) else {
            continue;
        };
        font_file.dict.remove(b"Filter");
        font_file.dict.remove(b"DecodeParms");
        if is_cff {
            // FontFile3 identifies its program by /Subtype; the rewritten
            // program is a bare CID-keyed CFF regardless of the input shape.
            font_file.dict.remove(b"Length1");
            font_file
                .dict
                .set("Subtype", Object::Name(b"CIDFontType0C".to_vec()));
        } else {
            font_file
                .dict
                .set("Length1", Object::Integer(final_program.len() as i64));
        }
        font_file.set_content(final_program);
        outcome.bytes_saved += saved;

        // Bridge each font's unchanged CIDs to the new glyph numbering. A
        // CIDFontType0 needs nothing here — its bridge lives inside the
        // rewritten charset.
        for type0_id in &type0_ids {
            let Some(candidate) = candidates.get(type0_id) else {
                continue;
            };
            let Some(cids) = used_cids.get(type0_id) else {
                continue;
            };
            let rewired = match &candidate.mapping {
                CidMapping::CffCharset { .. } => true,
                _ => rewire_descendant_font(document, candidate, cids, &remapper),
            };
            if rewired {
                outcome.fonts_subsetted += 1;
            }
        }
    }

    Ok(outcome)
}

/// Find `Type0 → CIDFontType0/2 → FontFile2/3` chains with an Identity
/// encoding.
fn index_candidates(document: &Document) -> HashMap<ObjectId, Type0Candidate> {
    let mut candidates = HashMap::new();
    for (&object_id, object) in &document.objects {
        let Object::Dictionary(font_dict) = object else {
            continue;
        };
        if !matches!(font_dict.get(b"Type"), Ok(Object::Name(kind)) if kind.as_slice() == b"Font")
        {
            continue;
        }
        if !matches!(font_dict.get(b"Subtype"), Ok(Object::Name(kind)) if kind.as_slice() == b"Type0")
        {
            continue;
        }
        if !matches!(font_dict.get(b"Encoding"), Ok(Object::Name(encoding))
            if matches!(encoding.as_slice(), b"Identity-H" | b"Identity-V"))
        {
            continue;
        }

        // DescendantFonts[0] must be a CIDFontType0 (CFF outlines) or
        // CIDFontType2 (TrueType outlines).
        let Some(descendant_id) = font_dict
            .get(b"DescendantFonts")
            .ok()
            .and_then(|entry| entry.as_array().ok())
            .and_then(|entries| entries.first().cloned())
            .and_then(|entry| entry.as_reference().ok())
        else {
            continue;
        };
        let Some(Object::Dictionary(descendant)) = document.objects.get(&descendant_id) else {
            continue;
        };
        let descendant_kind = match descendant.get(b"Subtype") {
            Ok(Object::Name(kind)) => kind,
            _ => continue,
        };
        let (font_file_id, mapping) = match descendant_kind.as_slice() {
            // TrueType outlines: FontDescriptor → FontFile2.
            b"CIDFontType2" => {
                let font_file_id = match font_file_key(document, descendant, b"FontFile2") {
                    Some(id) => id,
                    None => continue,
                };
                // A custom /CIDToGIDMap stream is readable; anything exotic skips.
                let mapping = match descendant.get(b"CIDToGIDMap") {
                    Ok(Object::Name(name)) if name.as_slice() == b"Identity" => {
                        CidMapping::Identity
                    }
                    Ok(Object::Reference(map_id)) => {
                        let Some(Object::Stream(map)) = document.objects.get(map_id) else {
                            continue;
                        };
                        match map.get_plain_content() {
                            Ok(bytes) => CidMapping::Stream(bytes),
                            Err(_) => continue,
                        }
                    }
                    // Absent defaults to the identity mapping per the PDF spec.
                    Err(_) => CidMapping::Identity,
                    Ok(_) => continue,
                };
                (font_file_id, mapping)
            }
            // CFF outlines: FontDescriptor → FontFile3 (/CIDFontType0C,
            // /Type1C, or /OpenType); the program must be a parseable
            // CID-keyed CFF (a CIDFontType0 with a SID-keyed or exotic
            // program is out of shape and skips).
            b"CIDFontType0" => {
                let font_file_id =
                    match font_file_key(document, descendant, b"FontFile3") {
                        Some(id) => id,
                        None => continue,
                    };
                let Some(Object::Stream(font_file)) = document.objects.get(&font_file_id) else {
                    continue;
                };
                match cff_mapping(font_file) {
                    Some(mapping) => (font_file_id, mapping),
                    None => continue,
                }
            }
            _ => continue,
        };

        candidates.insert(
            object_id,
            Type0Candidate {
                descendant_id,
                font_file_id,
                mapping,
            },
        );
    }
    candidates
}

/// Resolve `FontDescriptor → <key>` to the font file object id.
fn font_file_key(document: &Document, descendant: &Dictionary, key: &[u8]) -> Option<ObjectId> {
    descendant
        .get(b"FontDescriptor")
        .ok()
        .and_then(|entry| entry.as_reference().ok())
        .and_then(|descriptor_id| {
            let descriptor = match document.objects.get(&descriptor_id) {
                Some(Object::Dictionary(descriptor)) => descriptor,
                _ => return None,
            };
            descriptor.get(key).ok()?.as_reference().ok()
        })
}

/// Parse a FontFile3 stream into the CID mapping of a CID-keyed CFF.
/// Accepts bare CFF (`/CIDFontType0C`, the in-spec form, and the `/Type1C`
/// label some producers mislabel CID programs with) and OpenType-wrapped
/// CFF (`/OpenType`). Duplicate CIDs in the charset reject the font — the
/// reverse mapping would be ambiguous.
fn cff_mapping(font_file: &Stream) -> Option<CidMapping> {
    let subtype = match font_file.dict.get(b"Subtype") {
        Ok(Object::Name(subtype))
            if matches!(subtype.as_slice(), b"CIDFontType0C" | b"Type1C" | b"OpenType") =>
        {
            subtype.clone()
        }
        _ => return None,
    };
    let program = font_file.get_plain_content().ok()?;
    let bare = if subtype.as_slice() == b"OpenType" {
        cff::extract_cff_table(&program)?
    } else {
        &program[..]
    };
    let font = cff::CffFont::parse(bare)?;
    if !font.is_cid_keyed() {
        return None;
    }
    let cids_by_gid = font.cids_by_gid()?;
    let mut gid_by_cid = HashMap::with_capacity(cids_by_gid.len());
    for (gid, cid) in cids_by_gid.iter().enumerate() {
        let gid = gid as u16;
        if gid == 0 {
            continue; // .notdef has no charset entry
        }
        if gid_by_cid.insert(*cid, gid).is_some() {
            return None; // duplicate CID — ambiguous reverse mapping
        }
    }
    Some(CidMapping::CffCharset {
        cids_by_gid,
        gid_by_cid,
    })
}

/// Annotation appearance streams carry their own content (and resources)
/// that may draw any embedded font — a context this pass never walks.
fn page_has_appearance_streams(document: &Document, page_dict: &Dictionary) -> bool {
    let Ok(Object::Array(annotations)) = page_dict.get(b"Annots") else {
        return false;
    };
    annotations.iter().any(|annotation| {
        let annotation_dict = match annotation {
            Object::Reference(annotation_id) => match document.objects.get(annotation_id) {
                Some(Object::Dictionary(dict)) => Some(dict),
                _ => None,
            },
            Object::Dictionary(dict) => Some(dict),
            _ => None,
        };
        annotation_dict.is_some_and(|dict| dict.get(b"AP").is_ok())
    })
}

/// Map each embedded TrueType program to every font dictionary consuming
/// it: `Type0` chains land on their descendant CIDFont id, simple fonts on
/// their own id. A program shared with a font outside the candidate set
/// must not be subset — the other consumer's glyph numbering would break.
fn font_file_consumers(document: &Document) -> HashMap<ObjectId, Vec<ObjectId>> {
    let mut consumers: HashMap<ObjectId, Vec<ObjectId>> = HashMap::new();
    for (&object_id, object) in &document.objects {
        let Object::Dictionary(font_dict) = object else {
            continue;
        };
        if !matches!(font_dict.get(b"Type"), Ok(Object::Name(kind)) if kind.as_slice() == b"Font")
        {
            continue;
        }
        if matches!(font_dict.get(b"Subtype"), Ok(Object::Name(kind)) if kind.as_slice() == b"Type0")
        {
            let Some(descendant_id) = font_dict
                .get(b"DescendantFonts")
                .ok()
                .and_then(|entry| entry.as_array().ok())
                .and_then(|entries| entries.first().cloned())
                .and_then(|entry| entry.as_reference().ok())
            else {
                continue;
            };
            let Some(Object::Dictionary(descendant)) = document.objects.get(&descendant_id)
            else {
                continue;
            };
            if let Some(font_file_id) = font_file_of(document, descendant) {
                consumers.entry(font_file_id).or_default().push(descendant_id);
            }
        } else if let Some(font_file_id) = font_file_of(document, font_dict) {
            // Simple (non-CID) fonts embed FontFile2 directly.
            consumers.entry(font_file_id).or_default().push(object_id);
        }
    }
    consumers
}

/// `FontDescriptor → FontFile2/FontFile3` reference of one font or CIDFont
/// dictionary (either key — the consumer map must see every program).
fn font_file_of(document: &Document, font_dict: &Dictionary) -> Option<ObjectId> {
    let descriptor_id = font_dict
        .get(b"FontDescriptor")
        .ok()?
        .as_reference()
        .ok()?;
    let Some(Object::Dictionary(descriptor)) = document.objects.get(&descriptor_id) else {
        return None;
    };
    for key in [b"FontFile2".as_slice(), b"FontFile3".as_slice()] {
        if let Ok(Object::Reference(id)) = descriptor.get(key) {
            return Some(*id);
        }
    }
    None
}

/// Resolve a page's `/Resources`, climbing `/Parent` for inherited entries.
fn effective_page_resources(document: &Document, page_id: ObjectId) -> Option<&Dictionary> {
    let mut current = page_id;
    let mut hops = 0;
    loop {
        let Object::Dictionary(page_dict) = document.objects.get(&current)? else {
            return None;
        };
        match page_dict.get(b"Resources") {
            Ok(Object::Dictionary(resources)) => return Some(resources),
            Ok(Object::Reference(resources_id)) => {
                match document.objects.get(resources_id) {
                    Some(Object::Dictionary(resources)) => return Some(resources),
                    _ => return None,
                }
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

/// Walk one content stream (and its form XObjects), attributing the CIDs in
/// every shown string to the Type0 font that is current at that point.
/// Returns false when a `Tf` name cannot be resolved here — the caller aborts.
fn collect_context_cids(
    document: &Document,
    content: &Content,
    resources: &Dictionary,
    candidates: &HashMap<ObjectId, Type0Candidate>,
    used_cids: &mut HashMap<ObjectId, HashSet<u16>>,
    visited_forms: &mut HashSet<ObjectId>,
) -> Result<bool, AppError> {
    let font_dict = resources
        .get(b"Font")
        .ok()
        .and_then(|entry| match entry {
            Object::Dictionary(dict) => Some(dict),
            Object::Reference(dict_id) => match document.objects.get(dict_id) {
                Some(Object::Dictionary(dict)) => Some(dict),
                _ => None,
            },
            _ => None,
        });

    // name → Type0 object id for the fonts of this context we can subset.
    let mut subsettable_here: HashMap<Vec<u8>, ObjectId> = HashMap::new();
    if let Some(font_dict) = font_dict {
        for (name, entry) in font_dict {
            if let Ok(font_id) = entry.as_reference() {
                if candidates.contains_key(&font_id) {
                    subsettable_here.insert(name.clone(), font_id);
                }
            }
        }
    }

    let mut current_type0: Option<ObjectId> = None;
    for operation in &content.operations {
        match operation.operator.as_str() {
            "Tf" => {
                let Some(Object::Name(name)) = operation.operands.first() else {
                    continue;
                };
                current_type0 = subsettable_here.get(name).copied();
                // A font name outside the subsettable set is only dangerous
                // when it does not resolve at all — it may fall back to this
                // context through inheritance elsewhere.
                if current_type0.is_none() {
                    let Some(font_dict) = font_dict else {
                        return Ok(false);
                    };
                    let Ok(font_entry) = font_dict.get(name.as_slice()) else {
                        return Ok(false);
                    };
                    // Type3 glyph procedures are content streams that may
                    // select and draw the candidate fonts — a context this
                    // pass never walks, so one Type3 anywhere aborts.
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
                        return Ok(false);
                    }
                }
            }
            "Tj" | "'" | "\"" | "TJ" => {
                let Some(font_id) = current_type0 else {
                    continue;
                };
                for operand in &operation.operands {
                    match operand {
                        Object::String(bytes, _) => {
                            record_cid_bytes(bytes, font_id, used_cids);
                        }
                        Object::Array(items) => {
                            for item in items {
                                if let Object::String(bytes, _) = item {
                                    record_cid_bytes(bytes, font_id, used_cids);
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            "Do" => {
                let Some(Object::Name(name)) = operation.operands.first() else {
                    continue;
                };
                let Some(xobject_dict) = resources
                    .get(b"XObject")
                    .ok()
                    .and_then(|entry| match entry {
                        Object::Dictionary(dict) => Some(dict),
                        Object::Reference(dict_id) => match document.objects.get(dict_id) {
                            Some(Object::Dictionary(dict)) => Some(dict),
                            _ => None,
                        },
                        _ => None,
                    })
                else {
                    return Ok(false);
                };
                let Some(Object::Reference(form_id)) = xobject_dict.get(name.as_slice()).ok()
                else {
                    return Ok(false);
                };
                if !visited_forms.insert(*form_id) {
                    continue;
                }
                let Some(Object::Stream(form)) = document.objects.get(form_id) else {
                    return Ok(false);
                };
                if !matches!(form.dict.get(b"Subtype"), Ok(Object::Name(kind)) if kind.as_slice() == b"Form")
                {
                    continue;
                }
                // A form without its own /Resources may resolve fonts against
                // the page's — unresolvable, so abort.
                let Some(form_resources) = form
                    .dict
                    .get(b"Resources")
                    .ok()
                    .and_then(|entry| match entry {
                        Object::Dictionary(dict) => Some(dict),
                        Object::Reference(dict_id) => match document.objects.get(dict_id) {
                            Some(Object::Dictionary(dict)) => Some(dict),
                            _ => None,
                        },
                        _ => None,
                    })
                else {
                    return Ok(false);
                };
                let form_content = match decode_form_content(document, *form_id) {
                    Some(decoded) => decoded,
                    None => return Ok(false),
                };
                if !collect_context_cids(
                    document,
                    &form_content,
                    form_resources,
                    candidates,
                    used_cids,
                    visited_forms,
                )? {
                    return Ok(false);
                }
            }
            _ => {}
        }
    }

    Ok(true)
}

fn record_cid_bytes(bytes: &[u8], font_id: ObjectId, used_cids: &mut HashMap<ObjectId, HashSet<u16>>) {
    let entry = used_cids.entry(font_id).or_default();
    for pair in bytes.chunks_exact(2) {
        entry.insert(u16::from_be_bytes([pair[0], pair[1]]));
    }
}

fn decode_form_content(document: &Document, form_id: ObjectId) -> Option<Content> {
    let Object::Stream(form) = document.objects.get(&form_id)? else {
        return None;
    };
    let bytes = form.get_plain_content().ok()?;
    Content::decode(&bytes).ok()
}

/// CID → glyph id through the font's own mapping. `None` only for a CFF
/// charset that does not know the CID — the caller then skips the whole
/// program (that CID must keep resolving against the original font).
fn cid_to_gid(candidate: &Type0Candidate, cid: u16) -> Option<u16> {
    match &candidate.mapping {
        CidMapping::Stream(map) => {
            let offset = cid as usize * 2;
            if offset + 2 <= map.len() {
                Some(u16::from_be_bytes([map[offset], map[offset + 1]]))
            } else {
                // A short custom map falls back to identity for the tail —
                // the established CIDFontType2 behavior.
                Some(cid)
            }
        }
        CidMapping::Identity => Some(cid),
        CidMapping::CffCharset { gid_by_cid, .. } => gid_by_cid.get(&cid).copied(),
    }
}

/// Point the descendant CIDFont at the subset: generate a `/CIDToGIDMap`
/// stream covering every used CID. `/W` is left untouched — widths are
/// keyed by CID (ISO 32000, table 115) and the content stream's CIDs are
/// unchanged, so the original array remains correct for every renderer.
fn rewire_descendant_font(
    document: &mut Document,
    candidate: &Type0Candidate,
    cids: &HashSet<u16>,
    remapper: &subsetter::GlyphRemapper,
) -> bool {
    let max_cid = match cids.iter().max() {
        Some(&max_cid) => max_cid,
        None => return false,
    };

    // CIDToGIDMap stream: big-endian new gid per CID, 0..=max_cid.
    let mut map_bytes = vec![0u8; (max_cid as usize + 1) * 2];
    for &cid in cids {
        // TrueType mappings always resolve (identity fallback); the CFF
        // misses were filtered out before the program was rewritten.
        let gid = cid_to_gid(candidate, cid).unwrap_or(cid);
        let Some(new_gid) = remapper.get(gid) else {
            continue;
        };
        let offset = cid as usize * 2;
        map_bytes[offset..offset + 2].copy_from_slice(&new_gid.to_be_bytes());
    }
    let map_stream = Stream::new(
        dictionary! {
            "Length" => (map_bytes.len() as i64),
        },
        map_bytes,
    );

    let map_id = document.add_object(map_stream);
    let Some(Object::Dictionary(descendant)) = document.objects.get_mut(&candidate.descendant_id)
    else {
        return false;
    };
    descendant.set("CIDToGIDMap", Object::Reference(map_id));

    true
}
