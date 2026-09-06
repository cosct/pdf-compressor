//! CMYK → RGB conversion calibrated against mainstream PDF renderers.
//! CMYK → RGB 转换 —— 与主流渲染器对 CMYK 的解释对齐（0.7.0 路线 P0）。
//!
//! [`cmyk_samples_to_rgb`] is the single chokepoint every CMYK path funnels
//! through: raw planes, CMYK-based Indexed palettes, DCT streams, and
//! 4-component JPX codestreams. Samples are packed interleaved bytes with
//! 0 = no ink; the result is packed RGB.
//!
//! - With the `cmyk-cms` feature, images carrying an ICC profile (ICC N=4)
//!   convert through Little CMS with the exact transform poppler builds for
//!   the same file — embedded profile → built-in sRGB, relative-colorimetric
//!   intent, black-point compensation — so the re-encoded RGB matches a
//!   color-managed renderer's interpretation instead of drifting ≈7.6 dB off
//!   it (measured on Adobe YCK fixtures, 2026-09). Profile-less CMYK
//!   (`/DeviceCMYK` names, CMYK-based Indexed, JPX) converts through the
//!   16-term SWOP approximation matrix that poppler and PDFium both use for
//!   device CMYK; an embedded profile that Little CMS cannot parse falls
//!   back to the same matrix, mirroring poppler's unusable-profile path.
//! - Without the feature the naive ink-subtraction formula stays (0.6.0
//!   behavior), still guarded by the opt-in `cmyk_conversion` setting.
//!
//! [`decode_dct_cmyk_stream`] 补上 DCT 路径的最后一块：zune-jpeg 默认把
//! Adobe CMYK/YCCK 在解码器内部折成 RGB（朴素公式），feature 开启时改为
//! 取出原始 4 分量平面再走上面的校准转换。

// ---------------------------------------------------------------------------
// Naive ink-subtraction (feature-off path, unchanged since 0.6.0)
// ---------------------------------------------------------------------------

/// Convert one 8-bit CMYK sample to RGB with the ink-subtraction formula
/// `rgb = (1 - cmy) × (1 - k)` — the device-level conversion applied when no
/// color management is available. PDF raw and JPX CMYK samples are
/// non-inverted (0 = no ink), unlike the Adobe-convention inverted CMYK
/// found inside some DCT streams, which the JPEG path resolves before the
/// plane gets here.
///
/// Compiled out of non-test `cmyk-cms` builds, where the calibrated paths
/// fully replace it (tests keep it around as the naive baseline to diff
/// against).
#[cfg(any(not(feature = "cmyk-cms"), test))]
pub(crate) fn cmyk_to_rgb(cyan: u8, magenta: u8, yellow: u8, key: u8) -> [u8; 3] {
    let white = 255u16 - u16::from(key);
    let subtract = |channel: u8| -> u8 {
        (((255u16 - u16::from(channel)) * white + 127) / 255) as u8
    };
    [subtract(cyan), subtract(magenta), subtract(yellow)]
}

/// Convert packed interleaved CMYK samples (0 = no ink) to packed RGB.
/// A trailing partial sample is ignored; callers pass exact planes.
pub(crate) fn cmyk_samples_to_rgb(samples: &[u8], icc_profile: Option<&[u8]>) -> Vec<u8> {
    #[cfg(feature = "cmyk-cms")]
    {
        if let Some(profile) = icc_profile {
            if let Some(rgb) = lcms_cmyk_to_rgb(samples, profile) {
                return rgb;
            }
        }
        calibrated::samples_to_rgb(samples)
    }
    #[cfg(not(feature = "cmyk-cms"))]
    {
        let _ = icc_profile;
        let mut rgb = Vec::with_capacity(samples.len() / 4 * 3);
        for sample in samples.as_chunks::<4>().0 {
            let [red, green, blue] = cmyk_to_rgb(sample[0], sample[1], sample[2], sample[3]);
            rgb.extend_from_slice(&[red, green, blue]);
        }
        rgb
    }
}

// ---------------------------------------------------------------------------
// Calibrated conversion (`cmyk-cms`)
// ---------------------------------------------------------------------------

#[cfg(feature = "cmyk-cms")]
mod calibrated {
    /// Naive ink-subtraction as the documented baseline the calibrated paths
    /// replaced — kept for the fidelity tests to diff against.
    #[cfg(test)]
    use super::cmyk_to_rgb;

