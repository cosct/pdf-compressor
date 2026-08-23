//! Pipeline integration tests — build real PDFs with lopdf and run them
//! through the analyze/compress pipeline end to end.
//! 管线集成测试 — 用 lopdf 构造真实 PDF，端到端运行分析/压缩管线。

use std::{
    fs,
    io::Cursor,
    path::{Path, PathBuf},
    sync::{atomic::AtomicBool, Arc},
};

use image::{codecs::jpeg::JpegEncoder, DynamicImage, Rgb, RgbImage};
use lopdf::{dictionary, Document, Object, Stream};

use super::analyzer::analyze_pdf_with_progress;
use super::compressor::compress_pdf_with_progress;
use super::settings::{CompressionSettings, CompressionSettingsOverrides};
use super::target_size::compress_pdf_to_target_size;

const FIXTURE_TEXT: &str = "Pipeline integration fixture";

// ---------------------------------------------------------------------------
// Fixture construction
// ---------------------------------------------------------------------------

/// Gradient plus deterministic noise: behaves like a photograph under JPEG
/// (poorly compressible at high quality) while staying reproducible.
fn deterministic_rgb_image(width: u32, height: u32) -> RgbImage {
    let mut image = RgbImage::new(width, height);
    let mut state: u32 = 0x1234_5678;

    for y in 0..height {
        for x in 0..width {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let noise = ((state >> 24) & 0xFF) as i32 - 128;
            let channel = |value: u32, span: u32| {
                (value.saturating_mul(255) / span) as i32
            };
            let red = channel(x, width.max(1));
            let green = channel(y, height.max(1));
            let blue = channel(x + y, width.saturating_add(height).max(1));
            let components = [
                (red + noise).clamp(0, 255) as u8,
                (green + noise).clamp(0, 255) as u8,
                (blue + noise).clamp(0, 255) as u8,
            ];
            image.put_pixel(x, y, Rgb(components));
        }
    }

    image
}

fn encode_jpeg(image: RgbImage, quality: u8) -> Vec<u8> {
    let dynamic = DynamicImage::ImageRgb8(image);
    let mut cursor = Cursor::new(Vec::new());
    JpegEncoder::new_with_quality(&mut cursor, quality)
        .encode_image(&dynamic)
        .expect("failed to encode fixture JPEG");
    cursor.into_inner()
}

/// Build a one-page PDF containing visible text, one embedded JPEG image
/// (optionally with a flate-compressed grayscale soft mask), and optionally
/// byte-identical duplicate copies of that image.
fn build_pdf_bytes_ext(
    jpeg: Vec<u8>,
    width: u32,
    height: u32,
    smask_gray: Option<Vec<u8>>,
    duplicates: usize,
) -> Vec<u8> {
    let mut doc = Document::with_version("1.5");
    let info_id = doc.add_object(dictionary! {
        "Producer" => Object::string_literal("pdf-compressor test fixture"),
    });
    let pages_id = doc.new_object_id();
    let font_id = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
    });

    let smask_ref = smask_gray.map(|gray| {
        let mut soft_mask = Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Image",
                "Width" => width as i64,
                "Height" => height as i64,
                "ColorSpace" => "DeviceGray",
                "BitsPerComponent" => 8,
            },
            gray,
        );
        let _ = soft_mask.compress();
        Object::Reference(doc.add_object(soft_mask))
    });

    let mut xobjects = lopdf::Dictionary::new();
    let mut draw_ops = String::new();
    for copy in 0..=duplicates {
        let mut image_dict = dictionary! {
            "Type" => "XObject",
            "Subtype" => "Image",
            "Width" => width as i64,
            "Height" => height as i64,
            "ColorSpace" => "DeviceRGB",
            "BitsPerComponent" => 8,
            "Filter" => "DCTDecode",
        };
        if let Some(reference) = &smask_ref {
            image_dict.set("SMask", reference.clone());
        }
        let image_id = doc.add_object(Stream::new(image_dict, jpeg.clone()));
        let name = format!("Im{copy}");
        xobjects.set(name.as_bytes().to_vec(), Object::Reference(image_id));
        draw_ops.push_str(&format!("q 400 0 0 300 72 400 cm /{name} Do Q\n"));
    }

    let resources_id = doc.add_object(dictionary! {
        "Font" => dictionary! {
            "F1" => font_id,
        },
        "XObject" => xobjects,
    });
    let content = format!("{draw_ops}BT /F1 24 Tf 72 720 Td ({FIXTURE_TEXT}) Tj ET\n");
    let content_id = doc.add_object(Stream::new(dictionary! {}, content.into_bytes()));
    let page_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "Contents" => content_id,
        "Resources" => resources_id,
        "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
    });
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![Object::Reference(page_id)],
            "Count" => 1,
        }),
    );
    let catalog_id = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });
    doc.trailer.set("Root", catalog_id);
    doc.trailer.set("Info", info_id);

    let mut bytes = Vec::new();
    doc.save_modern(&mut bytes).expect("failed to save fixture PDF");
    bytes
}

