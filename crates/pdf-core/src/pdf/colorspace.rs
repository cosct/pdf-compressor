//! Color-space resolution for raw image streams — ICC-based and indexed
//! sources, plus resource-dictionary name aliases.
//! 原始图像流的色彩空间解析 — ICC 与 Indexed 来源，及资源字典名字别名。
//!
//! `optimize_image_stream` runs on streams that were moved out of the
//! document, so everything the decoder needs beyond the stream dictionary
//! must be resolved up front here, while the document is still intact: the
//! `/N` of an ICC profile stream, the lookup table of an indexed image, and
//! `/ColorSpace` name aliases pointing at resource-dictionary entries. The
//! resolved shape also decides what `/ColorSpace` the rebuilt stream should
//! carry so the pixel interpretation survives the rewrite (an ICC-based
//! image keeps its profile reference instead of silently degrading to
//! DeviceRGB).
//! `optimize_image_stream` 处理的是已移出文档的流，解码所需的文档级信息
//! （ICC 流的 `/N`、Indexed 的查找表、指向资源字典的名字别名）必须在此处
//! 预先解析。解析结果同时决定重建流应携带的 `/ColorSpace`，让像素解释在
//! 重写后保持不变（ICC 图像保留其 profile 引用，而不是静默退化为 DeviceRGB）。

use std::collections::HashMap;
use std::sync::Arc;

use lopdf::{Dictionary, Document, Object, Stream};

/// What the raw decoder should assume, beyond what the stream dict itself
/// can express.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum DecodeColorSpace {
    /// 1 channel, 8 bpc — ICC N=1 or an equivalent alias.
    Gray,
    /// 3 channels, 8 bpc — ICC N=3 or an equivalent alias.
    Rgb,
    /// 4 channels, 8 bpc — ICC N=4, `DeviceCMYK`, or an equivalent alias.
    /// Decoded planes come back as RGB through the calibrated CMYK
    /// conversion (`cmyk::cmyk_samples_to_rgb`), so rebuilt streams declare
    /// a 3-channel space and never restore the original CMYK
    /// `/ColorSpace`. `icc` carries the embedded profile bytes when the
    /// space resolved from an ICC stream — extracted only under the
    /// `cmyk-cms` feature, where the CMS converter consumes them.
    Cmyk {
        icc: Option<Arc<Vec<u8>>>,
    },
    /// Palette image: index bytes expand to `base_channels` channels
    /// (1 = gray base, 3 = RGB base, 4 = CMYK base) at decode time.
    /// A CMYK base keeps its profile bytes in `base_icc` the same way.
    Indexed {
        base_channels: u8,
        palette: Vec<u8>,
        base_icc: Option<Arc<Vec<u8>>>,
    },
}

impl DecodeColorSpace {
    pub fn channel_count(&self) -> u8 {
        match self {
            Self::Gray => 1,
            Self::Rgb => 3,
            Self::Cmyk { .. } => 4,
            Self::Indexed { .. } => 3,
        }
    }
}

/// Fully resolved color-space context for one image stream.
#[derive(Clone, Debug)]
pub(crate) struct ImageColorSpaceInfo {
    pub decode: DecodeColorSpace,
    /// `/ColorSpace` value to write on the rebuilt stream, with the channel
    /// count it declares — kept when it matches the encoded plane so an
    /// ICC-based original does not silently lose its profile. `None` keeps
    /// the plane-derived Device name.
    pub rebuild_color_space: Option<(Object, u8)>,
}

/// Collect `/ColorSpace` entries from every resource dictionary in the
/// document as a name→value alias table. Names defined with conflicting
/// values are dropped (an image using such a name stays skipped).
pub(crate) fn collect_color_space_aliases(document: &Document) -> HashMap<Vec<u8>, Object> {
    let mut aliases: HashMap<Vec<u8>, Object> = HashMap::new();
    let mut ambiguous: Vec<Vec<u8>> = Vec::new();

    for object in document.objects.values() {
        // Resource dictionaries live in page and form dictionaries; streams
        // carry them in their dict.
        let resources: Option<&Object> = match object {
            Object::Dictionary(dict) => dict.get(b"Resources").ok(),
            Object::Stream(stream) => stream.dict.get(b"Resources").ok(),
            _ => None,
        };
        let Some(resources_dict) = resources.and_then(|entry| match entry {
            Object::Dictionary(dict) => Some(dict),
            Object::Reference(dict_id) => match document.objects.get(dict_id) {
                Some(Object::Dictionary(dict)) => Some(dict),
                _ => None,
            },
            _ => None,
        }) else {
            continue;
        };
        let Some(color_spaces) = resources_dict
            .get(b"ColorSpace")
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
            continue;
        };

        for (name, value) in color_spaces {
            match aliases.get(name) {
                Some(existing) if !values_equal(existing, value) => {
                    ambiguous.push(name.clone());
                }
                Some(_) => {}
                None => {
                    aliases.insert(name.clone(), value.clone());
                }
            }
        }
    }

    for name in ambiguous {
        aliases.remove(&name);
    }
    aliases
}