    /// Convert one 8-bit CMYK sample (0 = no ink) to RGB with the 16-term
    /// SWOP approximation matrix from xpdf heritage — bit-for-bit the
    /// conversion `GfxDeviceCMYKColorSpace::getRGBLine` applies in poppler
    /// (and the same matrix PDFium carries), including its
    /// truncate-after-clip output rounding. This is the calibration target
    /// for profile-less CMYK: renderers without a configured default CMYK
    /// profile (pdftoppm, out of the box) interpret device CMYK this way.
    fn calibrated_cmyk_to_rgb(cyan: u8, magenta: u8, yellow: u8, key: u8) -> [u8; 3] {
        let c = f64::from(cyan) / 255.0;
        let m = f64::from(magenta) / 255.0;
        let y = f64::from(yellow) / 255.0;
        let k = f64::from(key) / 255.0;
        let (c1, m1, y1, k1) = (1.0 - c, 1.0 - m, 1.0 - y, 1.0 - k);
        let mut r = c1 * m1 * y1 * k1;
        let mut g = r;
        let mut b = r;
        // The 16 corner terms of a SWOP-fitted CMYK→RGB matrix, unrolled
        // exactly as poppler's `cmykToRGBMatrixMultiplication`.
        let mut term = |cc: f64, mm: f64, yy: f64, kk: f64, dr: f64, dg: f64, db: f64| {
            let x = cc * mm * yy * kk;
            r += dr * x;
            g += dg * x;
            b += db * x;
        };
        term(c1, m1, y1, k, 0.1373, 0.1216, 0.1255);
        term(c1, m1, y, k1, 1.0, 0.9490, 0.0);
        term(c1, m1, y, k, 0.1098, 0.1020, 0.0);
        term(c1, m, y1, k1, 0.9255, 0.0, 0.5490);
        term(c1, m, y1, k, 0.1412, 0.0, 0.0);
        term(c1, m, y, k1, 0.9294, 0.1098, 0.1412);
        term(c1, m, y, k, 0.1333, 0.0, 0.0);
        term(c, m1, y1, k1, 0.0, 0.6784, 0.9373);
        term(c, m1, y1, k, 0.0, 0.0588, 0.1412);
        term(c, m1, y, k1, 0.0, 0.6510, 0.3137);
        term(c, m1, y, k, 0.0, 0.0745, 0.0);
        term(c, m, y1, k1, 0.1804, 0.1922, 0.5725);
        term(c, m, y1, k, 0.0, 0.0, 0.0078);
        term(c, m, y, k1, 0.2118, 0.2119, 0.2235);
        // poppler's dblToByte truncates after clipping — keep that exact
        // rounding so the matrix matches the renderer sample-for-sample.
        let to_byte = |value: f64| -> u8 { (value.clamp(0.0, 1.0) * 255.0) as u8 };
        [to_byte(r), to_byte(g), to_byte(b)]
    }

    pub(super) fn samples_to_rgb(samples: &[u8]) -> Vec<u8> {
        let mut rgb = Vec::with_capacity(samples.len() / 4 * 3);
        for sample in samples.as_chunks::<4>().0 {
            let [red, green, blue] =
                calibrated_cmyk_to_rgb(sample[0], sample[1], sample[2], sample[3]);
            rgb.extend_from_slice(&[red, green, blue]);
        }
        rgb
    }

    /// Naive ink-subtraction as the documented baseline the calibrated paths
    /// replaced — kept for the fidelity tests to diff against.
    #[cfg(test)]
    pub(super) fn naive_samples_to_rgb(samples: &[u8]) -> Vec<u8> {
        let mut rgb = Vec::with_capacity(samples.len() / 4 * 3);
        for sample in samples.as_chunks::<4>().0 {
            let [red, green, blue] = cmyk_to_rgb(sample[0], sample[1], sample[2], sample[3]);
            rgb.extend_from_slice(&[red, green, blue]);
        }
        rgb
    }
}

