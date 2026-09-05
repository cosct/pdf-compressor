//! JBIG2 (ITU T.88) image-stream decoding — pure Rust via `hayro-jbig2`.
//! JBIG2（ITU T.88）图像流解码 — 经 hayro-jbig2 的纯 Rust 实现。
//!
//! Scanned text pages compressed with JBIG2 are the last major input codec
//! gap; decoded planes are near-bilevel by construction and flow into the
//! existing G4/JPEG outlets like every other decode path.
//!
//! Gate discipline mirrors the CCITT/JPX shape gates: `jbig2_input_shape`
//! decides decodability from the stream dictionary alone (single-element
//! filter chain, plain dimensions, no `/DecodeParms`, no `/Decode`), and
//! the shared `stream_filter_info` + analyzer mirror pick it up. The
//! document-level context is the `/JBIG2Globals` segment stream: it is
//! resolved once in `prepare_document` (while the document is intact) and
//! carried to the decoder alongside the stream — the same
//! pre-resolution pattern the ICC color spaces use.
//!
//! The decoder feeds a local push-based sink that assembles an 8-bit luma
//! plane directly (black = 0, white = 255 — matching DeviceGray semantics
//! of the rebuilt stream), skipping the intermediate bitmap buffer the
//! `image` integration would allocate.

use image::DynamicImage;
use lopdf::Stream;

use super::optional_integer;
use crate::error::AppError;

/// Decodable JBIG2 input shape, shared by the filter gate and the decoder.
#[derive(Clone, Copy, Debug)]
pub(super) struct Jbig2InputShape {
    width: u32,
    height: u32,
}

/// Validate that a JBIG2 stream uses a shape this engine can decode.
/// Rejected: missing/oversized dimensions (the ceiling matches the JPEG
/// format limit the re-encoder enforces), `/DecodeParms` (not defined for
/// JBIG2Decode in the PDF spec), `/Decode` component mappings, and
/// non-single-element filter chains (the caller checks the chain).
pub(super) fn jbig2_input_shape(stream: &Stream) -> Option<Jbig2InputShape> {
    let width = optional_integer(stream, b"Width")?;
    let height = optional_integer(stream, b"Height")?;
    // Never let hostile dimensions reach the decoded-plane allocation: a
    // negative i64 would wrap to a huge value at the `as u32` cast.
    if !(1..=65_535).contains(&width) || !(1..=65_535).contains(&height) {
        return None;
    }
    if stream.dict.get(b"DecodeParms").is_ok() {
        return None;
    }
    if stream.dict.get(b"Decode").is_ok() {
        return None;
    }
    Some(Jbig2InputShape {
        width: width as u32,
        height: height as u32,
    })
}

/// Push-based decode sink: assembles the decoded bilevel plane as an 8-bit
/// luma image (black = 0), reusing the row-padded allocation pattern of
/// the CCITT expanders.
struct PlaneSink {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
    written: usize,
    row: usize,
}

impl PlaneSink {
    fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            pixels: vec![0u8; width as usize * height as usize],
            written: 0,
            row: 0,
        }
    }

    fn fill(&mut self, black: bool, count: u32) {
        let value = if black { 0 } else { 255 };
        let limit = self.pixels.len();
        let mut remaining = count as usize;
        while remaining > 0 && self.written < limit {
            let row_end = (self.row + 1) * self.width as usize;
            let take = remaining.min(row_end - self.written).min(limit - self.written);
            self.pixels[self.written..self.written + take].fill(value);
            self.written += take;
            remaining -= take;
        }
    }

    fn finish(self) -> Option<DynamicImage> {
        if self.written != self.pixels.len() {
            return None;
        }
        image::GrayImage::from_raw(self.width, self.height, self.pixels)
            .map(DynamicImage::ImageLuma8)
    }
}

impl hayro_jbig2::Decoder for PlaneSink {
    fn push_pixel(&mut self, black: bool) {
        self.fill(black, 1);
    }

    fn push_pixel_chunk(&mut self, black: bool, chunk_count: u32) {
        self.fill(black, chunk_count * 8);
    }

    fn next_line(&mut self) {
        // Rows are already contiguous in the linear buffer; the sink only
        // tracks the row boundary for the clamping in `fill`.
        self.row += 1;
    }
}

