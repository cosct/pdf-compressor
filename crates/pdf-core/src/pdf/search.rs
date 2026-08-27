//! Target-size search state — per-image caches, probe rounds, and the final
//! materialization of winning parameters.
//! 目标大小搜索状态 — 逐图缓存、探测轮次与获胜参数的最终物化。

use std::{collections::HashSet, sync::{atomic::AtomicBool, Arc}};

use lopdf::{Document, Object, ObjectId, Stream};

use super::compressor::{record_image_skip, take_image_tasks, CompressionStats, ImageTask};
use super::encode::{optimize_image_stream, ImageOptimization, ImageSearchCache, SkipPolicy};
use super::settings::CompressionSettings;
use crate::error::AppError;

/// Total budget for all cached color planes during a target-size search.
/// Above it, the largest cached bitmaps are evicted and re-decode next round.
const BITMAP_CACHE_TOTAL_BUDGET_BYTES: u64 = 768 * 1024 * 1024;

/// One image object tracked across probe rounds: the moved-out original
/// streams plus the caches and the latest completed encoding.
pub(crate) struct ImageSearchEntry {
    pub object_id: ObjectId,
    pub stream: Stream,
    /// `(original object id, stream)` of the `/SMask`, cloned when the mask
    /// is shared by several images.
    pub smask: Option<(ObjectId, Stream)>,
    pub cache: ImageSearchCache,
    last: Option<LastEncoding>,
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
) -> Vec<ImageSearchEntry> {
    take_image_tasks(document, image_object_ids, shared_smask_ids)
        .into_iter()
        .map(|task: ImageTask| ImageSearchEntry {
            object_id: task.object_id,
            stream: task.stream,
            smask: task.smask,
            cache: ImageSearchCache::default(),
            last: None,
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
    )?;
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

/// Drop the largest cached bitmaps until the total stays under the budget.
/// Evicted images simply re-decode on the next probe round.
pub(crate) fn enforce_bitmap_cache_budget(entries: &mut [ImageSearchEntry]) {
    let mut sized: Vec<(usize, u64)> = entries
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| entry.cache.cached_bitmap_bytes().map(|bytes| (index, bytes)))
        .collect();

    let mut total: u64 = sized.iter().map(|(_, size)| *size).sum();
    if total <= BITMAP_CACHE_TOTAL_BUDGET_BYTES {
        return;
    }

    sized.sort_unstable_by_key(|&(_, size)| std::cmp::Reverse(size));
    for (index, size) in sized {
        if total <= BITMAP_CACHE_TOTAL_BUDGET_BYTES {
            break;
        }
        total = total.saturating_sub(size);
        entries[index].cache.drop_bitmap();
    }
}
