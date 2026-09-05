//! Deterministic fixture builders shared by the integration tests, the
//! criterion benchmarks, and the examples.
//! 确定性夹具生成器 — 集成测试、criterion 基准与示例共用。
//!
//! Enabled via the `testutil` feature, which is turned on by this crate's
//! dev-dependency on itself — normal (non-dev) builds never compile it.
//! 通过 testutil 特性启用（由本 crate 对自身的 dev-dependency 打开），
//! 正常构建不会编译本模块。

use std::io::Cursor;

use image::{codecs::jpeg::JpegEncoder, DynamicImage, GrayImage, Luma, Rgb, RgbImage};
use lopdf::{dictionary, Object, Stream};

/// gids inside `assets/test-font.ttf` (deterministic — the file is committed;
/// regenerate only via pyftsubset with the documented charset).
pub const TEST_FONT: &[u8] = include_bytes!("../assets/test-font.ttf");
pub const TEST_FONT_GID_A: u16 = 19;
pub const TEST_FONT_GID_D: u16 = 22;
pub const TEST_FONT_GID_F: u16 = 24;
pub const TEST_FONT_GID_P: u16 = 34;

/// Peak signal-to-noise ratio (dB) between two same-sized images on the luma
/// channel. `None` when the dimensions differ; `INFINITY` for identical
/// planes. The quality-regression gates compare a fixture's source plane
/// against the decoded output of a compression run.
pub fn luma_psnr_db(left: &DynamicImage, right: &DynamicImage) -> Option<f64> {
    let left = left.to_luma8();
    let right = right.to_luma8();
    if left.dimensions() != right.dimensions() || left.is_empty() {
        return None;
    }

    let squared_error: f64 = left
        .as_raw()
        .iter()
        .zip(right.as_raw())
        .map(|(&a, &b)| {
            let difference = i32::from(a) - i32::from(b);
            f64::from(difference * difference)
        })
        .sum();
    let mean_squared_error = squared_error / left.as_raw().len() as f64;
    if mean_squared_error == 0.0 {
        return Some(f64::INFINITY);
    }
    Some(10.0 * (255.0f64 * 255.0 / mean_squared_error).log10())
}

/// One page drawing "PDF" (gids 34/22/24) through a Type0/CIDFontType2 font
/// with the full 77-glyph test font embedded as FontFile2 — the font
/// subsetting fixture.
pub fn build_type0_pdf_bytes() -> Vec<u8> {
    let font = TEST_FONT;
    let mut doc = lopdf::Document::with_version("1.5");
    let pages_id = doc.new_object_id();

    let font_file_id = doc.add_object(Stream::new(
        dictionary! {
            "Length" => (font.len() as i64),
            "Length1" => (font.len() as i64),
        },
        font.to_vec(),
    ));
    let descriptor_id = doc.add_object(dictionary! {
        "Type" => "FontDescriptor",
        "FontName" => "TestFont",
        "Flags" => 4,
        "FontBBox" => vec![
            Object::Integer(0),
            Object::Integer(-200),
            Object::Integer(1200),
            Object::Integer(900),
        ],
        "ItalicAngle" => 0,
        "Ascent" => 900,
        "Descent" => Object::Integer(-200),
        "CapHeight" => 700,
        "StemV" => 80,
        "FontFile2" => font_file_id,
    });

    // Distinct per-glyph widths so the /W remap is observable.
    let widths: Vec<Object> = vec![
        Object::Integer(i64::from(TEST_FONT_GID_D)),
        Object::Array(vec![700.into()]),
        Object::Integer(i64::from(TEST_FONT_GID_F)),
        Object::Array(vec![600.into()]),
        Object::Integer(i64::from(TEST_FONT_GID_P)),
        Object::Array(vec![650.into()]),
    ];
    let descendant_id = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "CIDFontType2",
        "BaseFont" => "TestFont",
        "CIDSystemInfo" => dictionary! {
            "Registry" => Object::string_literal("Adobe"),
            "Ordering" => Object::string_literal("Identity"),
            "Supplement" => 0,
        },
        "FontDescriptor" => descriptor_id,
        "DW" => 1000,
        "W" => widths,
        "CIDToGIDMap" => "Identity",
    });

    // ToUnicode maps the drawn gids to P/D/F so text extraction works.
    let to_unicode_id = doc.add_object(Stream::new(
        dictionary! {},
        b"/CIDInit /ProcSet findresource begin
12 dict begin
begincmap
/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def
/CMapName /Adobe-Identity-UCS def
/CMapType 2 def
1 begincodespacerange
<0000> <ffff>
endcodespacerange
3 beginbfchar
<0016> <0044>
<0018> <0046>
<0022> <0050>
endbfchar
endcmap
CMapName currentdict /CMap defineresource pop
end
end"
            .to_vec(),
    ));

    let type0_id = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type0",
        "BaseFont" => "TestFont",
        "Encoding" => "Identity-H",
        "DescendantFonts" => vec![Object::Reference(descendant_id)],
        "ToUnicode" => to_unicode_id,
    });

    let resources_id = doc.add_object(dictionary! {
        "Font" => dictionary! { "F1" => type0_id },
    });
    // Hex string with CIDs 34 (P), 22 (D), 24 (F).
    let content = b"BT /F1 24 Tf 72 720 Td <002200160018> Tj ET\n";
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
    let catalog_id = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", catalog_id);

    let mut bytes = Vec::new();
    doc.save_modern(&mut bytes).expect("save fixture");
    bytes
}

