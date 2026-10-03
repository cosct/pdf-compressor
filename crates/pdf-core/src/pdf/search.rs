//! Target-size search state — per-image caches, probe rounds, and the final
//! materialization of winning parameters.
//! 目标大小搜索状态 — 逐图缓存、探测轮次与获胜参数的最终物化。

use std::collections::{HashMap, HashSet};
use std::sync::{atomic::AtomicBool, Arc};

use lopdf::{Document, Object, ObjectId, Stream};

use super::compressor::{record_image_skip, take_image_tasks, CompressionStats, ImageTask};
use super::encode::{optimize_image_stream, ImageOptimization, ImageSearchCache, SkipPolicy};
use super::settings::CompressionSettings;
use crate::error::AppError;

/// Total budget for everything the search caches across images — decoded
/// color planes, alpha planes, and memoized alpha/G4 products. Above it,
/// caches are evicted (color planes first, products second) and re-derived
/// next round; the re-decode cost is the price of a bounded memory ceiling.
const BITMAP_CACHE_TOTAL_BUDGET_BYTES: u64 = if cfg!(target_os = "android") {
    64 * 1024 * 1024
} else {
    768 * 1024 * 1024
};

/// One image object tracked across probe rounds: the moved-out original
/// streams plus the caches and the latest completed encoding.
pub(crate) struct ImageSearchEntry {
    pub object_id: ObjectId,
    pub stream: Stream,
    /// `(original object id, stream)` of the `/SMask`, cloned when the mask
    /// is shared by several images.
    pub smask: Option<(ObjectId, Stream)>,
    /// Resolved color-space context (ICC/Indexed/aliases) carried over from
    /// the preparation pass.
    pub color_space: Option<super::colorspace::ImageColorSpaceInfo>,
    /// `/JBIG2Globals` segment bytes carried over from the preparation pass.
    pub jbig2_globals: Option<Arc<Vec<u8>>>,
    pub cache: ImageSearchCache,
    last: Option<LastEncoding>,
    cache_budget: u64,
}

struct LastEncoding {
    params: RoundParams,
    optimization: ImageOptimization,
}

/// Probe parameters for one target-size search round.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct RoundParams {
    pub quality: u8,
    pub edge: u16,
}

/// Shared per-run context for probing and materializing search entries.
#[derive(Clone, Copy)]
pub(crate) struct SearchContext<'a> {
    pub settings: &'a CompressionSettings,
    pub skip_policy: SkipPolicy,
    pub cancel_flag: &'a Arc<AtomicBool>,
    pub task_id: &'a str,
}

/// Move the image objects (and their exclusive soft masks) out of the
/// document as search entries, largest first. Mirrors `take_image_tasks`.
pub(crate) fn take_image_search_entries(
    document: &mut Document,
    image_object_ids: &[ObjectId],
    shared_smask_ids: &HashSet<ObjectId>,
    color_spaces: &HashMap<ObjectId, super::colorspace::ImageColorSpaceInfo>,
    jbig2_globals: &HashMap<ObjectId, Arc<Vec<u8>>>,
) -> Vec<ImageSearchEntry> {
    let cache_budget = BITMAP_CACHE_TOTAL_BUDGET_BYTES / image_object_ids.len().max(1) as u64;
    take_image_tasks(
        document,
        image_object_ids,
        shared_smask_ids,
        color_spaces,
        jbig2_globals,
    )
    .into_iter()
    .map(|task: ImageTask| ImageSearchEntry {
        object_id: task.object_id,
        stream: task.stream,
        smask: task.smask,
        color_space: task.color_space,
        jbig2_globals: task.jbig2_globals,
        cache: ImageSearchCache::default(),
        last: None,
        cache_budget,
    })
    .collect()
}

/// Run one probe round for a single image at `params`; fills the caches and
/// stores the resulting optimization for a later materialize at the same
/// parameters. Returns the image's byte contribution to the output:
/// re-encoded color + alpha bytes, or the untouched stream bytes when skipped.
pub(crate) fn probe_image_at(
    entry: &mut ImageSearchEntry,
    params: RoundParams,
    context: &SearchContext<'_>,
) -> Result<usize, AppError> {
    let round_settings = CompressionSettings {
        image_quality: params.quality,
        max_image_size_px: params.edge,
        ..context.settings.clone()
    };
    let optimization = optimize_image_stream(
        &entry.stream,
        entry.smask.as_ref().map(|(_, smask)| smask),
        &round_settings,
        context.cancel_flag,
        context.task_id,
        context.skip_policy,
        Some(&mut entry.cache),
        entry.color_space.as_ref(),
        entry.jbig2_globals.as_deref().map(Vec::as_slice),
    )?;
    // Every entry owns a fixed share of the retained-cache budget. Trim
    // before sending a worker result, not at the end of the document round.
    // Transient decoding still costs at most one bounded image per worker;
    // this is a retained-cache limit, not a process RSS guarantee.
    if entry.cache.cached_bytes() > entry.cache_budget {
        entry.cache.drop_bitmap();
    }
    if entry.cache.cached_bytes() > entry.cache_budget {
        entry.cache.drop_products();
    }
    let contribution = image_contribution(entry, &optimization);
    entry.last = Some(LastEncoding {
        params,
        optimization,
    });
    Ok(contribution)
}

/// Bytes this image adds to the serialized output for `optimization`.
/// Object framing (dictionaries, xref entries) is constant per image and
/// absorbed by the caller's baseline constant.
fn image_contribution(entry: &ImageSearchEntry, optimization: &ImageOptimization) -> usize {
    match optimization {
        ImageOptimization::Recompressed { stream, smask } => {
            stream.content.len() + smask.as_ref().map_or(0, |mask| mask.content.len())
        }
        ImageOptimization::Skipped { .. } => {
            entry.stream.content.len()
                + entry
                    .smask
                    .as_ref()
                    .map_or(0, |(_, mask)| mask.content.len())
        }
    }
}

