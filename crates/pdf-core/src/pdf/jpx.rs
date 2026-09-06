//! JPX (JPEG 2000) image-stream decoding — the feature-gated C path.
//! JPX（JPEG 2000）图像流解码 — feature 门控的 C 解码路径。
//!
//! Decoding goes through the `jpeg2k` safe wrapper over `openjpeg-sys`, which
//! compiles vendored OpenJPEG with the plain `cc` crate and links it
//! statically: no cmake at build time and no runtime `libopenjp2` to ship in
//! the installers. The unsafe surface is therefore confined to the vendored
//! FFI bindings; this module only touches the safe API.
//!
//! Gate discipline mirrors the CCITT shape gate: `jpx_input_shape` decides
//! decodability from the stream dictionary alone (cheap, shared with the
//! analyzer via `stream_filter_info`), and `decode_jpx_stream` re-derives the
//! shape before touching C. Everything outside the proven-decodable shape —
//! flate-mixed filter chains, indirect nonstandard parameters, `/Decode`
//! arrays, subsampled or alpha-tagged components — stays skipped.
//!
//! PDF-specific rules (spec 8.9.5.1): a JPX stream is self-describing; the
//! dictionary `/ColorSpace` is advisory when the codestream carries its own
//! (via the JP2 header box). The decoder therefore trusts the decoded
//! component count — 1 → gray, 3 → RGB, 4 → CMYK (converted to RGB at decode
//! time through the shared calibrated path in `cmyk`). OpenJPEG
//! has already applied any codestream-internal multi-component transform
//! before the components reach us, matching how `opj_decompress` renders.

use image::DynamicImage;
use lopdf::Stream;

use super::optional_integer;
use crate::error::AppError;

/// Decodable JPX input shape, shared by the filter gate and the decoder.
#[derive(Clone, Copy, Debug)]
pub(super) struct JpxInputShape {
    width: u32,
    height: u32,
}

/// Validate that a JPX stream uses a shape this engine can decode. Returns
/// the shape on success. Rejected: missing/oversized dimensions (the ceiling
/// matches the JPEG format limit the re-encoder enforces), nonstandard
/// `/DecodeParms`, and `/Decode` component mappings (the rewritten JPEG would
/// inherit an interpretation that no longer matches its bytes). Filter chains
/// with more than one entry are rejected by the caller.
pub(super) fn jpx_input_shape(stream: &Stream) -> Option<JpxInputShape> {
    let width = optional_integer(stream, b"Width")?;
    let height = optional_integer(stream, b"Height")?;
    // Never let hostile dimensions reach the decoded-plane allocation: a
    // negative i64 would wrap to a huge value at the `as u32` cast.
    if !(1..=65_535).contains(&width) || !(1..=65_535).contains(&height) {
        return None;
    }
    // JPXDecode defines no /DecodeParms in the PDF spec; a stream carrying
    // them is nonstandard enough to leave alone.
    if stream.dict.get(b"DecodeParms").is_ok() {
        return None;
    }
    if stream.dict.get(b"Decode").is_ok() {
        return None;
    }
    Some(JpxInputShape {
        width: width as u32,
        height: height as u32,
    })
}

