//! Font subsetting — shrink embedded `Type0`/`CIDFontType2` TrueType fonts to
//! the glyphs the content streams actually draw.
//! 字体子集化 — 将内嵌的 `Type0`/`CIDFontType2` TrueType 字体裁剪到内容流实际
//! 绘制的字形。
//!
//! The typst `subsetter` crate retains glyphs by id and strips `cmap`, so a
//! subset can only be consumed as a CID font. That is exactly what we have:
//! content strings keep their original 2-byte CIDs untouched, and the new
//! glyph numbering is bridged with a generated `/CIDToGIDMap` stream plus a
//! remapped `/W` width array. Nothing outside the font object tree is
//! rewritten. Fail-closed: any font with an unusual shape (non-Identity
//! encoding, non-TrueType outlines, unreadable CIDToGIDMap, subsetting
//! failure, or no size win) keeps its original program, and one ambiguous
//! `Tf` name anywhere aborts the whole pass.
//! typst 的 `subsetter` 按字形 id 保留并剥离 `cmap`，子集只能作 CID 字体使用——
//! 这正合适：内容流的 2 字节 CID 原样保留，用生成的 `/CIDToGIDMap` 流与重映射的
//! `/W` 宽度数组桥接新字形编号，字体对象树之外零改写。任何异常形态（非 Identity
//! 编码、非 TrueType 轮廓、CIDToGIDMap 不可读、子集化失败、无体积收益）都保留
//! 原字体；任一处出现无法解析的 `Tf` 名字则整体放弃本轮。

use std::collections::{HashMap, HashSet};
use std::sync::{atomic::AtomicBool, Arc};

use lopdf::content::Content;
use lopdf::{dictionary, Dictionary, Document, Object, ObjectId, Stream};

use super::ensure_not_cancelled;
use crate::error::AppError;

/// Outcome of one subsetting pass.
#[derive(Debug, Default)]
pub(crate) struct FontSubsetOutcome {
    pub fonts_subsetted: usize,
    pub bytes_saved: u64,
}

/// A `Type0 → CIDFontType2 → FontFile2` chain eligible for subsetting.
#[derive(Debug)]
struct Type0Candidate {
    descendant_id: ObjectId,
    font_file_id: ObjectId,
    /// Raw `/CIDToGIDMap` stream bytes when the font carries a custom map;
    /// `None` means identity (absent or `/Identity`).
    custom_cid_to_gid: Option<Vec<u8>>,
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

    let mut outcome = FontSubsetOutcome::default();
    for (font_file_id, type0_ids) in files {
        ensure_not_cancelled(cancel_flag, task_id)?;

        // Union of the glyph ids every sharing font draws.
        let mut old_gids: HashSet<u16> = HashSet::new();
        old_gids.insert(0); // .notdef must survive
        for type0_id in &type0_ids {
            let Some(candidate) = candidates.get(type0_id) else {
                continue;
            };
            let Some(cids) = used_cids.get(type0_id) else {
                continue;
            };
            for cid in cids {
                old_gids.insert(cid_to_gid(candidate, *cid));
            }
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
        let subset = match subsetter::subset(&original, 0, &remapper) {
            Ok(subset) => subset,
            Err(_) => continue,
        };
        if subset.len() >= original.len() {
            continue;
        }

        // Replace the font program in place (same object id, same dict shape).
        let Some(Object::Stream(font_file)) = document.objects.get_mut(&font_file_id) else {
            continue;
        };
        font_file.dict.remove(b"Filter");
        font_file.dict.remove(b"DecodeParms");
        font_file
            .dict
            .set("Length1", Object::Integer(subset.len() as i64));
        font_file.set_content(subset.clone());
        outcome.bytes_saved += (original.len() - subset.len()) as u64;

        // Bridge each font's unchanged CIDs to the new glyph numbering.
        for type0_id in &type0_ids {
            let Some(candidate) = candidates.get(type0_id) else {
                continue;
            };
            let Some(cids) = used_cids.get(type0_id) else {
                continue;
            };
            if !rewire_descendant_font(document, candidate, cids, &remapper) {
                continue;
            }
            outcome.fonts_subsetted += 1;
        }
    }

    Ok(outcome)
}

/// Find `Type0 → CIDFontType2 → FontFile2` chains with an Identity encoding.
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

        // DescendantFonts[0] must be a CIDFontType2 (TrueType outlines).
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
        if !matches!(descendant.get(b"Subtype"), Ok(Object::Name(kind)) if kind.as_slice() == b"CIDFontType2")
        {
            continue;
        }