/// Apply the winning parameters to the document: reuses the stored last-round
/// encoding when the parameters match (no re-encode), otherwise runs one more
/// probe from the caches. Entries stay intact, so the search can continue
/// after an over-budget materialization.
pub(crate) fn materialize_image_entry(
    document: &mut Document,
    entry: &mut ImageSearchEntry,
    params: RoundParams,
    context: &SearchContext<'_>,
    stats: &mut CompressionStats,
) -> Result<(), AppError> {
    if !entry
        .last
        .as_ref()
        .is_some_and(|last| last.params == params)
    {
        probe_image_at(entry, params, context)?;
    }

    let optimization = &entry
        .last
        .as_ref()
        .expect("probe stored an encoding")
        .optimization;
    match optimization {
        ImageOptimization::Recompressed { stream, smask } => {
            let mut rebuilt = stream.clone();
            if rebuilt.dict.get(b"Filter").is_ok_and(|filter| {
                matches!(filter, Object::Name(name) if name.as_slice() == b"CCITTFaxDecode")
            }) {
                stats.images_bilevel_encoded += 1;
            }
            if let Some(smask_stream) = smask {
                let smask_id = document.add_object(smask_stream.clone());
                rebuilt.dict.set("SMask", Object::Reference(smask_id));
            }
            document
                .objects
                .insert(entry.object_id, Object::Stream(rebuilt));
            stats.images_recompressed += 1;
        }
        ImageOptimization::Skipped { reason } => {
            document
                .objects
                .insert(entry.object_id, Object::Stream(entry.stream.clone()));
            if let Some((smask_id, smask_stream)) = &entry.smask {
                document
                    .objects
                    .insert(*smask_id, Object::Stream(smask_stream.clone()));
            }
            record_image_skip(stats, entry.object_id, reason.clone());
        }
    }
    Ok(())
}

/// Keep the search's cached bytes under the budget, in two stages: first the
/// largest color planes are evicted (they dominate the total; evicted images
/// simply re-decode next round), then — if the products alone still exceed
/// the budget — the alpha/G4 memoizations go as well. The sticky undecodable
/// memos are plain strings and never evicted.
pub(crate) fn enforce_bitmap_cache_budget(entries: &mut [ImageSearchEntry]) {
    let mut sized: Vec<(usize, u64)> = entries
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| {
            entry
                .cache
                .cached_bitmap_bytes()
                .map(|bytes| (index, bytes))
        })
        .collect();

    let mut total: u64 = entries.iter().map(|entry| entry.cache.cached_bytes()).sum();
    if total <= BITMAP_CACHE_TOTAL_BUDGET_BYTES {
        return;
    }

    // Stage 1: color planes, largest first.
    sized.sort_unstable_by_key(|&(_, size)| std::cmp::Reverse(size));
    for (index, size) in sized {
        if total <= BITMAP_CACHE_TOTAL_BUDGET_BYTES {
            break;
        }
        total = total.saturating_sub(size);
        entries[index].cache.drop_bitmap();
    }
    if total <= BITMAP_CACHE_TOTAL_BUDGET_BYTES {
        return;
    }

    // Stage 2: alpha and G4 products, largest contributor first.
    let mut product_sized: Vec<(usize, u64)> = entries
        .iter()
        .enumerate()
        .map(|(index, entry)| (index, entry.cache.cached_bytes()))
        .collect();
    product_sized.sort_unstable_by_key(|&(_, size)| std::cmp::Reverse(size));
    for (index, _) in product_sized {
        if total <= BITMAP_CACHE_TOTAL_BUDGET_BYTES {
            break;
        }
        let before = entries[index].cache.cached_bytes();
        entries[index].cache.drop_products();
        let after = entries[index].cache.cached_bytes();
        total = total.saturating_sub(before - after);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_evicts_before_return_and_reencoding_preserves_size() {
        let bytes = crate::testutil::jpeg_page_pdf_bytes(640, 480, 95);
        let mut document = Document::load_mem(&bytes).unwrap();
        let ids: Vec<_> = document
            .objects
            .iter()
            .filter_map(|(id, object)| {
                object
                    .as_stream()
                    .ok()
                    .filter(|stream| {
                        stream
                            .dict
                            .get(b"Subtype")
                            .and_then(Object::as_name)
                            .is_ok_and(|name| name == b"Image")
                    })
                    .map(|_| *id)
            })
            .collect();
        let mut entries = take_image_search_entries(
            &mut document,
            &ids,
            &HashSet::new(),
            &HashMap::new(),
            &HashMap::new(),
        );
        assert!(!entries.is_empty());
        let settings = CompressionSettings::from_sources(None, Default::default());
        let cancel = Arc::new(AtomicBool::new(false));
        let context = SearchContext {
            settings: &settings,
            skip_policy: SkipPolicy::for_document(entries.len(), false),
            cancel_flag: &cancel,
            task_id: "cache-test",
        };
        let params = RoundParams {
            quality: 55,
            edge: 400,
        };
        for entry in &mut entries {
            // Force eviction on a small fixture instead of allocating hundreds of MB.
            entry.cache_budget = 1;
            let first = probe_image_at(entry, params, &context).unwrap();
            assert!(
                entry.cache.cached_bytes() <= 1,
                "evict before the worker can enqueue this entry"
            );
            let second = probe_image_at(entry, params, &context).unwrap();
            assert_eq!(first, second);
            assert!(entry.cache.cached_bytes() <= 1);
        }
    }
}