/// Decode a JPX image stream into an RGB or grayscale plane. The codestream
/// may be a raw J2K codestream or a JP2 file — `from_bytes` auto-detects —
/// because both forms appear inside PDFs. Subsampled components (per-
/// component dx/dy > 1, e.g. 4:2:0 chroma) come back from OpenJPEG at their
/// own reduced grid; they are upsampled bilinearly to the image grid. For
/// those codestreams a SYCC declaration (JP2 `colr` box) switches the
/// three-component assembly through the BT.601 YCbCr inverse — proper
/// JPEG 2000 semantics, since poppler outright refuses to render
/// subsampled components ("Component has different WxH than component 0")
/// and no renderer interpretation exists to match. Non-subsampled
/// codestreams keep the historical plane semantics untouched. Four-
/// component (CMYK) codestreams come back as RGB via the shared calibrated
/// conversion, but only
/// when `allow_cmyk` is set (the compressor's opt-in gate); otherwise they
/// fail cleanly so the image keeps its original stream.
pub(super) fn decode_jpx_stream(stream: &Stream, allow_cmyk: bool) -> Result<DynamicImage, AppError> {
    let shape = jpx_input_shape(stream)
        .ok_or_else(|| AppError::PdfBuild("JPX stream does not use a supported decode shape".into()))?;
    let image = jpeg2k::Image::from_bytes(&stream.content).map_err(|error| {
        AppError::PdfBuild(format!("failed to decode JPX image stream: {error:?}"))
    })?;

    // The codestream is authoritative for its own geometry; a mismatch with
    // the dictionary means the stream dict cannot be trusted for the rebuild.
    if image.width() != shape.width || image.height() != shape.height {
        return Err(AppError::PdfBuild(
            "JPX codestream dimensions do not match the stream dictionary".into(),
        ));
    }

    let components = image.components();
    // Alpha channels have no faithful rewrite path here — transparency
    // belongs to /SMask. Precision above 16 bits is exotic for scanned PDFs
    // and needlessly widens the scaling math. Subsampled components are
    // handled below; their geometry is no longer a rejection.
    if components
        .iter()
        .any(|component| component.is_alpha() || !(1..=16).contains(&component.precision()))
    {
        return Err(AppError::PdfBuild(
            "JPX components use an unsupported shape (alpha or precision)".into(),
        ));
    }

    let width = shape.width;
    let height = shape.height;
    let subsampled = components
        .iter()
        .any(|component| component.width() != width || component.height() != height);
    let planes: Vec<Vec<u8>> = components
        .iter()
        .map(|component| component_plane(component, width, height))
        .collect();

    let mismatched_buffer = || AppError::PdfBuild("JPX decode produced mismatched buffer".into());
    match planes.as_slice() {
        [gray] => image::GrayImage::from_raw(width, height, gray.clone())
            .map(DynamicImage::ImageLuma8)
            .ok_or_else(mismatched_buffer),
        [first, second, third] => {
            // SYCC only changes the interpretation of subsampled
            // codestreams; full-grid three-component planes keep the
            // historical component→RGB assembly exactly.
            let pixels = if subsampled && matches!(image.color_space(), jpeg2k::ColorSpace::SYCC) {
                sycc_planes_to_rgb(first, second, third)
            } else {
                let mut pixels = Vec::with_capacity(first.len() * 3);
                for (r, (g, b)) in first.iter().zip(second.iter().zip(third)) {
                    pixels.extend_from_slice(&[*r, *g, *b]);
                }
                pixels
            };
            image::RgbImage::from_raw(width, height, pixels)
                .map(DynamicImage::ImageRgb8)
                .ok_or_else(mismatched_buffer)
        }
        [cyan, magenta, yellow, key] if allow_cmyk => {
            // CMYK planes convert through the shared chokepoint; JPX
            // codestreams carry no PDF-level ICC profile, so the calibrated
            // SWOP matrix applies (0 = no ink, same polarity as raw planes).
            let mut plane = Vec::with_capacity(cyan.len() * 4);
            for (c, (m, (y, k))) in cyan
                .iter()
                .zip(magenta.iter().zip(yellow.iter().zip(key)))
            {
                plane.extend_from_slice(&[*c, *m, *y, *k]);
            }
            let pixels = super::cmyk::cmyk_samples_to_rgb(&plane, None);
            image::RgbImage::from_raw(width, height, pixels)
                .map(DynamicImage::ImageRgb8)
                .ok_or_else(mismatched_buffer)
        }
        _ => Err(AppError::PdfBuild(format!(
            "JPX codestream has an unsupported component count: {}",
            components.len()
        ))),
    }
}

