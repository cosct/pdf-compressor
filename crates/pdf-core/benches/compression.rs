//! Compression pipeline benchmark — tracks time and throughput across presets
//! on a deterministic photographic fixture.
//! 压缩管线基准 — 在确定性照片型夹具上跟踪各预设的耗时与吞吐。

use std::{
    fs,
    path::PathBuf,
    sync::{atomic::AtomicBool, Arc},
};

use std::hint::black_box;

use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use lopdf::{dictionary, Document, Object, Stream};

use pdf_core::pdf::{
    compress_pdf_with_progress, CompressionSettings, CompressionSettingsOverrides,
};
use pdf_core::testutil::{encode_jpeg, fixture_rgb_image};

fn build_fixture_pdf(path: &PathBuf) -> usize {
    const WIDTH: u32 = 1600;
    const HEIGHT: u32 = 1200;
    let jpeg = encode_jpeg(fixture_rgb_image(WIDTH, HEIGHT), 95);

    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let font_id = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
    });
    let image_id = doc.add_object(Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Image",
            "Width" => WIDTH as i64,
            "Height" => HEIGHT as i64,
            "ColorSpace" => "DeviceRGB",
            "BitsPerComponent" => 8,
            "Filter" => "DCTDecode",
        },
        jpeg,
    ));
    let resources_id = doc.add_object(dictionary! {
        "Font" => dictionary! { "F1" => font_id },
        "XObject" => dictionary! { "Im0" => image_id },
    });
    let content =
        b"q 400 0 0 300 72 400 cm /Im0 Do Q\nBT /F1 24 Tf 72 720 Td (Bench fixture) Tj ET\n";
    let content_id = doc.add_object(Stream::new(dictionary! {}, content.to_vec()));
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
    doc.save(path).expect("failed to save bench fixture");

    fs::metadata(path).expect("fixture metadata").len() as usize
}

fn settings_for(preset: &str) -> CompressionSettings {
    CompressionSettings::from_sources(
        None,
        CompressionSettingsOverrides {
            preset: Some(preset.to_string()),
            ..Default::default()
        },
    )
}

fn bench_compression(c: &mut Criterion) {
    let dir = tempfile::tempdir().expect("tempdir");
    let fixture = dir.path().join("bench-fixture.pdf");
    let fixture_len = build_fixture_pdf(&fixture);
    let fixture_path = fixture.to_str().expect("utf-8 path").to_string();

    let mut group = c.benchmark_group("compress");
    group.throughput(Throughput::Bytes(fixture_len as u64));
    group.sample_size(10);

    for preset in ["maximum", "balanced", "conservative"] {
        group.bench_function(preset, |b| {
            b.iter(|| {
                // Fresh output location per iteration: build_output_path
                // deduplicates output names, so a shared directory would
                // exhaust the 100-name budget across criterion iterations.
                let out_dir = tempfile::tempdir().expect("tempdir");
                let settings = CompressionSettings {
                    output_dir: Some(out_dir.path().to_string_lossy().into_owned()),
                    ..settings_for(preset)
                };
                let response = compress_pdf_with_progress(
                    black_box(&fixture_path),
                    black_box(None),
                    black_box(settings),
                    Arc::new(AtomicBool::new(false)),
                    |_| {},
                )
                .expect("bench compression must succeed");
                black_box(response);
            })
        });
    }

    group.finish();
}

criterion_group!(benches, bench_compression);
criterion_main!(benches);