fn values_equal(left: &Object, right: &Object) -> bool {
    match (left, right) {
        (Object::Name(a), Object::Name(b)) => a == b,
        (Object::Integer(a), Object::Integer(b)) => a == b,
        (Object::Array(a), Object::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(x, y)| values_equal(x, y))
        }
        (Object::Reference(a), Object::Reference(b)) => a == b,
        _ => false,
    }
}

/// Resolve a raw image stream's `/ColorSpace` into decoder instructions.
/// Returns `None` for plain device names (the dict is self-sufficient) and
/// for anything unsupported (the caller's existing skip logic still fires).
pub(crate) fn resolve_image_color_space(
    document: &Document,
    stream: &Stream,
    aliases: &HashMap<Vec<u8>, Object>,
) -> Option<ImageColorSpaceInfo> {
    let declared = match stream.dict.get(b"ColorSpace") {
        Ok(value) => value.clone(),
        Err(_) => return None, // absent defaults to DeviceRGB — dict-driven
    };

    // Name: a device space (handled by the dict-driven path) or an alias.
    let value = match &declared {
        Object::Name(name)
            if matches!(name.as_slice(), b"DeviceGray" | b"DeviceRGB") =>
        {
            return None;
        }
        Object::Name(name) => aliases.get(name)?.clone(),
        Object::Array(_) => declared.clone(),
        _ => return None,
    };

    resolve_value(document, &value)
}

fn resolve_value(document: &Document, value: &Object) -> Option<ImageColorSpaceInfo> {
    match value {
        // [/ICCBased <ref>] — channel count from the profile stream's /N.
        Object::Array(items) if items.len() == 2 => {
            let (Object::Name(head), Object::Reference(profile_id)) = (&items[0], &items[1])
            else {
                return resolve_indexed(document, value);
            };
            if head.as_slice() != b"ICCBased" {
                return resolve_indexed(document, value);
            }
            let profile = document.objects.get(profile_id)?;
            let Object::Stream(profile) = profile else {
                return None;
            };
            let channels = optional_dict_integer(&profile.dict, b"N")?;
            match channels {
                1 => Some(ImageColorSpaceInfo {
                    decode: DecodeColorSpace::Gray,
                    rebuild_color_space: Some((value.clone(), 1)),
                }),
                3 => Some(ImageColorSpaceInfo {
                    decode: DecodeColorSpace::Rgb,
                    rebuild_color_space: Some((value.clone(), 3)),
                }),
                // CMYK planes are converted to RGB at decode time, so the
                // profile must not ride along on the rebuilt 3-channel
                // stream. The bytes themselves ride along instead — the
                // `cmyk-cms` converter converts through the embedded profile
                // exactly like the renderers do.
                4 => Some(ImageColorSpaceInfo {
                    decode: DecodeColorSpace::Cmyk {
                        icc: icc_profile_bytes(profile),
                    },
                    rebuild_color_space: None,
                }),
                // Exotic counts stay skipped.
                _ => None,
            }
        }
        Object::Array(_) => resolve_indexed(document, value),
        _ => None,
    }
}