/// A component's samples at the full image grid: components at their own
/// (reduced) grid are upsampled bilinearly — the standard chroma
/// reconstruction for subsampled color components.
fn component_plane(component: &jpeg2k::ImageComponent, width: u32, height: u32) -> Vec<u8> {
    let data: Vec<u8> = component.data_u8().collect();
    if component.width() == width && component.height() == height {
        return data;
    }
    let source = image::GrayImage::from_raw(component.width(), component.height(), data)
        .expect("component buffer matches its declared grid");
    image::imageops::resize(&source, width, height, image::imageops::FilterType::Triangle)
        .into_raw()
}

/// Convert full-resolution YCbCr planes (SYCC subsampled codestreams) to
/// packed RGB with the full-range BT.601 inverse — the same coefficient
/// family the JPEG paths use.
fn sycc_planes_to_rgb(luma: &[u8], cb: &[u8], cr: &[u8]) -> Vec<u8> {
    let mut rgb = Vec::with_capacity(luma.len() * 3);
    for ((&y, &cb), &cr) in luma.iter().zip(cb).zip(cr) {
        let luma = f64::from(y);
        let cb = f64::from(cb) - 128.0;
        let cr = f64::from(cr) - 128.0;
        let red = luma + 1.402 * cr;
        let green = luma - 0.344_136 * cb - 0.714_136 * cr;
        let blue = luma + 1.772 * cb;
        let to_byte = |value: f64| value.clamp(0.0, 255.0).round() as u8;
        rgb.extend_from_slice(&[to_byte(red), to_byte(green), to_byte(blue)]);
    }
    rgb
}