/// Decode a JBIG2 image stream (embedded organization, Annex D.3) into an
/// 8-bit grayscale plane. `globals` carries the `/JBIG2Globals` segment
/// bytes when the stream references one; shared symbol dictionaries live
/// there, which is why the document resolved it up front.
pub(super) fn decode_jbig2_stream(
    stream: &Stream,
    globals: Option<&[u8]>,
) -> Result<DynamicImage, AppError> {
    let shape = jbig2_input_shape(stream).ok_or_else(|| {
        AppError::PdfBuild("JBIG2 stream does not use a supported decode shape".into())
    })?;
    let image = hayro_jbig2::Image::new_embedded(&stream.content, globals).map_err(|error| {
        AppError::PdfBuild(format!("failed to decode JBIG2 image stream: {error:?}"))
    })?;
    if image.width() != shape.width || image.height() != shape.height {
        return Err(AppError::PdfBuild(
            "JBIG2 segment dimensions do not match the stream dictionary".into(),
        ));
    }

    let mut sink = PlaneSink::new(shape.width, shape.height);
    image
        .decode(&mut sink)
        .map_err(|error| AppError::PdfBuild(format!("failed to decode JBIG2 pixels: {error:?}")))?;
    sink.finish()
        .ok_or_else(|| AppError::PdfBuild("JBIG2 decode produced mismatched buffer".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::JBIG2_SCAN;
    use lopdf::dictionary;

    fn jbig2_stream(content: &[u8], width: i64, height: i64) -> Stream {
        Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Image",
                "Width" => width,
                "Height" => height,
                "BitsPerComponent" => 1,
                "Filter" => "JBIG2Decode",
            },
            content.to_vec(),
        )
    }

    #[test]
    fn standalone_form_also_parses() {
        // Diagnostic companion: the committed asset is derived by stripping
        // the 8-byte standalone file header; this pins that the source file
        // itself parses through `Image::new`, isolating embedded-form
        // issues from codestream issues.
        let data = include_bytes!("../../assets/jbig2-scan-file.jbig2");
        let image = hayro_jbig2::Image::new(data)
            .unwrap_or_else(|error| panic!("standalone form must parse: {error:?}"));
        let (width, height) = crate::testutil::jbig2_scan_dimensions();
        assert_eq!((image.width(), image.height()), (width, height));
    }

    #[test]
    fn committed_scan_decodes_to_a_bilevel_plane() {
        // The committed fixture is a symbol-dictionary text page from the
        // hayro conformance assets (see scripts/make-jbig2-fixture.sh). The
        // pixel ground truth has no independent encoder to derive from, so
        // this asserts the structural contract (dimensions, bilevel, both
        // poles present); the end-to-end fidelity anchor is the poppler
    // render comparison in the integration tests.
        let (width, height) = crate::testutil::jbig2_scan_dimensions();
        let stream = jbig2_stream(JBIG2_SCAN, i64::from(width), i64::from(height));
        let plane = decode_jbig2_stream(&stream, None)
            .unwrap_or_else(|error| panic!("committed scan must decode: {error}"));
        let decoded = plane.to_luma8();
        assert_eq!(decoded.dimensions(), (width, height));
        let raw = decoded.as_raw();
        assert!(
            raw.contains(&0) && raw.contains(&255),
            "a text scan must contain both black and white pixels"
        );
    }

    #[test]
    fn shape_gate_rejects_hostile_shapes() {
        let (width, height) = crate::testutil::jbig2_scan_dimensions();
        assert!(jbig2_input_shape(&jbig2_stream(JBIG2_SCAN, 0, i64::from(height))).is_none());
        assert!(jbig2_input_shape(&jbig2_stream(JBIG2_SCAN, 65_536, i64::from(height))).is_none());
        let mut with_parms = jbig2_stream(JBIG2_SCAN, i64::from(width), i64::from(height));
        with_parms.dict.set("DecodeParms", dictionary! { "K" => -1 });
        assert!(jbig2_input_shape(&with_parms).is_none());
        let mut with_decode = jbig2_stream(JBIG2_SCAN, i64::from(width), i64::from(height));
        with_decode.dict.set("Decode", vec![0.into(), 1.into()]);
        assert!(jbig2_input_shape(&with_decode).is_none());
        // The well-formed shape passes.
        assert!(
            jbig2_input_shape(&jbig2_stream(JBIG2_SCAN, i64::from(width), i64::from(height)))
                .is_some()
        );
    }

    #[test]
    fn garbage_content_fails_cleanly() {
        let (width, height) = crate::testutil::jbig2_scan_dimensions();
        let stream = jbig2_stream(b"garbage segments", i64::from(width), i64::from(height));
        assert!(decode_jbig2_stream(&stream, None).is_err());
    }

    #[test]
    fn dimension_mismatch_is_rejected() {
        let (width, height) = crate::testutil::jbig2_scan_dimensions();
        let stream = jbig2_stream(JBIG2_SCAN, i64::from(width) + 7, i64::from(height));
        assert!(decode_jbig2_stream(&stream, None).is_err());
    }
}