#[cfg(feature = "cmyk-cms")]
fn lcms_cmyk_to_rgb(samples: &[u8], profile_bytes: &[u8]) -> Option<Vec<u8>> {
    use std::cell::RefCell;
    use std::collections::HashMap;

    use lcms2::Transform;

    let pixels = samples.len() / 4;
    if pixels == 0 {
        return None;
    }

    // Transforms are relatively expensive to build (LCMS compiles a LUT);
    // workers convert many images per thread, so keep one transform per
    // distinct profile per thread. `Transform` is neither `Send` nor `Sync`
    // (it carries an LCMS pixel cache), which thread-local storage sidesteps.
    thread_local! {
        static TRANSFORMS: RefCell<HashMap<Vec<u8>, Transform<u8, u8>>> =
            RefCell::new(HashMap::new());
    }

    let mut rgb = vec![0u8; pixels * 3];
    let transform = TRANSFORMS.with(|cache| {
        let mut cache = cache.borrow_mut();
        if !cache.contains_key(profile_bytes) {
            let built = build_transform(profile_bytes)?;
            cache.insert(profile_bytes.to_vec(), built);
        }
        // Safe to unwrap: the key was just inserted above on this thread.
        let transform = cache.get_mut(profile_bytes).expect("profile just inserted");
        transform.transform_pixels(&samples[..pixels * 4], &mut rgb);
        Some(())
    });
    transform?;
    Some(rgb)
}

#[cfg(feature = "cmyk-cms")]
fn build_transform(profile_bytes: &[u8]) -> Option<lcms2::Transform<u8, u8>> {
    use lcms2::{Flags, Intent, PixelFormat, Profile, Transform};

    let profile = Profile::new_icc(profile_bytes).ok()?;
    let srgb = Profile::new_srgb();
    // The exact transform poppler's GfxICCBasedColorSpace builds for an
    // embedded 4-channel profile with no display profile set (pdftoppm's
    // defaults): relative colorimetric with black-point compensation into
    // LCMS's built-in sRGB. Matching it is what makes the re-encoded JPEG
    // render like the original under poppler/PDFium.
    Transform::<u8, u8>::new_flags(
        &profile,
        PixelFormat::CMYK_8,
        &srgb,
        PixelFormat::RGB_8,
        Intent::RelativeColorimetric,
        Flags::BLACKPOINT_COMPENSATION,
    )
    .ok()
}

// ---------------------------------------------------------------------------
// DCT (JPEG) CMYK planes (`cmyk-cms`)
// ---------------------------------------------------------------------------

/// Decode a 4-component JPEG (Adobe CMYK or YCCK) into an RGB image through
/// the calibrated conversion. Returns `None` whenever the stream is not a
/// decodable 4-component JPEG so the caller falls back to the image crate's
/// generic decode.
///
/// Sample polarity follows the renderers this engine calibrates against
/// (poppler via libjpeg, PDFium): a 4-component DCT stream's decoded output
/// is consumed as plain CMYK where 0 = no ink — transform-0 samples pass
/// through unmodified, and YCCK is undone as `cmy = 255 − YCbCr⁻¹(stored)`
/// with the K channel passed through as stored (libjpeg's
/// `ycck_cmyk_convert` semantics). The historical "Adobe inverted CMYK"
/// reading is NOT applied by these renderers (verified against poppler
/// 26.08 with raw-plane fixtures, 2026-09).
#[cfg(feature = "cmyk-cms")]
pub(super) fn decode_dct_cmyk_stream(
    content: &[u8],
    icc_profile: Option<&[u8]>,
    cmyk_decodes: Option<&[super::colorspace::ChannelDecode]>,
) -> Option<image::DynamicImage> {
    use image::DynamicImage;
    use zune_jpeg::zune_core::bytestream::ZCursor;
    use zune_jpeg::zune_core::colorspace::ColorSpace;
    use zune_jpeg::zune_core::options::DecoderOptions;
    use zune_jpeg::JpegDecoder;

    let mut decoder = JpegDecoder::new(ZCursor::new(content));
    decoder.decode_headers().ok()?;
    let input = decoder.input_colorspace()?;
    if !matches!(input, ColorSpace::CMYK | ColorSpace::YCCK) {
        return None;
    }
    let info = decoder.info()?;

    // Requesting the input colorspace as the output makes zune skip its
    // built-in CMYK→RGB fold (the naive formula this module replaces) and
    // hand back the raw 4-component plane.
    decoder.set_options(DecoderOptions::default().jpeg_set_out_colorspace(input));
    let mut samples = decoder.decode().ok()?;

    if input == ColorSpace::YCCK {
        ycck_to_cmyk(&mut samples);
    }

    // A four-channel /Decode remaps the CMYK components before any
    // conversion — the inversion does not commute with the CMS transform,
    // so it applies here, on the raw samples.
    if let Some(decodes) = cmyk_decodes {
        for pixel in samples.as_chunks_mut::<4>().0 {
            for (channel, decode) in pixel.iter_mut().zip(decodes) {
                if matches!(decode, super::colorspace::ChannelDecode::Invert) {
                    *channel = 255 - *channel;
                }
            }
        }
    }

    let rgb = cmyk_samples_to_rgb(&samples, icc_profile);
    let image = image::RgbImage::from_raw(u32::from(info.width), u32::from(info.height), rgb)?;
    Some(DynamicImage::ImageRgb8(image))
}