/// [/Indexed <base> <hival> <lookup>] — expand at decode time; the rebuilt
/// stream declares the base space.
fn resolve_indexed(document: &Document, value: &Object) -> Option<ImageColorSpaceInfo> {
    let Object::Array(items) = value else {
        return None;
    };
    if items.len() != 4 {
        return None;
    }
    let Object::Name(head) = &items[0] else {
        return None;
    };
    if head.as_slice() != b"Indexed" {
        return None;
    }
    let Object::Integer(hival) = items[2] else {
        return None;
    };
    if !(0..=255).contains(&hival) {
        return None;
    }

    // Base space: device name or ICC-based. CMYK bases (4 channels) expand
    // to RGB at decode time, like every other CMYK path; the rebuilt stream
    // therefore never carries the CMYK base space.
    let (base_channels, rebuild_base, base_icc): (u8, Option<Object>, Option<Arc<Vec<u8>>>) =
        match &items[1] {
            Object::Name(base) if base.as_slice() == b"DeviceGray" => (1, None, None),
            Object::Name(base) if base.as_slice() == b"DeviceRGB" => (3, None, None),
            Object::Name(base) if base.as_slice() == b"DeviceCMYK" => (4, None, None),
            Object::Array(_) => {
                let resolved = resolve_value(document, &items[1])?;
                let channels = resolved.decode.channel_count();
                // Preserve an ICC-based base on the rebuilt stream — unless
                // the decode converts to RGB (CMYK), where it would
                // misdescribe the plane. The CMYK base's profile bytes stay
                // available for the conversion itself.
                let (keep, icc) = match resolved.decode {
                    DecodeColorSpace::Cmyk { icc } => (None, icc),
                    _ => (
                        resolved.rebuild_color_space.map(|(object, _)| object),
                        None,
                    ),
                };
                (channels, keep, icc)
            }
            _ => return None,
        };

    // Lookup: an inline string or a (possibly compressed) stream.
    let lookup: Vec<u8> = match &items[3] {
        Object::String(bytes, _) => bytes.clone(),
        Object::Reference(lookup_id) => {
            let lookup_object = document.objects.get(lookup_id)?;
            let Object::Stream(lookup) = lookup_object else {
                return None;
            };
            lookup.get_plain_content().ok()?
        }
        _ => return None,
    };

    let needed = (hival as usize + 1) * base_channels as usize;
    if lookup.len() < needed {
        // Spec-short tables are tolerated by readers; pad with zeros rather
        // than refusing a legitimately compressible image.
        let mut padded = lookup;
        padded.resize(needed, 0);
        return Some(ImageColorSpaceInfo {
            decode: DecodeColorSpace::Indexed {
                base_channels,
                palette: padded,
                base_icc,
            },
            rebuild_color_space: rebuild_base.map(|object| (object, base_channels)),
        });
    }

    Some(ImageColorSpaceInfo {
        decode: DecodeColorSpace::Indexed {
            base_channels,
            palette: lookup[..needed].to_vec(),
            base_icc,
        },
        rebuild_color_space: rebuild_base.map(|object| (object, base_channels)),
    })
}

/// Extract an ICC profile stream's bytes for the CMYK converter. Size-capped
/// (real profiles are well under 2 MiB; anything bigger is misplaced data)
/// and only compiled in with `cmyk-cms` — feature-off builds never pay the
/// extraction or the memory.
#[cfg(feature = "cmyk-cms")]
fn icc_profile_bytes(profile: &Stream) -> Option<Arc<Vec<u8>>> {
    const MAX_ICC_BYTES: usize = 4 << 20;
    // A valid profile starts with a 128-byte header.
    let bytes = profile.get_plain_content().ok()?;
    (128..=MAX_ICC_BYTES).contains(&bytes.len()).then(|| Arc::new(bytes))
}

#[cfg(not(feature = "cmyk-cms"))]
fn icc_profile_bytes(_profile: &Stream) -> Option<Arc<Vec<u8>>> {
    None
}

fn optional_dict_integer(dict: &Dictionary, key: &[u8]) -> Option<i64> {
    match dict.get(key) {
        Ok(Object::Integer(value)) => Some(*value),
        _ => None,
    }
}

/// Decode palette index bytes (4 or 8 bits per component) into index bytes.
/// 4-bit rows start on byte boundaries — an odd-width row ends with a
/// padding nibble that is not a pixel, so unpacking needs the row width.
pub(crate) fn unpack_indices(data: &[u8], bits: i64, width: u32) -> Option<Vec<u8>> {
    match bits {
        8 => Some(data.to_vec()),
        4 => {
            let row_bytes = (width as usize).div_ceil(2);
            let row_width = width as usize;
            let mut indices = Vec::with_capacity(data.len() * 2);
            for row in data.chunks(row_bytes) {
                indices.extend(
                    row.iter()
                        .flat_map(|byte| [byte >> 4, byte & 0x0F])
                        .take(row_width),
                );
            }
            Some(indices)
        }
        _ => None,
    }
}