fn build_pdf_bytes(jpeg: Vec<u8>, width: u32, height: u32) -> Vec<u8> {
    build_pdf_bytes_ext(jpeg, width, height, None, 0)
}

fn write_fixture(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, bytes).expect("failed to write fixture");
    path
}

fn fresh_fixture(dir: &Path) -> PathBuf {
    let jpeg = encode_jpeg(deterministic_rgb_image(1600, 1200), 95);
    write_fixture(dir, "fixture.pdf", &build_pdf_bytes(jpeg, 1600, 1200))
}

fn maximum_settings() -> CompressionSettings {
    CompressionSettings::from_sources(
        None,
        CompressionSettingsOverrides {
            preset: Some("maximum".to_string()),
            ..Default::default()
        },
    )
}

fn noop_cancel_flag() -> Arc<AtomicBool> {
    Arc::new(AtomicBool::new(false))
}

fn extracted_text(path: &Path) -> String {
    Document::load(path)
        .and_then(|doc| doc.extract_text(&[1]))
        .expect("failed to extract text")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn analyze_reports_expected_signals() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = fresh_fixture(dir.path());

    let response = analyze_pdf_with_progress(path.to_str().unwrap(), |_| {})
        .expect("analysis must succeed");

    assert_eq!(response.page_count, 1);
    assert_eq!(response.image_object_count, 1);
    assert!(response.file_size_bytes > 10_000.0, "fixture should be sizable");
    assert!(response.max_image_edge_px >= 1200);
}

#[test]
fn compress_round_trip_preserves_text_and_shrinks() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = fresh_fixture(dir.path());
    let original_bytes = fs::read(&path).expect("read original");
    let original_text = extracted_text(&path);

    let response = compress_pdf_with_progress(
        path.to_str().unwrap(),
        maximum_settings(),
        noop_cancel_flag(),
        |_| {},
    )
    .expect("compression must succeed");

    let output = PathBuf::from(&response.output_path);
    assert!(output.exists(), "output file must exist");
    assert!(
        fs::metadata(&output).expect("output metadata").len() * 2 < original_bytes.len() as u64,
        "expected the noisy high-quality fixture to shrink by more than half"
    );
    assert!(response.images_recompressed >= 1);
    assert!(response.metadata_removed, "metadata strip is on by default");

    // The output must be a loadable PDF with the same page and text content.
    let reloaded = Document::load(&output).expect("output must be a valid PDF");
    assert_eq!(reloaded.get_pages().len(), 1);
    assert_eq!(original_text.trim(), extracted_text(&output).trim());

    // The original must be left untouched.
    assert_eq!(fs::read(&path).expect("re-read original"), original_bytes);
}

#[test]
fn compress_survives_broken_image_stream() {
    // Regression test: one corrupt JPEG inside a PDF must degrade to a skip,
    // never fail the whole file.
    let mut jpeg = encode_jpeg(deterministic_rgb_image(1600, 1200), 95);
    for byte in jpeg.iter_mut().skip(64) {
        *byte = byte.wrapping_add(0x5A);
    }

    let dir = tempfile::tempdir().expect("tempdir");
    let path = write_fixture(dir.path(), "broken-image.pdf", &build_pdf_bytes(jpeg, 1600, 1200));

    let response = compress_pdf_with_progress(
        path.to_str().unwrap(),
        maximum_settings(),
        noop_cancel_flag(),
        |_| {},
    )
    .expect("a broken image stream must not fail the whole compression");

    assert!(
        response.images_skipped + response.images_recompressed >= 1,
        "the image must be either skipped or recompressed"
    );
    assert!(PathBuf::from(&response.output_path).exists());
}

#[test]
fn mutated_corpus_never_panics() {
    // Deterministic byte mutations of a valid fixture run through load +
    // compression. Ok and Err are both acceptable; panicking is not.
    let bytes = build_pdf_bytes(
        encode_jpeg(deterministic_rgb_image(800, 600), 90),
        800,
        600,
    );
    let dir = tempfile::tempdir().expect("tempdir");
    let mut rng: u32 = 0xC0FF_EE01;

    for case in 0..96u32 {
        let mut mutant = bytes.clone();
        let mutations = 1 + (rng % 8);
        for _ in 0..mutations {
            rng = rng.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let index = (rng as usize) % mutant.len();
            mutant[index] = (rng >> 24) as u8;
        }

        let path = write_fixture(dir.path(), &format!("mutant-{case}.pdf"), &mutant);

        if let Ok(doc) = Document::load(&path) {
            let _ = doc.get_pages();
        }

        let _ = compress_pdf_with_progress(
            path.to_str().unwrap(),
            maximum_settings(),
            noop_cancel_flag(),
            |_| {},
        );
    }
}