/// Undo the YCCK encoding: the ITU-R BT.601 YCbCr inverse over the first
/// three channels yields the inverted CMY triple, and the K channel passes
/// through as stored (see the polarity note on
/// [`decode_dct_cmyk_stream`]).
#[cfg(feature = "cmyk-cms")]
fn ycck_to_cmyk(samples: &mut [u8]) {
    for pixel in samples.as_chunks_mut::<4>().0 {
        let (y, cb, cr) = (f64::from(pixel[0]), f64::from(pixel[1]), f64::from(pixel[2]));
        let red = y + 1.402 * (cr - 128.0);
        let green = y - 0.344_136 * (cb - 128.0) - 0.714_136 * (cr - 128.0);
        let blue = y + 1.772 * (cb - 128.0);
        let clamp = |value: f64| value.clamp(0.0, 255.0).round() as u8;
        // The YCbCr inverse lands on inverted CMY; flip to 0 = no ink.
        pixel[0] = 255 - clamp(red);
        pixel[1] = 255 - clamp(green);
        pixel[2] = 255 - clamp(blue);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn naive_cmyk_conversion_corner_cases() {
        // No ink anywhere → paper white.
        assert_eq!(cmyk_to_rgb(0, 0, 0, 0), [255, 255, 255]);
        // Full key alone → rich black, all channels pulled to zero.
        assert_eq!(cmyk_to_rgb(0, 0, 0, 255), [0, 0, 0]);
        // Pure ink primaries survive the key-free path exactly: full cyan
        // ink absorbs all red light, and symmetrically for the others.
        assert_eq!(cmyk_to_rgb(255, 0, 0, 0), [0, 255, 255]);
        assert_eq!(cmyk_to_rgb(0, 255, 0, 0), [255, 0, 255]);
        assert_eq!(cmyk_to_rgb(0, 0, 255, 0), [255, 255, 0]);
        // Full ink everywhere → black.
        assert_eq!(cmyk_to_rgb(255, 255, 255, 255), [0, 0, 0]);
        // Half cyan, no key: 255 - 255/2 = 127.5 → 128 (rounded).
        assert_eq!(cmyk_to_rgb(128, 0, 0, 0)[0], 127);
        // 50% key darkens every channel to ~127.
        assert_eq!(cmyk_to_rgb(0, 0, 0, 128), [127, 127, 127]);
    }

    #[test]
    fn naive_cmyk_conversion_is_monotonic_per_channel() {
        for channel in 0..3 {
            let mut sample = [0u8, 0, 0, 0];
            let mut last = 255;
            for ink in 0..=255u8 {
                sample[channel] = ink;
                let value = cmyk_to_rgb(sample[0], sample[1], sample[2], sample[3])[channel];
                assert!(value <= last, "more ink must not brighten the channel");
                last = value;
            }
        }
    }

    /// With `cmyk-cms` the profile-less chokepoint is the SWOP matrix, not
    /// this formula — the equality only holds in feature-off builds.
    #[cfg(not(feature = "cmyk-cms"))]
    #[test]
    fn buffer_conversion_matches_per_sample_formula() {
        let samples: Vec<u8> = (0..64u8).collect();
        let rgb = cmyk_samples_to_rgb(&samples, None);
        assert_eq!(rgb.len(), 48);
        for (sample, pixel) in samples.as_chunks::<4>().0.iter().zip(rgb.as_chunks::<3>().0) {
            assert_eq!(
                pixel,
                &cmyk_to_rgb(sample[0], sample[1], sample[2], sample[3])
            );
        }
    }

    #[cfg(feature = "cmyk-cms")]
    mod calibrated_tests {
        use super::*;

        #[test]
        fn calibrated_corners_stay_sane() {
            // Paper white must survive the SWOP matrix; full ink lands on
            // the matrix's rich black (real ink is not a perfect absorber).
            assert_eq!(calibrated::samples_to_rgb(&[0, 0, 0, 0]), [255, 255, 255]);
            assert_eq!(calibrated::samples_to_rgb(&[255, 255, 255, 255]), [0, 0, 0]);
            // A saturated yellow (no key) stays a yellow hue, not neutral.
            let yellow = calibrated::samples_to_rgb(&[0, 0, 255, 0]);
            assert_eq!(yellow, [255, 241, 0]);
        }

        #[test]
        fn calibrated_conversion_is_monotonic_per_channel() {
            for channel in 0..3 {
                let mut sample = [0u8, 0, 0, 0];
                let mut last = 255;
                for ink in 0..=255u8 {
                    sample[channel] = ink;
                    let value = calibrated::samples_to_rgb(&sample)[channel];
                    assert!(value <= last, "more ink must not brighten the channel");
                    last = value;
                }
            }
        }

        #[test]
        fn calibrated_matrix_reproduces_poppler_reference_values() {
            // Reference values computed from poppler 26.08's
            // GfxDeviceCMYKColorSpace::getRGBLine (byte → /255.0, SWOP
            // matrix, clip, ×255 truncated) for a mid-tone spread; pinning
            // them guards the exact arithmetic against accidental "nicer"
            // rounding.
            let cases: &[(u8, u8, u8, u8, [u8; 3])] = &[
                (0, 0, 0, 0, [255, 255, 255]),
                (255, 255, 255, 255, [0, 0, 0]),
                (0, 0, 0, 255, [35, 31, 32]),
                (128, 0, 0, 0, [127, 213, 246]),
                (0, 128, 0, 0, [245, 127, 197]),
                (0, 0, 128, 0, [255, 248, 127]),
                (64, 96, 32, 16, [179, 142, 176]),
                (200, 40, 160, 90, [41, 111, 83]),
                (255, 0, 0, 0, [0, 172, 239]),
                (0, 255, 0, 0, [236, 0, 139]),
            ];
            for &(c, m, y, k, expected) in cases {
                assert_eq!(
                    calibrated::samples_to_rgb(&[c, m, y, k]),
                    expected,
                    "CMYK({c},{m},{y},{k})"
                );
            }
        }

        #[test]
        fn lcms_embedded_profile_conversion_is_sane_and_distinct() {
            // The CGATS TR 001 CMYK profile (CC0, Compact-ICC-Profiles) —
            // the same bytes the render-fidelity fixtures embed.
            let profile: &[u8] = include_bytes!("../../assets/cgats001-cmyk.icc");

            // Paper white must stay paper white under the profile.
            let white = cmyk_samples_to_rgb(&[0, 0, 0, 0], Some(profile));
            assert!(white.iter().all(|&v| v >= 250), "white drifted: {white:?}");

            // Full ink must land near black (perceptual table, small slack).
            let black = cmyk_samples_to_rgb(&[255, 255, 255, 255], Some(profile));
            assert!(black.iter().all(|&v| v <= 12), "black drifted: {black:?}");

            // The CMS output must measurably differ from both the naive and
            // the matrix conversions — otherwise the profile path is not
            // actually engaged.
            let samples: Vec<u8> = (0..=255u8)
                .flat_map(|i| [i, 255 - i, (i / 2), (i / 3)])
                .collect();
            let cms = cmyk_samples_to_rgb(&samples, Some(profile));
            let naive = calibrated::naive_samples_to_rgb(&samples);
            let matrix = calibrated::samples_to_rgb(&samples);
            let max_delta = |a: &[u8], b: &[u8]| {
                a.iter().zip(b).map(|(x, y)| x.abs_diff(*y)).max().unwrap()
            };
            assert!(max_delta(&cms, &naive) > 8, "CMS output equals naive formula");
            assert!(max_delta(&cms, &matrix) > 2, "CMS output equals the bare matrix");

            // Deterministic across calls (transform cache hit path).
            assert_eq!(cms, cmyk_samples_to_rgb(&samples, Some(profile)));
        }
    }
}
