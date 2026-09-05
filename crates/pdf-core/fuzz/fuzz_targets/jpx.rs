#![no_main]

//! Fuzz target for the JPX (JPEG 2000) decode path — the feature-gated C
//! surface. Arbitrary bytes are wrapped as the codestream of a JPXDecode
//! image inside a minimal valid PDF, padded past the small-stream skip so
//! the compressor always reaches `decode_jpx_stream` and hands the bytes to
//! OpenJPEG. The generic `pipeline` target cannot realistically evolve a
//! valid PDF with a JPX stream from scratch, so the hostile-codestream duty
//! lives here.
//! JPX（JPEG 2000）解码路径的模糊测试目标 — feature 门控的 C 解码面。
//! 任意字节被包进一个最小合法 PDF 的 JPXDecode 图像流，并填充到超过
//! 小流跳过阈值，确保压缩器总是抵达 `decode_jpx_stream` 并把字节交给
//! OpenJPEG。通用 pipeline 目标几乎不可能从随机字节演化出带 JPX 流的
//! 合法 PDF，所以对抗性码流的职责放在这里。
//!
//! Run from `crates/pdf-core/` with:
//! `cargo +nightly fuzz run jpx -- -max_total_time=60`

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use libfuzzer_sys::fuzz_target;
use lopdf::{dictionary, Object, Stream};

use pdf_core::{compress_pdf_with_progress, CompressionSettings, CompressionSettingsOverrides};

/// Fixed dictionary dimensions, deliberately different from what any evolved
/// codestream will carry, so the codestream-vs-dictionary mismatch check is
/// exercised as well. Large enough to clear the trivial-pixel skip.
const FUZZ_WIDTH: i64 = 256;
const FUZZ_HEIGHT: i64 = 256;

fuzz_target!(|data: &[u8]| {
    // Pad past the small-stream skip so the decode attempt is guaranteed;
    // trailing bytes after a codestream's EOC are legal and must be
    // tolerated, so the padding itself is worth fuzzing too.
    let mut codestream = data.to_vec();
    codestream.resize(data.len().max(96 * 1024), 0xAB);

    let mut doc = lopdf::Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let image_id = doc.add_object(Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Image",
            "Width" => FUZZ_WIDTH,
            "Height" => FUZZ_HEIGHT,
            "ColorSpace" => "DeviceRGB",
            "BitsPerComponent" => 8,
            "Filter" => "JPXDecode",
        },
        codestream,
    ));
    let content_id = doc.add_object(Stream::new(
        dictionary! {},
        b"q 100 0 0 100 0 0 cm /Im0 Do Q\n".to_vec(),
    ));
    let resources_id = doc.add_object(dictionary! {
        "XObject" => dictionary! { "Im0" => image_id },
    });
    let page_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "Contents" => content_id,
        "Resources" => resources_id,
        "MediaBox" => vec![Object::Integer(0), 0.into(), 100.into(), 100.into()],
    });
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![Object::Reference(page_id)],
            "Count" => 1,
        }),
    );
    let catalog_id = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", catalog_id);

    let mut bytes = Vec::new();
    if doc.save_modern(&mut bytes).is_err() {
        return;
    }

    let dir = std::env::temp_dir().join(format!("pdf-core-fuzz-jpx-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("input.pdf");
    if std::fs::write(&path, &bytes).is_err() {
        return;
    }

    let settings = CompressionSettings::from_sources(None, CompressionSettingsOverrides::default());
    let _ = compress_pdf_with_progress(
        &path.to_string_lossy(),
        None,
        settings,
        Arc::new(AtomicBool::new(false)),
        |_| {},
    );

    let _ = std::fs::remove_file(&path);
});
