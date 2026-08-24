//! Build a synthetic multi-image photographic PDF for manual benchmarking.
//! 生成多页照片型 PDF，用于手动基准测试。
//!
//! Usage: make_fixture <output.pdf> [images] [width] [height] [quality]

use std::io::Cursor;

use image::{codecs::jpeg::JpegEncoder, DynamicImage, GenericImageView};
use lopdf::{dictionary, Document, Object, Stream};

use pdf_core::testutil::{deterministic_rgb_image, FIXTURE_SEED};

fn main() {
    let mut args = std::env::args().skip(1);
    let output = args.next().expect("output path");
    let image_count: usize = args.next().map_or(8, |v| v.parse().expect("count"));
    let width: u32 = args.next().map_or(1600, |v| v.parse().expect("width"));
    let height: u32 = args.next().map_or(1200, |v| v.parse().expect("height"));
    let quality: u8 = args.next().map_or(95, |v| v.parse().expect("quality"));

    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let font_id = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
    });

    let mut kids = Vec::new();
    for index in 0..image_count {
        let rgb =
            deterministic_rgb_image(width, height, FIXTURE_SEED.wrapping_add(index as u32));
        let dynamic = DynamicImage::ImageRgb8(rgb);
        let (w, h) = dynamic.dimensions();
        let mut cursor = Cursor::new(Vec::new());
        JpegEncoder::new_with_quality(&mut cursor, quality)
            .encode_image(&dynamic)
            .expect("encode fixture JPEG");
        let image_id = doc.add_object(Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Image",
                "Width" => w as i64,
                "Height" => h as i64,
                "ColorSpace" => "DeviceRGB",
                "BitsPerComponent" => 8,
                "Filter" => "DCTDecode",
            },
            cursor.into_inner(),
        ));
        let resources_id = doc.add_object(dictionary! {
            "Font" => dictionary! { "F1" => font_id },
            "XObject" => dictionary! { "Im0" => image_id },
        });
        let content = format!(
            "q 500 0 0 375 48 400 cm /Im0 Do Q\nBT /F1 24 Tf 72 720 Td (Page {}) Tj ET\n",
            index + 1
        );
        let content_id = doc.add_object(Stream::new(dictionary! {}, content.into_bytes()));
        let page_id = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "Contents" => content_id,
            "Resources" => resources_id,
            "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        });
        kids.push(Object::Reference(page_id));
    }

    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => kids,
            "Count" => image_count as i64,
        }),
    );
    let catalog_id = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });
    doc.trailer.set("Root", catalog_id);
    doc.save(&output).expect("save fixture");

    let bytes = std::fs::metadata(&output).expect("metadata").len();
    println!("wrote {output}: {bytes} bytes, {image_count} images ({width}x{height} q{quality})");
}