// ---------------------------------------------------------------------------
// Image deduplication
// ---------------------------------------------------------------------------

#[test]
fn compress_dedupes_identical_images() {
    let jpeg = encode_jpeg(deterministic_rgb_image(1600, 1200), 95);
    let bytes = build_pdf_bytes_ext(jpeg, 1600, 1200, None, 3);
    let original_text = {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = write_fixture(dir.path(), "dedupe-source.pdf", &bytes);
        extracted_text(&path)
    };

    let dir = tempfile::tempdir().expect("tempdir");
    let path = write_fixture(dir.path(), "dedupe.pdf", &bytes);

    let response = compress_pdf_with_progress(
        path.to_str().unwrap(),
        maximum_settings(),
        noop_cancel_flag(),
        |_| {},
    )
    .expect("compression must succeed");

    assert_eq!(response.images_deduplicated, 3);
    assert!(response.notices.iter().any(|n| n.code == "compress.note.imageDedupe"));

    let output = PathBuf::from(&response.output_path);
    let reloaded = Document::load(&output).expect("output must be a valid PDF");
    assert_eq!(reloaded.get_pages().len(), 1);
    assert_eq!(original_text.trim(), extracted_text(&output).trim());
}

// ---------------------------------------------------------------------------
// Soft-mask (transparency) recompression
// ---------------------------------------------------------------------------

#[test]
fn compress_rewrites_smask_alpha() {
    let (width, height) = (1600u32, 1200u32);
    let jpeg = encode_jpeg(deterministic_rgb_image(width, height), 95);

    // Deterministic noisy alpha plane.
    let mut state: u32 = 0x0BAD_F00D;
    let smask_gray: Vec<u8> = (0..(width as usize * height as usize))
        .map(|_| {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (state >> 24) as u8
        })
        .collect();

    let bytes = build_pdf_bytes_ext(jpeg, width, height, Some(smask_gray), 0);
    let dir = tempfile::tempdir().expect("tempdir");
    let path = write_fixture(dir.path(), "smask.pdf", &bytes);

    let response = compress_pdf_with_progress(
        path.to_str().unwrap(),
        maximum_settings(),
        noop_cancel_flag(),
        |_| {},
    )
    .expect("compression must succeed");

    assert!(
        response.images_recompressed >= 1,
        "the transparent image must be rewritten, not skipped"
    );

    // The output must still carry an SMask on a DCTDecode image stream.
    let output = PathBuf::from(&response.output_path);
    let reloaded = Document::load(&output).expect("output must be a valid PDF");
    let has_transparent_jpeg = reloaded.objects.values().any(|object| {
        let Object::Stream(stream) = object else {
            return false;
        };
        stream.dict.get(b"SMask").is_ok()
            && matches!(stream.dict.get(b"Filter"), Ok(Object::Name(f)) if f.as_slice() == b"DCTDecode")
    });
    assert!(
        has_transparent_jpeg,
        "the rewritten image must keep its soft mask"
    );
}

// ---------------------------------------------------------------------------
// Target-size mode
// ---------------------------------------------------------------------------

#[test]
fn target_size_mode_meets_budget() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = fresh_fixture(dir.path());
    let original = fs::metadata(&path).expect("fixture metadata").len();
    let target = original * 60 / 100;

    let mut progress = |_| {};
    let response = compress_pdf_to_target_size(
        path.to_str().unwrap(),
        target,
        maximum_settings(),
        noop_cancel_flag(),
        &mut progress,
    )
    .expect("target-size compression must succeed");

    let output = PathBuf::from(&response.output_path);
    let output_len = fs::metadata(&output).expect("output metadata").len();
    assert!(output_len <= target, "output {output_len} must fit {target}");
    assert!(response.notices.iter().any(|n| n.code == "compress.note.targetSizeMet"));
    assert_eq!(extracted_text(&path).trim(), extracted_text(&output).trim());
}

#[test]
fn target_size_mode_reports_best_effort() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = fresh_fixture(dir.path());

    // An impossible budget (2 KB for a photographic fixture): the engine must
    // still produce a valid best-effort output with a warning.
    let mut progress = |_| {};
    let response = compress_pdf_to_target_size(
        path.to_str().unwrap(),
        2_000,
        maximum_settings(),
        noop_cancel_flag(),
        &mut progress,
    )
    .expect("best-effort output must still be produced");

    assert!(PathBuf::from(&response.output_path).exists());
    assert!(
        response
            .notices
            .iter()
            .any(|n| n.code == "compress.warning.targetSizeMissed")
    );
}