        // FontDescriptor → FontFile2 must be an embedded TrueType program.
        let Some(font_file_id) = descendant
            .get(b"FontDescriptor")
            .ok()
            .and_then(|entry| entry.as_reference().ok())
            .and_then(|descriptor_id| {
                let descriptor = match document.objects.get(&descriptor_id) {
                    Some(Object::Dictionary(descriptor)) => descriptor,
                    _ => return None,
                };
                descriptor.get(b"FontFile2").ok()?.as_reference().ok()
            })
        else {
            continue;
        };

        // A custom /CIDToGIDMap stream is readable; anything exotic skips.
        let custom_cid_to_gid = match descendant.get(b"CIDToGIDMap") {
            Ok(Object::Name(name)) if name.as_slice() == b"Identity" => None,
            Ok(Object::Reference(map_id)) => {
                let Some(Object::Stream(map)) = document.objects.get(map_id) else {
                    continue;
                };
                match map.get_plain_content() {
                    Ok(bytes) => Some(bytes),
                    Err(_) => continue,
                }
            }
            // Absent defaults to the identity mapping per the PDF spec.
            Err(_) => None,
            Ok(_) => continue,
        };

        candidates.insert(
            object_id,
            Type0Candidate {
                descendant_id,
                font_file_id,
                custom_cid_to_gid,
            },
        );
    }
    candidates
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
                    if font_dict.get(name.as_slice()).is_err() {
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

/// CID → glyph id through the font's own mapping (identity by default).
fn cid_to_gid(candidate: &Type0Candidate, cid: u16) -> u16 {
    match &candidate.custom_cid_to_gid {
        Some(map) => {
            let offset = cid as usize * 2;
            if offset + 2 <= map.len() {
                u16::from_be_bytes([map[offset], map[offset + 1]])
            } else {
                cid
            }
        }
        None => cid,
    }
}

/// Point the descendant CIDFont at the subset: generate a `/CIDToGIDMap`
/// stream covering every used CID and remap `/W` widths to the new glyph ids.
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
        let gid = cid_to_gid(candidate, cid);
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

    let Some(Object::Dictionary(descendant)) = document.objects.get(&candidate.descendant_id)
    else {
        return false;
    };

    // Collect old widths (cid → width object) before renumbering.
    let old_widths = match descendant.get(b"W") {
        Ok(Object::Array(entries)) => parse_width_array(entries),
        _ => Vec::new(),
    };

    let mut new_widths: Vec<Object> = Vec::new();
    let mut sorted_cids: Vec<u16> = cids.iter().copied().collect();
    sorted_cids.sort_unstable();
    for &cid in &sorted_cids {
        let gid = cid_to_gid(candidate, cid);
        let Some(new_gid) = remapper.get(gid) else {
            continue;
        };
        if let Some((_, width)) = old_widths.iter().find(|(old_cid, _)| *old_cid == cid) {
            new_widths.push(Object::Integer(i64::from(new_gid)));
            new_widths.push(Object::Array(vec![width.clone()]));
        }
    }

    let map_id = document.add_object(map_stream);
    let Some(Object::Dictionary(descendant)) = document.objects.get_mut(&candidate.descendant_id)
    else {
        return false;
    };
    descendant.set("CIDToGIDMap", Object::Reference(map_id));
    if !new_widths.is_empty() {
        descendant.set("W", Object::Array(new_widths));
    }

    true
}

/// Parse a `/W` width array into `(cid, width)` pairs. Width objects are kept
/// verbatim (Integer or Real) to avoid any float re-serialization drift.
fn parse_width_array(entries: &[Object]) -> Vec<(u16, Object)> {
    let mut widths = Vec::new();
    let mut index = 0;

    let as_int = |object: &Object| -> Option<u16> {
        match object {
            Object::Integer(value) if (0..=65_535).contains(value) => Some(*value as u16),
            _ => None,
        }
    };

    while index < entries.len() {
        let Some(start) = as_int(&entries[index]) else {
            break;
        };
        index += 1;
        match entries.get(index) {
            Some(Object::Array(values)) => {
                for (offset, value) in values.iter().enumerate() {
                    if let Some(cid) = start.checked_add(offset as u16) {
                        widths.push((cid, value.clone()));
                    }
                }
                index += 1;
            }
            Some(first) if as_int(first).is_some() => {
                let first = as_int(first).unwrap_or_default();
                index += 1;
                match entries.get(index) {
                    Some(Object::Array(values)) => {
                        for (offset, value) in values.iter().enumerate() {
                            if let Some(cid) = first.checked_add(offset as u16) {
                                widths.push((cid, value.clone()));
                            }
                        }
                        index += 1;
                    }
                    Some(width) => {
                        // Constant width for start..=first.
                        if first >= start {
                            for cid in start..=first {
                                widths.push((cid, width.clone()));
                            }
                        }
                        index += 1;
                    }
                    None => break,
                }
            }
            _ => break,
        }
    }

    widths
}