/// Decode a JPX-encoded `/SMask`: a single-component codestream at the
/// dictionary's grid. Multi-component or alpha-carrying masks stay
/// unsupported — the PDF spec defines soft masks as luminance-only, and
/// anything else would change the mask's meaning in the rewrite.
pub(super) fn decode_smask_jpx(
    stream: &Stream,
    width: u32,
    height: u32,
) -> Option<image::GrayImage> {
    let shape = jpx_input_shape(stream)?;
    if shape.width != width || shape.height != height {
        return None;
    }
    let image = jpeg2k::Image::from_bytes(&stream.content).ok()?;
    if image.width() != width || image.height() != height {
        return None;
    }
    let [component] = image.components() else {
        return None;
    };
    if component.is_alpha() || !(1..=16).contains(&component.precision()) {
        return None;
    }
    let plane = component_plane(component, width, height);
    image::GrayImage::from_raw(width, height, plane)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{
        jpx_bilevel_reference, jpx_gray_reference, jpx_rgb_reference, JPX_BILEVEL_J2K,
        JPX_BILEVEL_HEIGHT, JPX_BILEVEL_WIDTH, JPX_GRAY_J2K, JPX_PLANE_HEIGHT, JPX_PLANE_WIDTH,
        JPX_RGB_J2K, JPX_RGB_JP2,
    };
    use image::GenericImageView;
    use lopdf::dictionary;

    fn jpx_stream(content: &[u8], width: i64, height: i64) -> Stream {
        Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Image",
                "Width" => width,
                "Height" => height,
                "Filter" => "JPXDecode",
            },
            content.to_vec(),
        )
    }

    fn fixture_dims(bilevel: bool) -> (u32, u32) {
        if bilevel {
            (JPX_BILEVEL_WIDTH, JPX_BILEVEL_HEIGHT)
        } else {
            (JPX_PLANE_WIDTH, JPX_PLANE_HEIGHT)
        }
    }

    fn fixture_stream(content: &[u8], bilevel: bool) -> Stream {
        let (width, height) = fixture_dims(bilevel);
        jpx_stream(content, i64::from(width), i64::from(height))
    }

    #[test]
    fn embedded_codestreams_decode_to_matching_planes() {
        for (name, content, bilevel, channels) in [
            ("rgb-j2k", JPX_RGB_J2K, false, 3usize),
            ("rgb-jp2", JPX_RGB_JP2, false, 3),
            ("gray-j2k", JPX_GRAY_J2K, false, 1),
            ("bilevel-j2k", JPX_BILEVEL_J2K, true, 1),
        ] {
            let stream = fixture_stream(content, bilevel);
            let plane = decode_jpx_stream(&stream, true)
                .unwrap_or_else(|error| panic!("{name}: {error}"));
            assert_eq!(plane.dimensions(), fixture_dims(bilevel), "{name} dimensions");
            let bytes = plane.as_bytes();
            assert_eq!(
                bytes.chunks(channels).count(),
                (fixture_dims(bilevel).0 * fixture_dims(bilevel).1) as usize,
                "{name} pixel count"
            );
        }
    }

    /// The committed fixtures are lossless, so decoded planes must match the
    /// reference patterns pixel for pixel — this is the fidelity anchor for
    /// the whole JPX path (everything downstream re-encodes from these
    /// planes).
    #[test]
    fn lossless_codestreams_round_trip_exactly() {
        let rgb = decode_jpx_stream(&fixture_stream(JPX_RGB_J2K, false), true).expect("rgb j2k");
        assert_eq!(
            rgb.to_rgb8().as_raw(),
            jpx_rgb_reference().as_raw(),
            "RGB codestream must decode to the reference plane"
        );
        let rgb_jp2 = decode_jpx_stream(&fixture_stream(JPX_RGB_JP2, false), true).expect("rgb jp2");
        assert_eq!(
            rgb_jp2.to_rgb8().as_raw(),
            jpx_rgb_reference().as_raw(),
            "JP2 container must yield the same pixels as the raw codestream"
        );
        let gray = decode_jpx_stream(&fixture_stream(JPX_GRAY_J2K, false), true).expect("gray j2k");
        assert_eq!(
            gray.to_luma8().as_raw(),
            jpx_gray_reference().as_raw(),
            "grayscale codestream must decode to the reference plane"
        );
        let bilevel =
            decode_jpx_stream(&fixture_stream(JPX_BILEVEL_J2K, true), true).expect("bilevel");
        assert_eq!(
            bilevel.to_luma8().as_raw(),
            jpx_bilevel_reference().as_raw(),
            "bilevel codestream must decode to the reference plane"
        );
    }

    #[test]
    fn dictionary_dimension_mismatch_is_rejected() {
        let stream = jpx_stream(
            JPX_RGB_J2K,
            i64::from(JPX_PLANE_WIDTH) + 32,
            i64::from(JPX_PLANE_HEIGHT),
        );
        assert!(decode_jpx_stream(&stream, true).is_err());
    }

    #[test]
    fn shape_gate_rejects_hostile_dimensions_and_parms() {
        let width = i64::from(JPX_PLANE_WIDTH);
        let height = i64::from(JPX_PLANE_HEIGHT);
        assert!(jpx_input_shape(&jpx_stream(JPX_RGB_J2K, 0, height)).is_none());
        assert!(jpx_input_shape(&jpx_stream(JPX_RGB_J2K, -1, height)).is_none());
        assert!(jpx_input_shape(&jpx_stream(JPX_RGB_J2K, 65_536, height)).is_none());
        let mut with_parms = jpx_stream(JPX_RGB_J2K, width, height);
        with_parms.dict.set("DecodeParms", dictionary! { "K" => -1 });
        assert!(jpx_input_shape(&with_parms).is_none());
        let mut with_decode = jpx_stream(JPX_RGB_J2K, width, height);
        with_decode.dict.set(
            "Decode",
            vec![0.into(), 1.into(), 0.into(), 1.into(), 0.into(), 1.into()],
        );
        assert!(jpx_input_shape(&with_decode).is_none());
        // The well-formed shape passes.
        assert!(jpx_input_shape(&jpx_stream(JPX_RGB_J2K, width, height)).is_some());
    }

    #[test]
    fn garbage_content_fails_cleanly() {
        let width = i64::from(JPX_PLANE_WIDTH);
        let height = i64::from(JPX_PLANE_HEIGHT);
        let stream = jpx_stream(b"not a codestream at all", width, height);
        assert!(decode_jpx_stream(&stream, true).is_err());
        let empty = jpx_stream(&[], width, height);
        assert!(decode_jpx_stream(&empty, true).is_err());
    }
}