/// Gradient plus deterministic noise: behaves like a photograph under JPEG
/// (poorly compressible at high quality) while staying reproducible for a
/// given `seed`.
pub fn deterministic_rgb_image(width: u32, height: u32, seed: u32) -> RgbImage {
    let mut image = RgbImage::new(width, height);
    let mut state: u32 = seed;

    for y in 0..height {
        for x in 0..width {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let noise = ((state >> 24) & 0xFF) as i32 - 128;
            let channel = |value: u32, span: u32| (value.saturating_mul(255) / span) as i32;
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

/// The historical default seed used by the pipeline fixtures — keeps
/// generated bytes identical to the pre-`testutil` inline copies.
pub const FIXTURE_SEED: u32 = 0x1234_5678;

/// `deterministic_rgb_image` with the standard fixture seed.
pub fn fixture_rgb_image(width: u32, height: u32) -> RgbImage {
    deterministic_rgb_image(width, height, FIXTURE_SEED)
}

/// Smooth gradient with a light deterministic dither: compresses to the
/// "small but not tiny" JPEG range (a few tens of KB at moderate quality),
/// mimicking compact real-world scans for skip-heuristic tests. Reproducible
/// for a given `seed`.
pub fn gradient_rgb_image(width: u32, height: u32, seed: u32) -> RgbImage {
    let mut image = RgbImage::new(width, height);
    let mut state: u32 = seed;
    let band = (width / 32).max(1);

    for y in 0..height {
        for x in 0..width {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let dither = ((state >> 28) & 0x7) as i32 - 4;
            let wave = ((x + y + (seed % 64)) / band) as i32 * 3;
            let components = [
                (128 + wave + dither).clamp(0, 255) as u8,
                (96 + wave / 2 + dither).clamp(0, 255) as u8,
                (160 + wave * 2 / 3 + dither).clamp(0, 255) as u8,
            ];
            image.put_pixel(x, y, Rgb(components));
        }
    }

    image
}

/// Encode an RGB image as JPEG with the image crate's built-in encoder.
pub fn encode_jpeg(image: RgbImage, quality: u8) -> Vec<u8> {
    let dynamic = DynamicImage::ImageRgb8(image);
    let mut cursor = Cursor::new(Vec::new());
    JpegEncoder::new_with_quality(&mut cursor, quality)
        .encode_image(&dynamic)
        .expect("failed to encode fixture JPEG");
    cursor.into_inner()
}

/// A bilevel "scanned text page": white background with thick black text
/// lines. Zero midtone pixels, few edges — the shape CCITT Group 4 compresses
/// extremely well. Deterministic for a given `seed`.
pub fn bilevel_scan_image(width: u32, height: u32, seed: u32) -> GrayImage {
    let mut image = GrayImage::from_pixel(width, height, Luma([255u8]));
    let mut state: u32 = seed;
    let band = (height / 24).max(4);
    let mut y = band;

    while y + band <= height.saturating_sub(band) {
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let text_height = (band * 2 / 5).max(2);
        let left = (state % (width / 8).max(1)) + width / 16;
        let right = width - ((state >> 8) % (width / 8).max(1)) - width / 16;
        for dy in 0..text_height {
            if y + dy >= height {
                break;
            }
            for x in left..right.max(left + 1) {
                image.put_pixel(x, y + dy, Luma([0u8]));
            }
        }
        y += band;
    }

    image
}

/// `bilevel_scan_image` converted to RGB (for the JPEG fixture encoder).
pub fn bilevel_scan_rgb_image(width: u32, height: u32, seed: u32) -> RgbImage {
    DynamicImage::ImageLuma8(bilevel_scan_image(width, height, seed)).to_rgb8()
}

/// Encode a grayscale image as CCITT Group 4 (luma >= 128 → white). Used to
/// build CCITT *input* fixtures; mirrors the engine's T.6 conventions
/// (`BlackIs1: true`).
#[cfg(feature = "ccitt")]
pub fn encode_ccitt_g4(image: &GrayImage) -> Vec<u8> {
    let (width, _) = image.dimensions();
    let mut encoder = fax::encoder::Encoder::new(fax::VecWriter::new());
    for row in image.as_raw().chunks(width as usize) {
        let _ = encoder.encode_line(
            row.iter()
                .map(|&luma| if luma >= 128 { fax::Color::White } else { fax::Color::Black }),
            width,
        );
    }
    let writer = match encoder.finish() {
        Ok(writer) => writer,
        Err(infallible) => match infallible {},
    };
    writer.finish()
}

/// One MH run length (T.4 one-dimensional): makeup codes for every full
/// 64-block (capped at 2560), then the terminating code for the remainder.
#[cfg(feature = "ccitt")]
fn write_g3_run(writer: &mut fax::VecWriter, mut run: u16, color: fax::Color) {
    use fax::BitWriter as _;

    let code = |value: u16| {
        match color {
            fax::Color::Black => fax::maps::black::encode(value),
            fax::Color::White => fax::maps::white::encode(value),
        }
        .expect("valid MH run code")
    };

    while run >= 64 {
        let makeup = run / 64 * 64;
        let makeup = makeup.min(2560);
        let _ = writer.write(code(makeup));
        run -= makeup;
    }
    let _ = writer.write(code(run));
}

/// Encode a grayscale image as one-dimensional Group 3 MH (K = 0) without EOL
/// markers — PDF's default `/CCITTFaxDecode` shape. Used to build G3 *input*
/// fixtures; luma >= 128 renders white, matching `BlackIs1: true`.
#[cfg(feature = "ccitt")]
pub fn encode_ccitt_g3_1d(image: &GrayImage, byte_align: bool, with_eol: bool) -> Vec<u8> {
    use fax::BitWriter as _;

    let (width, _) = image.dimensions();
    let mut writer = fax::VecWriter::new();

    let eol = fax::maps::EOL;
    if with_eol {
        // An EOL before the first row, as a TIFF-style G3 file starts.
        let _ = writer.write(eol);
    }

    for row in image.as_raw().chunks(width as usize) {
        // Rows are alternating white/black runs and must start with a white
        // code — a leading black pixel needs a zero-length white run first.
        let mut color = fax::Color::White;
        let mut index = 0usize;
        if row.first().is_some_and(|&luma| luma < 128) {
            write_g3_run(&mut writer, 0, fax::Color::White);
            color = fax::Color::Black;
        }
        while index < row.len() {
            let black = row[index] < 128;
            let start = index;
            while index < row.len() && (row[index] < 128) == black {
                index += 1;
            }
            write_g3_run(&mut writer, (index - start) as u16, color);
            color = !color;
        }
        if with_eol {
            let _ = writer.write(eol);
        } else if byte_align {
            writer.pad();
        }
    }

    if with_eol {
        // RTC: six consecutive EOLs terminate the coded image.
        for _ in 0..6 {
            let _ = writer.write(eol);
        }
    }

    // `finish` pads the trailing partial byte with zeros — legal in both shapes.
    writer.finish()
}

