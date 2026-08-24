//! JPEG encoder evaluation — image crate's built-in encoder vs the
//! jpeg-encoder crate (SIMD build) on the pipeline's own fixture generator.
//! JPEG 编码器评估 — 在管线同款夹具上对比 image 内置编码器与 jpeg-encoder（SIMD）。

use std::io::Cursor;

use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};
use image::{codecs::jpeg::JpegEncoder, DynamicImage};

use pdf_core::testutil::fixture_rgb_image;

fn bench_encoders(c: &mut Criterion) {
    const WIDTH: u32 = 1600;
    const HEIGHT: u32 = 1200;
    let rgb = fixture_rgb_image(WIDTH, HEIGHT);
    let dynamic = DynamicImage::ImageRgb8(rgb.clone());
    let raw_pixels = rgb.as_raw();

    let mut group = c.benchmark_group("jpeg-encode");
    group.throughput(Throughput::Bytes((WIDTH * HEIGHT * 3) as u64));
    group.sample_size(20);

    for quality in [58u8, 72, 82] {
        group.bench_function(format!("image-crate/q{quality}"), |b| {
            b.iter(|| {
                let mut cursor = Cursor::new(Vec::with_capacity(200 * 1024));
                let mut encoder = JpegEncoder::new_with_quality(&mut cursor, quality);
                encoder
                    .encode_image(black_box(&dynamic))
                    .expect("image-crate encode");
                black_box(cursor.into_inner()).len()
            })
        });

        group.bench_function(format!("jpeg-encoder-simd/q{quality}"), |b| {
            b.iter(|| {
                let mut output = Vec::with_capacity(200 * 1024);
                let encoder = jpeg_encoder::Encoder::new(&mut output, quality);
                encoder
                    .encode(
                        black_box(raw_pixels),
                        WIDTH as u16,
                        HEIGHT as u16,
                        jpeg_encoder::ColorType::Rgb,
                    )
                    .expect("jpeg-encoder encode");
                black_box(output).len()
            })
        });
    }

    group.finish();
}

criterion_group!(benches, bench_encoders);
criterion_main!(benches);
