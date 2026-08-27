//! Pipeline integration tests — build real PDFs with lopdf and run them
//! through the analyze/compress pipeline end to end.
//! 管线集成测试 — 用 lopdf 构造真实 PDF，端到端运行分析/压缩管线。

use std::{
    fs,
    io::Cursor,
    path::{Path, PathBuf},
    sync::{atomic::AtomicBool, Arc},
    time::Instant,
};

use image::{codecs::jpeg::JpegEncoder, DynamicImage, GenericImageView};
use lopdf::{dictionary, Document, Object, Stream, StringFormat};

use super::analyzer::analyze_pdf_with_progress;
use super::compressor::compress_pdf_with_progress;
use super::settings::{BilevelCodec, CompressionSettings, CompressionSettingsOverrides};
use super::target_size::compress_pdf_to_target_size;
use crate::error::AppError;
use crate::testutil::{
    bilevel_scan_image, bilevel_scan_rgb_image, encode_ccitt_g4, encode_jpeg, fixture_rgb_image,
    FIXTURE_SEED,
};

const FIXTURE_TEXT: &str = "Pipeline integration fixture";

// ---------------------------------------------------------------------------
// Fixture construction
// ---------------------------------------------------------------------------

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
    doc.save_modern(&mut bytes)
        .expect("failed to save fixture PDF");
    bytes
}

fn build_pdf_bytes(jpeg: Vec<u8>, width: u32, height: u32) -> Vec<u8> {
    build_pdf_bytes_ext(jpeg, width, height, None, 0)
}

/// Build a one-page PDF whose single embedded image is a CCITT fax stream
/// with the given `K` parameter (G4 transcode fixtures).
fn build_ccitt_pdf_bytes(g4: Vec<u8>, width: u32, height: u32, k: i64) -> Vec<u8> {
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

    let image_id = doc.add_object(Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Image",
            "Width" => width as i64,
            "Height" => height as i64,
            "ColorSpace" => "DeviceGray",
            "BitsPerComponent" => 1,
            "Filter" => "CCITTFaxDecode",
            "DecodeParms" => dictionary! {
                "K" => k,
                "Columns" => width as i64,
                "Rows" => height as i64,
                "BlackIs1" => true,
            },
        },
        g4,
    ));
    let resources_id = doc.add_object(dictionary! {
        "Font" => dictionary! {
            "F1" => font_id,
        },
        "XObject" => dictionary! {
            "Im0" => image_id,
        },
    });
    let content = format!("q 400 0 0 300 72 400 cm /Im0 Do Q\nBT /F1 24 Tf 72 720 Td ({FIXTURE_TEXT}) Tj ET\n");
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
    doc.save_modern(&mut bytes)
        .expect("failed to save fixture PDF");
    bytes
}

fn write_fixture(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, bytes).expect("failed to write fixture");
    path
}

fn fresh_fixture(dir: &Path) -> PathBuf {
    let jpeg = encode_jpeg(fixture_rgb_image(1600, 1200), 95);
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

    let response =
        analyze_pdf_with_progress(path.to_str().unwrap(), |_| {}).expect("analysis must succeed");

    assert_eq!(response.page_count, 1);
    assert_eq!(response.image_object_count, 1);
    assert!(
        response.file_size_bytes > 10_000.0,
        "fixture should be sizable"
    );
    assert!(response.max_image_edge_px >= 1200);
}

/// Encrypt a saved fixture in place with the given passwords using lopdf's
/// standard security handler (V1/RC4).
fn encrypt_fixture(path: &Path, owner_password: &str, user_password: &str) {
    let mut document = Document::load(path).expect("load fixture for encryption");
    // The standard security handler derives its key from the file ID.
    if !document.trailer.has(b"ID") {
        document.trailer.set(
            "ID",
            Object::Array(vec![
                Object::String(vec![0x11; 16], StringFormat::Hexadecimal),
                Object::String(vec![0x22; 16], StringFormat::Hexadecimal),
            ]),
        );
    }
    let version = lopdf::EncryptionVersion::V1 {
        document: &document,
        owner_password,
        user_password,
        permissions: lopdf::Permissions::all(),
    };
    let state = lopdf::EncryptionState::try_from(version).expect("build encryption state");
    document.encrypt(&state).expect("encrypt fixture");
    document.save(path).expect("save encrypted fixture");
}

#[test]
fn encrypted_document_is_rejected_not_corrupted() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = fresh_fixture(dir.path());
    // A real user password: lopdf's empty-password auto-decrypt on load fails,
    // the object graph stays unparsed, and the guard must refuse the file.
    encrypt_fixture(&path, "owner", "user");

    for label in ["analyze", "compress", "target-size"] {
        let result = match label {
            "analyze" => analyze_pdf_with_progress(path.to_str().unwrap(), |_| {}).map(|_| ()),
            "compress" => compress_pdf_with_progress(
                path.to_str().unwrap(),
                maximum_settings(),
                noop_cancel_flag(),
                |_| {},
            )
            .map(|_| ()),
            _ => compress_pdf_to_target_size(
                path.to_str().unwrap(),
                100_000,
                maximum_settings(),
                noop_cancel_flag(),
                &mut |_| {},
            )
            .map(|_| ()),
        };
        match result {
            Err(AppError::Encrypted) => {}
            other => panic!("{label} must reject encrypted input, got {other:?}"),
        }
    }

    // No `__optimized-*.pdf` shell file may be left behind.
    let leftovers: Vec<_> = fs::read_dir(dir.path())
        .expect("read dir")
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .contains("__optimized-")
        })
        .collect();
    assert!(leftovers.is_empty(), "no output expected for encrypted PDF");
}

#[test]
fn owner_password_only_document_unlocks_and_compresses() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = fresh_fixture(dir.path());
    // Empty user password (owner-password-only): readable without a password.
    encrypt_fixture(&path, "owner-secret", "");

    let analysis = analyze_pdf_with_progress(path.to_str().unwrap(), |_| {})
        .expect("owner-password-only PDF must analyze");
    assert_eq!(analysis.page_count, 1);
    assert!(analysis.notices.iter().any(|notice| {
        notice.code == "analysis.note.encryptedUnlocked"
    }));

    let response = compress_pdf_with_progress(
        path.to_str().unwrap(),
        maximum_settings(),
        noop_cancel_flag(),
        |_| {},
    )
    .expect("owner-password-only PDF must compress");

    // The output must be a plain, readable, unencrypted PDF.
    let output = Document::load(&response.output_path).expect("reload output");
    assert_eq!(output.get_pages().len(), 1);
    assert!(
        !output.trailer.has(b"Encrypt"),
        "output must not carry the /Encrypt dictionary"
    );
    assert!(response.notices.iter().any(|notice| {
        notice.code == "compress.note.decryptedInput"
    }));
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
    let mut jpeg = encode_jpeg(fixture_rgb_image(1600, 1200), 95);
    for byte in jpeg.iter_mut().skip(64) {
        *byte = byte.wrapping_add(0x5A);
    }

    let dir = tempfile::tempdir().expect("tempdir");
    let path = write_fixture(
        dir.path(),
        "broken-image.pdf",
        &build_pdf_bytes(jpeg, 1600, 1200),
    );

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
    let bytes = build_pdf_bytes(encode_jpeg(fixture_rgb_image(800, 600), 90), 800, 600);
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
    let jpeg = encode_jpeg(fixture_rgb_image(1600, 1200), 95);
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
    assert!(response
        .notices
        .iter()
        .any(|n| n.code == "compress.note.imageDedupe"));

    let output = PathBuf::from(&response.output_path);
    let reloaded = Document::load(&output).expect("output must be a valid PDF");
    assert_eq!(reloaded.get_pages().len(), 1);
    assert_eq!(original_text.trim(), extracted_text(&output).trim());
}

/// Two pages whose content streams are separate objects with identical bytes —
/// the generic stream dedup must merge them and rewrite the /Contents edges.
#[test]
fn compress_dedupes_identical_content_streams() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let font_id = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
    });
    let resources_id = doc.add_object(dictionary! {
        "Font" => dictionary! { "F1" => font_id },
    });
    let content_bytes =
        format!("BT /F1 24 Tf 72 720 Td ({FIXTURE_TEXT}) Tj ET\n").into_bytes();
    let mut kids = Vec::new();
    for _ in 0..2 {
        // A separate content stream object per page, byte-identical.
        let content_id = doc.add_object(Stream::new(dictionary! {}, content_bytes.clone()));
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
            "Count" => 2,
        }),
    );
    let catalog_id = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", catalog_id);
    let mut bytes = Vec::new();
    doc.save_modern(&mut bytes).expect("save fixture");
    let path = write_fixture(dir.path(), "content-dedupe.pdf", &bytes);

    let response = compress_pdf_with_progress(
        path.to_str().unwrap(),
        maximum_settings(),
        noop_cancel_flag(),
        |_| {},
    )
    .expect("compression must succeed");

    assert!(
        response.notices.iter().any(|n| n.code == "compress.note.streamDedupe"),
        "expected a stream dedupe notice, got {:?}",
        response.notices
    );

    let output = PathBuf::from(&response.output_path);
    let reloaded = Document::load(&output).expect("output must be a valid PDF");
    assert_eq!(reloaded.get_pages().len(), 2, "both pages must survive");
    let both_pages = reloaded
        .extract_text(&[1, 2])
        .expect("both pages must stay extractable");
    assert_eq!(
        both_pages.trim(),
        format!("{FIXTURE_TEXT}\n{FIXTURE_TEXT}").trim(),
        "both pages must keep their text"
    );
}

/// In-edge rewriting must leave no stub objects behind (an indirect object
/// whose body is a bare reference), and duplicates of an image must collapse
/// into exactly one stream object.
#[test]
fn dedupe_rewrites_references_without_leaving_stubs() {
    let jpeg = encode_jpeg(fixture_rgb_image(1600, 1200), 95);
    let bytes = build_pdf_bytes_ext(jpeg, 1600, 1200, None, 3);
    let dir = tempfile::tempdir().expect("tempdir");
    let path = write_fixture(dir.path(), "stub-check.pdf", &bytes);

    let response = compress_pdf_with_progress(
        path.to_str().unwrap(),
        maximum_settings(),
        noop_cancel_flag(),
        |_| {},
    )
    .expect("compression must succeed");
    assert_eq!(response.images_deduplicated, 3);

    let reloaded = Document::load(&response.output_path).expect("output must be a valid PDF");
    let bare_references = reloaded
        .objects
        .values()
        .filter(|object| matches!(object, Object::Reference(_)))
        .count();
    assert_eq!(bare_references, 0, "no stub objects may remain");

    let image_objects = reloaded
        .objects
        .values()
        .filter(|object| {
            matches!(
                object,
                Object::Stream(stream)
                    if matches!(stream.dict.get(b"Subtype"), Ok(Object::Name(name)) if name.as_slice() == b"Image")
            )
        })
        .count();
    assert_eq!(image_objects, 1, "all duplicates must collapse into one image");
}

/// Identical images sharing one /SMask reference merge now that equivalence
/// covers full dictionaries (references to the same target stay mergeable).
#[test]
fn identical_images_with_shared_smask_merge() {
    let jpeg = encode_jpeg(fixture_rgb_image(1600, 1200), 95);
    let smask_gray = vec![128u8; 1600 * 1200];
    let bytes = build_pdf_bytes_ext(jpeg, 1600, 1200, Some(smask_gray), 1);
    let dir = tempfile::tempdir().expect("tempdir");
    let path = write_fixture(dir.path(), "smask-dedupe.pdf", &bytes);

    let response = compress_pdf_with_progress(
        path.to_str().unwrap(),
        maximum_settings(),
        noop_cancel_flag(),
        |_| {},
    )
    .expect("compression must succeed");

    assert_eq!(
        response.images_deduplicated, 1,
        "identical images with a shared soft mask must merge"
    );
    let reloaded = Document::load(&response.output_path).expect("output must be a valid PDF");
    assert_eq!(reloaded.get_pages().len(), 1);
}

// ---------------------------------------------------------------------------
// Unused resource cleanup
// ---------------------------------------------------------------------------

/// One page with two fonts (F1 used, F2 unused) and two images (Im0 unused,
/// Im1 used). Returns the built bytes plus the option to attach annotations.
#[allow(dead_code)]
fn build_unused_resources_pdf(annotations: bool) -> Vec<u8> {
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();

    let font_used = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
    });
    let font_unused = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Courier",
    });

    let image_unused = {
        let jpeg = encode_jpeg(fixture_rgb_image(400, 300), 90);
        doc.add_object(Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Image",
                "Width" => 400,
                "Height" => 300,
                "ColorSpace" => "DeviceRGB",
                "BitsPerComponent" => 8,
                "Filter" => "DCTDecode",
            },
            jpeg,
        ))
    };
    let image_used = {
        let jpeg = encode_jpeg(fixture_rgb_image(400, 300), 90);
        doc.add_object(Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Image",
                "Width" => 400,
                "Height" => 300,
                "ColorSpace" => "DeviceRGB",
                "BitsPerComponent" => 8,
                "Filter" => "DCTDecode",
            },
            jpeg,
        ))
    };

    let mut page_dict = dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "Resources" => dictionary! {
            "Font" => dictionary! {
                "F1" => font_used,
                "F2" => font_unused,
            },
            "XObject" => dictionary! {
                "Im0" => image_unused,
                "Im1" => image_used,
            },
        },
        "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
    };

    if annotations {
        let appearance = doc.add_object(Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Form",
                "BBox" => vec![0.into(), 0.into(), 10.into(), 10.into()],
            },
            "0 0 10 10 re f".as_bytes().to_vec(),
        ));
        let annotation = doc.add_object(dictionary! {
            "Type" => "Annot",
            "Subtype" => "Square",
            "Rect" => vec![10.into(), 10.into(), 100.into(), 100.into()],
            "AP" => dictionary! { "N" => appearance },
        });
        page_dict.set("Annots", vec![Object::Reference(annotation)]);
    }

    let content = format!(
        "q 300 0 0 225 72 400 cm /Im1 Do Q\nBT /F1 24 Tf 72 720 Td ({FIXTURE_TEXT}) Tj ET\n"
    );
    let content_id = doc.add_object(Stream::new(dictionary! {}, content.into_bytes()));
    page_dict.set("Contents", content_id);
    let page_id = doc.add_object(page_dict);
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

fn page_resource_names(document: &Document, category: &[u8]) -> Vec<String> {
    let page_id = *document
        .get_pages()
        .values()
        .next()
        .expect("at least one page");
    let Object::Dictionary(page_dict) = document
        .objects
        .get(&page_id)
        .expect("page object")
    else {
        panic!("page must be a dictionary");
    };
    let Ok(Object::Dictionary(category_dict)) = page_dict.get(b"Resources") else {
        panic!("page must carry resources");
    };
    let Ok(Object::Dictionary(entries)) = category_dict.get(category) else {
        panic!("category must exist: {}", String::from_utf8_lossy(category));
    };
    entries
        .iter()
        .map(|(key, _)| String::from_utf8_lossy(key).into_owned())
        .collect()
}

#[test]
fn removes_unused_font_and_xobject_entries() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = write_fixture(dir.path(), "unused-resources.pdf", &build_unused_resources_pdf(false));

    let response = compress_pdf_with_progress(
        path.to_str().unwrap(),
        maximum_settings(),
        noop_cancel_flag(),
        |_| {},
    )
    .expect("compression must succeed");

    assert!(
        response
            .notices
            .iter()
            .any(|n| n.code == "compress.note.resourcesCleaned"),
        "expected a resource cleanup notice, got {:?}",
        response.notices
    );

    let reloaded = Document::load(&response.output_path).expect("output must be a valid PDF");
    assert_eq!(page_resource_names(&reloaded, b"Font"), vec!["F1"]);
    assert_eq!(page_resource_names(&reloaded, b"XObject"), vec!["Im1"]);

    // The orphaned unused image must not survive in the output.
    let image_objects = reloaded
        .objects
        .values()
        .filter(|object| {
            matches!(
                object,
                Object::Stream(stream)
                    if matches!(stream.dict.get(b"Subtype"), Ok(Object::Name(name)) if name.as_slice() == b"Image")
            )
        })
        .count();
    assert_eq!(image_objects, 1, "only the used image may remain");

    assert!(
        extracted_text(Path::new(&response.output_path)).contains(FIXTURE_TEXT),
        "text must survive the cleanup"
    );
}

#[test]
fn keeps_resources_when_annotation_appearance_present() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = write_fixture(dir.path(), "annotated.pdf", &build_unused_resources_pdf(true));

    let response = compress_pdf_with_progress(
        path.to_str().unwrap(),
        maximum_settings(),
        noop_cancel_flag(),
        |_| {},
    )
    .expect("compression must succeed");

    let reloaded = Document::load(&response.output_path).expect("output must be a valid PDF");
    // Annotation appearance streams may fall back to the page's resources —
    // the page keeps everything, including the unused entries.
    let fonts = page_resource_names(&reloaded, b"Font");
    assert!(fonts.contains(&"F2".to_string()), "fonts stay untouched: {fonts:?}");
    let xobjects = page_resource_names(&reloaded, b"XObject");
    assert!(
        xobjects.contains(&"Im0".to_string()),
        "xobjects stay untouched: {xobjects:?}"
    );
    let _ = response;
}

// ---------------------------------------------------------------------------
// Font subsetting
// ---------------------------------------------------------------------------

#[cfg(feature = "subset-fonts")]
fn subset_settings() -> CompressionSettings {
    CompressionSettings::from_sources(
        None,
        CompressionSettingsOverrides {
            preset: Some("maximum".to_string()),
            subset_fonts: Some(true),
            ..Default::default()
        },
    )
}

#[cfg(feature = "subset-fonts")]
#[test]
fn subsets_cid_truetype_font_to_used_glyphs() {
    use crate::testutil::{
        build_type0_pdf_bytes, TEST_FONT, TEST_FONT_GID_A, TEST_FONT_GID_D, TEST_FONT_GID_F,
        TEST_FONT_GID_P,
    };

    let dir = tempfile::tempdir().expect("tempdir");
    let path = write_fixture(dir.path(), "type0.pdf", &build_type0_pdf_bytes());

    let response = compress_pdf_with_progress(
        path.to_str().unwrap(),
        subset_settings(),
        noop_cancel_flag(),
        |_| {},
    )
    .expect("compression must succeed");

    assert!(
        response
            .notices
            .iter()
            .any(|n| n.code == "compress.note.fontsSubsetted"),
        "expected a font subsetting notice, got {:?}",
        response.notices
    );

    let reloaded = Document::load(&response.output_path).expect("output must be a valid PDF");

    // FontFile2 must be much smaller and still carry a valid TrueType header.
    let mut font_file_len = 0usize;
    let mut descendant: Option<&lopdf::Dictionary> = None;
    for object in reloaded.objects.values() {
        match object {
            Object::Stream(stream) if stream.dict.get(b"Length1").is_ok() => {
                let program = stream.get_plain_content().unwrap_or_default();
                font_file_len = program.len();
                assert!(
                    program.len() < TEST_FONT.len() / 2,
                    "subset must be far smaller: {} vs {}",
                    program.len(),
                    TEST_FONT.len()
                );
                assert_eq!(&program[..4], b"\x00\x01\x00\x00", "still a TrueType file");
            }
            Object::Dictionary(dict)
                if matches!(dict.get(b"Subtype"), Ok(Object::Name(sub)) if sub.as_slice() == b"CIDFontType2") =>
            {
                descendant = Some(dict);
            }
            _ => {}
        }
    }
    assert!(font_file_len > 0, "FontFile2 must survive");

    // CIDToGIDMap bridges the unchanged CIDs to the new numbering.
    let descendant = descendant.expect("descendant CIDFont must survive");
    let map_id = match descendant.get(b"CIDToGIDMap") {
        Ok(Object::Reference(map_id)) => *map_id,
        other => panic!("expected a CIDToGIDMap stream, got {other:?}"),
    };
    let Object::Stream(map_stream) = reloaded.objects.get(&map_id).expect("map object") else {
        panic!("CIDToGIDMap must be a stream");
    };
    let cid_to_gid_map = map_stream.get_plain_content().expect("map content");
    let gid_at = |cid: u16| {
        let offset = cid as usize * 2;
        u16::from_be_bytes([cid_to_gid_map[offset], cid_to_gid_map[offset + 1]])
    };
    let new_gid_p = gid_at(TEST_FONT_GID_P);
    let new_gid_d = gid_at(TEST_FONT_GID_D);
    let new_gid_f = gid_at(TEST_FONT_GID_F);
    assert_ne!(new_gid_p, 0, "used CID must map to a real glyph");
    assert_ne!(new_gid_d, 0);
    assert_ne!(new_gid_f, 0);
    // An unused CID (A = 19) must not map anywhere.
    assert_eq!(gid_at(TEST_FONT_GID_A), 0);

    // /W widths must follow the new numbering.
    let Ok(Object::Array(widths)) = descendant.get(b"W") else {
        panic!("W array must survive");
    };
    let mut width_for = std::collections::HashMap::new();
    let mut index = 0;
    while index + 1 < widths.len() {
        if let (Object::Integer(gid), Object::Array(values)) = (&widths[index], &widths[index + 1])
        {
            if let Some(Object::Integer(width)) = values.first() {
                width_for.insert(*gid, *width);
            }
        }
        index += 2;
    }
    assert_eq!(width_for.get(&i64::from(new_gid_p)), Some(&650), "P keeps 650");
    assert_eq!(width_for.get(&i64::from(new_gid_d)), Some(&700), "D keeps 700");
    assert_eq!(width_for.get(&i64::from(new_gid_f)), Some(&600), "F keeps 600");

    // The page content must be untouched — the original CID bytes still there.
    let page_id = *reloaded.get_pages().values().next().expect("page");
    let content = reloaded
        .get_and_decode_page_content(page_id)
        .expect("content decodes");
    let text_operands: Vec<String> = content
        .operations
        .iter()
        .flat_map(|operation| {
            operation
                .operands
                .iter()
                .filter_map(|operand| match operand {
                    Object::String(bytes, _) => Some(bytes.clone()),
                    _ => None,
                })
                .collect::<Vec<_>>()
        })
        .flat_map(|bytes| bytes.chunks_exact(2).map(|pair| format!("{:02x}{:02x}", pair[0], pair[1])).collect::<Vec<_>>())
        .collect();
    assert!(
        text_operands.contains(&"0022".to_string())
            && text_operands.contains(&"0016".to_string())
            && text_operands.contains(&"0018".to_string()),
        "content CIDs unchanged: {text_operands:?}"
    );
}

#[cfg(feature = "subset-fonts")]
#[test]
fn subset_fonts_off_keeps_font_program_unchanged() {
    use crate::testutil::{build_type0_pdf_bytes, TEST_FONT};

    let dir = tempfile::tempdir().expect("tempdir");
    let path = write_fixture(dir.path(), "type0-off.pdf", &build_type0_pdf_bytes());

    let settings = CompressionSettings::from_sources(
        None,
        CompressionSettingsOverrides {
            preset: Some("maximum".to_string()),
            ..Default::default()
        },
    );
    let response = compress_pdf_with_progress(
        path.to_str().unwrap(),
        settings,
        noop_cancel_flag(),
        |_| {},
    )
    .expect("compression must succeed");
    assert!(
        !response
            .notices
            .iter()
            .any(|n| n.code == "compress.note.fontsSubsetted")
    );

    let reloaded = Document::load(&response.output_path).expect("output must be a valid PDF");
    for object in reloaded.objects.values() {
        if let Object::Stream(stream) = object {
            if stream.dict.get(b"Length1").is_ok() {
                let program = stream.get_plain_content().unwrap_or_default();
                assert_eq!(program.as_slice(), TEST_FONT, "font program must be untouched");
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Color-space support (ICC / Indexed / aliases)
// ---------------------------------------------------------------------------

/// How the fixture's single raw image declares its color space.
enum ColorSpaceFixture {
    /// Image dict carries `[/ICCBased <icc stream>]`; the profile stream is
    /// created with the given `/N`.
    DirectIcc { n: i64 },
    /// Image dict carries `[/Indexed <base> <hival> <lookup string>]`.
    DirectIndexed { base: Object, hival: i64 },
    /// Image dict carries the plain name `/CS0`; the page resources map
    /// `/CS0` to an `[/ICCBased …]` array with the given `/N`.
    AliasIcc { n: i64 },
}

/// One page with a single flate-compressed raw image using the given color
/// space shape. Pixels are a smooth gradient so the source stays compact.
fn build_color_space_pdf(
    fixture: ColorSpaceFixture,
    width: u32,
    height: u32,
    bits_per_component: i64,
    pixels: Vec<u8>,
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

    // Palette for indexed fixtures: a smooth RGB ramp.
    let palette: Vec<u8> = match &fixture {
        ColorSpaceFixture::DirectIndexed { hival, .. } => (0..=*hival as u8)
            .flat_map(|index| {
                let ramp = (index as u32 * 255 / 255) as u8;
                vec![ramp, 255 - ramp, (ramp / 2) + 64]
            })
            .collect(),
        _ => Vec::new(),
    };

    let mut alias_entry: Option<Object> = None;
    let color_space_value: Object = match fixture {
        ColorSpaceFixture::DirectIcc { n } | ColorSpaceFixture::AliasIcc { n } => {
            let icc_id = doc.add_object(Stream::new(
                dictionary! { "N" => n, "Length" => 4 },
                vec![0x01, 0x02, 0x03, 0x04],
            ));
            let array = Object::Array(vec![
                Object::Name(b"ICCBased".to_vec()),
                Object::Reference(icc_id),
            ]);
            if matches!(fixture, ColorSpaceFixture::AliasIcc { .. }) {
                alias_entry = Some(array);
                Object::Name(b"CS0".to_vec())
            } else {
                array
            }
        }
        ColorSpaceFixture::DirectIndexed { base, hival } => Object::Array(vec![
            Object::Name(b"Indexed".to_vec()),
            base,
            Object::Integer(hival),
            Object::String(palette, StringFormat::Literal),
        ]),
    };

    let mut image_stream = Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Image",
            "Width" => width as i64,
            "Height" => height as i64,
            "ColorSpace" => color_space_value,
            "BitsPerComponent" => bits_per_component,
        },
        pixels,
    );
    let _ = image_stream.compress();
    let image_id = doc.add_object(image_stream);

    let mut resources = dictionary! {
        "Font" => dictionary! { "F1" => font_id },
        "XObject" => dictionary! { "Im0" => image_id },
    };
    if let Some(alias) = alias_entry {
        resources.set("ColorSpace", dictionary! { "CS0" => alias });
    }
    let resources_id = doc.add_object(resources);
    let content = format!(
        "q 400 0 0 300 72 400 cm /Im0 Do Q\nBT /F1 24 Tf 72 720 Td ({FIXTURE_TEXT}) Tj ET\n"
    );
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
    doc.save_modern(&mut bytes).expect("save fixture");
    bytes
}

/// The single image stream of a reloaded document.
fn sole_image_stream(document: &Document) -> &Stream {
    document
        .objects
        .values()
        .find_map(|object| match object {
            Object::Stream(stream)
                if matches!(stream.dict.get(b"Subtype"), Ok(Object::Name(name)) if name.as_slice() == b"Image") =>
            {
                Some(stream)
            }
            _ => None,
        })
        .expect("exactly one image stream")
}

#[test]
fn icc_rgb_raw_image_recompresses_and_keeps_profile() {
    let rgb = crate::testutil::gradient_rgb_image(2000, 1500, FIXTURE_SEED).into_raw();
    let bytes = build_color_space_pdf(
        ColorSpaceFixture::DirectIcc { n: 3 },
        2000,
        1500,
        8,
        rgb,
    );
    let dir = tempfile::tempdir().expect("tempdir");
    let path = write_fixture(dir.path(), "icc-rgb.pdf", &bytes);

    let response = compress_pdf_with_progress(
        path.to_str().unwrap(),
        maximum_settings(),
        noop_cancel_flag(),
        |_| {},
    )
    .expect("compression must succeed");
    assert!(response.output_was_smaller, "ICC RGB raw image must shrink");

    let reloaded = Document::load(&response.output_path).expect("output must be a valid PDF");
    let image = sole_image_stream(&reloaded);
    assert!(
        matches!(image.dict.get(b"Filter"), Ok(Object::Name(name)) if name.as_slice() == b"DCTDecode"),
        "image must be re-encoded as JPEG"
    );
    // The ICC profile reference must survive the rewrite.
    let Object::Array(color_space) = image.dict.get(b"ColorSpace").expect("ColorSpace") else {
        panic!("ICC array must be preserved, got {:?}", image.dict.get(b"ColorSpace"));
    };
    assert!(matches!(&color_space[0], Object::Name(name) if name.as_slice() == b"ICCBased"));
    let Object::Reference(icc_id) = &color_space[1] else {
        panic!("second element must be the profile reference");
    };
    let Object::Stream(icc) = reloaded.objects.get(icc_id).expect("ICC stream must survive") else {
        panic!("profile must stay a stream");
    };
    assert_eq!(icc.dict.get(b"N").ok(), Some(&Object::Integer(3)));

    assert!(
        extracted_text(Path::new(&response.output_path)).contains(FIXTURE_TEXT),
        "text must survive the rewrite"
    );
}

#[test]
fn icc_cmyk_raw_image_is_skipped() {
    // CMYK plane: 4 channels of gradient.
    let mut pixels = Vec::with_capacity(2000 * 1500 * 4);
    for y in 0..1500u32 {
        for x in 0..2000u32 {
            pixels.push((x % 256) as u8);
            pixels.push((y % 256) as u8);
            pixels.push(((x + y) % 256) as u8);
            pixels.push(64);
        }
    }
    let bytes = build_color_space_pdf(
        ColorSpaceFixture::DirectIcc { n: 4 },
        2000,
        1500,
        8,
        pixels,
    );
    let dir = tempfile::tempdir().expect("tempdir");
    let path = write_fixture(dir.path(), "icc-cmyk.pdf", &bytes);

    let response = compress_pdf_with_progress(
        path.to_str().unwrap(),
        maximum_settings(),
        noop_cancel_flag(),
        |_| {},
    )
    .expect("compression must succeed");
    assert_eq!(response.images_recompressed, 0, "CMYK ICC must stay untouched");

    let reloaded = Document::load(&response.output_path).expect("output must be a valid PDF");
    let image = sole_image_stream(&reloaded);
    assert_eq!(
        image.dict.get(b"BitsPerComponent").ok(),
        Some(&Object::Integer(8)),
        "raw plane must be untouched"
    );
}

#[test]
fn indexed_8bit_image_expands_palette() {
    // 8-bit palette indices from a luma ramp.
    let mut indices = Vec::with_capacity(2000 * 1500);
    for y in 0..1500u32 {
        for x in 0..2000u32 {
            indices.push(((x * 255 / 1999) ^ (y * 255 / 1499)) as u8);
        }
    }
    let bytes = build_color_space_pdf(
        ColorSpaceFixture::DirectIndexed {
            base: Object::Name(b"DeviceRGB".to_vec()),
            hival: 255,
        },
        2000,
        1500,
        8,
        indices,
    );
    let dir = tempfile::tempdir().expect("tempdir");
    let path = write_fixture(dir.path(), "indexed-8.pdf", &bytes);

    let response = compress_pdf_with_progress(
        path.to_str().unwrap(),
        maximum_settings(),
        noop_cancel_flag(),
        |_| {},
    )
    .expect("compression must succeed");
    assert!(response.output_was_smaller, "indexed image must shrink");

    let reloaded = Document::load(&response.output_path).expect("output must be a valid PDF");
    let image = sole_image_stream(&reloaded);
    assert!(
        matches!(image.dict.get(b"Filter"), Ok(Object::Name(name)) if name.as_slice() == b"DCTDecode"),
        "palette image must be re-encoded as flat JPEG"
    );
    assert!(
        matches!(image.dict.get(b"ColorSpace"), Ok(Object::Name(cs)) if cs.as_slice() == b"DeviceRGB"),
        "output must declare the base space, got {:?}",
        image.dict.get(b"ColorSpace")
    );
    assert_eq!(
        image.dict.get(b"BitsPerComponent").ok(),
        Some(&Object::Integer(8))
    );
}

#[test]
fn indexed_4bit_image_expands_palette() {
    // 4-bit nibble indices from deterministic noise — a poorly compressible
    // source so the JPEG rewrite actually has room to win.
    let mut state: u32 = FIXTURE_SEED;
    let mut indices = Vec::with_capacity(2000 * 1500);
    for _ in 0..2000 * 1500 {
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        indices.push(((state >> 24) & 0x0F) as u8);
    }
    let bytes = build_color_space_pdf(
        ColorSpaceFixture::DirectIndexed {
            base: Object::Name(b"DeviceGray".to_vec()),
            hival: 15,
        },
        2000,
        1500,
        4,
        indices,
    );
    let dir = tempfile::tempdir().expect("tempdir");
    let path = write_fixture(dir.path(), "indexed-4.pdf", &bytes);

    let response = compress_pdf_with_progress(
        path.to_str().unwrap(),
        maximum_settings(),
        noop_cancel_flag(),
        |_| {},
    )
    .expect("compression must succeed");
    assert!(response.output_was_smaller, "4-bit indexed image must shrink");

    let reloaded = Document::load(&response.output_path).expect("output must be a valid PDF");
    let image = sole_image_stream(&reloaded);
    assert!(
        matches!(image.dict.get(b"ColorSpace"), Ok(Object::Name(cs)) if cs.as_slice() == b"DeviceGray"),
        "gray base space must be declared"
    );
}

#[test]
fn color_space_alias_resolves_through_resources() {
    let rgb = crate::testutil::gradient_rgb_image(2000, 1500, FIXTURE_SEED).into_raw();
    let bytes = build_color_space_pdf(
        ColorSpaceFixture::AliasIcc { n: 3 },
        2000,
        1500,
        8,
        rgb,
    );
    let dir = tempfile::tempdir().expect("tempdir");
    let path = write_fixture(dir.path(), "alias-icc.pdf", &bytes);

    let response = compress_pdf_with_progress(
        path.to_str().unwrap(),
        maximum_settings(),
        noop_cancel_flag(),
        |_| {},
    )
    .expect("compression must succeed");
    assert!(
        response.output_was_smaller,
        "the aliased ICC image must become compressible"
    );

    let reloaded = Document::load(&response.output_path).expect("output must be a valid PDF");
    let image = sole_image_stream(&reloaded);
    // The rebuilt stream re-declares the ICC array inline (the alias name is
    // meaningless outside the original resource context).
    let Object::Array(color_space) = image.dict.get(b"ColorSpace").expect("ColorSpace") else {
        panic!("rebuilt image must carry the resolved ICC array");
    };
    assert!(matches!(&color_space[0], Object::Name(name) if name.as_slice() == b"ICCBased"));
}

// ---------------------------------------------------------------------------
// JPEG encoder evaluation (image crate vs jpeg-encoder SIMD)
// ---------------------------------------------------------------------------

#[test]
fn jpeg_encoder_candidate_produces_valid_comparable_output() {
    let (width, height) = (1600u32, 1200u32);
    let rgb = fixture_rgb_image(width, height);
    let dynamic = DynamicImage::ImageRgb8(rgb.clone());

    for quality in [58u8, 72, 82] {
        let mut current_cursor = Cursor::new(Vec::new());
        let mut current_encoder = JpegEncoder::new_with_quality(&mut current_cursor, quality);
        current_encoder
            .encode_image(&dynamic)
            .expect("image-crate encode");
        let current_bytes = current_cursor.into_inner();

        let mut candidate_bytes = Vec::new();
        let candidate_encoder = jpeg_encoder::Encoder::new(&mut candidate_bytes, quality);
        candidate_encoder
            .encode(
                rgb.as_raw(),
                width as u16,
                height as u16,
                jpeg_encoder::ColorType::Rgb,
            )
            .expect("jpeg-encoder encode");

        // The candidate's output must decode back through the image crate
        // (the pipeline's decoder) to identical dimensions.
        let decoded = image::load_from_memory(&candidate_bytes)
            .expect("candidate JPEG must be decodable by the image crate");
        assert_eq!(decoded.dimensions(), (width, height));

        // Sizes are recorded with --nocapture; require the candidate to stay
        // within 30% of the current encoder at the same quality number.
        println!(
            "q{quality}: image-crate {} bytes, jpeg-encoder {} bytes ({:+.1}%)",
            current_bytes.len(),
            candidate_bytes.len(),
            (candidate_bytes.len() as f64 / current_bytes.len() as f64 - 1.0) * 100.0
        );
        assert!(
            candidate_bytes.len() <= current_bytes.len() + current_bytes.len() / 3,
            "candidate output at q{quality} is disproportionately larger"
        );
    }
}

// ---------------------------------------------------------------------------
// Soft-mask (transparency) recompression
// ---------------------------------------------------------------------------

#[test]
fn compress_rewrites_smask_alpha() {
    let (width, height) = (1600u32, 1200u32);
    let jpeg = encode_jpeg(fixture_rgb_image(width, height), 95);

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
// Grayscale re-encoding
// ---------------------------------------------------------------------------

#[test]
fn grayscale_mode_rewrites_color_images_as_device_gray() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = fresh_fixture(dir.path());

    let settings = CompressionSettings::from_sources(
        None,
        CompressionSettingsOverrides {
            preset: Some("maximum".to_string()),
            grayscale: Some(true),
            ..Default::default()
        },
    );

    let response =
        compress_pdf_with_progress(path.to_str().unwrap(), settings, noop_cancel_flag(), |_| {})
            .expect("compression must succeed");
    assert!(
        response.images_recompressed >= 1,
        "fixture image must be rewritten"
    );

    let output = PathBuf::from(&response.output_path);
    let reloaded = Document::load(&output).expect("output must be a valid PDF");

    // Every rewritten image must declare DeviceGray and its progressive JPEG
    // payload must decode back through the pipeline's decoder as luma.
    let mut checked = 0;
    for object in reloaded.objects.values() {
        let Object::Stream(stream) = object else {
            continue;
        };
        if !matches!(stream.dict.get(b"Subtype"), Ok(Object::Name(name)) if name.as_slice() == b"Image")
        {
            continue;
        }
        assert!(
            matches!(stream.dict.get(b"ColorSpace"), Ok(Object::Name(cs)) if cs.as_slice() == b"DeviceGray"),
            "rewritten images must be grayscale, got {:?}",
            stream.dict.get(b"ColorSpace")
        );
        // DCTDecode content is the raw JPEG payload — no PDF-side decompression.
        let decoded = image::load_from_memory(&stream.content).expect("JPEG payload must decode");
        assert!(
            !decoded.color().has_color(),
            "decoded image must be single-channel"
        );
        checked += 1;
    }
    assert!(checked >= 1, "at least one image must have been rewritten");
}

// ---------------------------------------------------------------------------
// CCITT Group 4 (bi-level) re-encoding
// ---------------------------------------------------------------------------

fn g4_settings() -> CompressionSettings {
    CompressionSettings::from_sources(
        None,
        CompressionSettingsOverrides {
            preset: Some("maximum".to_string()),
            bilevel_codec: Some(BilevelCodec::CcittG4),
            ..Default::default()
        },
    )
}

/// All image XObject streams of a loaded document.
fn image_streams(document: &Document) -> Vec<&Stream> {
    document
        .objects
        .values()
        .filter_map(|object| match object {
            Object::Stream(stream)
                if matches!(stream.dict.get(b"Subtype"), Ok(Object::Name(name)) if name.as_slice() == b"Image") =>
            {
                Some(stream)
            }
            _ => None,
        })
        .collect()
}

#[test]
fn bilevel_mode_rewrites_near_bilevel_jpegs_as_ccitt_g4() {
    let dir = tempfile::tempdir().expect("tempdir");
    let jpeg = encode_jpeg(bilevel_scan_rgb_image(2000, 1500, FIXTURE_SEED), 88);
    let path = write_fixture(
        dir.path(),
        "bilevel-jpeg.pdf",
        &build_pdf_bytes(jpeg, 2000, 1500),
    );

    let response = compress_pdf_with_progress(
        path.to_str().unwrap(),
        g4_settings(),
        noop_cancel_flag(),
        |_| {},
    )
    .expect("compression must succeed");
    assert!(response.output_was_smaller, "G4 output must beat the JPEG");

    let reloaded = Document::load(&response.output_path).expect("output must be a valid PDF");
    let streams = image_streams(&reloaded);
    assert_eq!(streams.len(), 1, "fixture has exactly one image");
    let stream = streams[0];
    assert!(
        matches!(stream.dict.get(b"Filter"), Ok(Object::Name(name)) if name.as_slice() == b"CCITTFaxDecode"),
        "near-bilevel image must be re-encoded as CCITT G4, got {:?}",
        stream.dict.get(b"Filter")
    );
    let parms = match stream.dict.get(b"DecodeParms") {
        Ok(Object::Dictionary(parms)) => parms,
        other => panic!("G4 stream must carry DecodeParms, got {other:?}"),
    };
    assert_eq!(parms.get(b"K").ok(), Some(&Object::Integer(-1)));
    assert_eq!(parms.get(b"BlackIs1").ok(), Some(&Object::Boolean(true)));
    assert_eq!(
        stream.dict.get(b"BitsPerComponent").ok(),
        Some(&Object::Integer(1))
    );
    assert!(
        matches!(stream.dict.get(b"ColorSpace"), Ok(Object::Name(cs)) if cs.as_slice() == b"DeviceGray")
    );
    // Max-edge of the maximum preset is 1400 px — the 2000 px input must be
    // resampled accordingly.
    assert_eq!(stream.dict.get(b"Width").ok(), Some(&Object::Integer(1400)));
    assert_eq!(stream.dict.get(b"Height").ok(), Some(&Object::Integer(1050)));

    assert!(
        extracted_text(Path::new(&response.output_path)).contains(FIXTURE_TEXT),
        "text must survive the rewrite"
    );
}

#[test]
fn bilevel_mode_keeps_continuous_tone_images_on_jpeg() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = fresh_fixture(dir.path());

    let response = compress_pdf_with_progress(
        path.to_str().unwrap(),
        g4_settings(),
        noop_cancel_flag(),
        |_| {},
    )
    .expect("compression must succeed");

    let reloaded = Document::load(&response.output_path).expect("output must be a valid PDF");
    for stream in image_streams(&reloaded) {
        assert!(
            matches!(stream.dict.get(b"Filter"), Ok(Object::Name(name)) if name.as_slice() == b"DCTDecode"),
            "photographic content must stay on JPEG, got {:?}",
            stream.dict.get(b"Filter")
        );
    }
}

#[test]
fn ccitt_g4_input_transcodes_when_resize_needed() {
    let dir = tempfile::tempdir().expect("tempdir");
    let image = bilevel_scan_image(2400, 1800, FIXTURE_SEED);
    let g4 = encode_ccitt_g4(&image);
    let path = write_fixture(
        dir.path(),
        "ccitt-in.pdf",
        &build_ccitt_pdf_bytes(g4, 2400, 1800, -1),
    );

    let response = compress_pdf_with_progress(
        path.to_str().unwrap(),
        g4_settings(),
        noop_cancel_flag(),
        |_| {},
    )
    .expect("compression must succeed");
    assert!(response.output_was_smaller, "resampled G4 must be smaller");

    let reloaded = Document::load(&response.output_path).expect("output must be a valid PDF");
    let streams = image_streams(&reloaded);
    assert_eq!(streams.len(), 1);
    let stream = streams[0];
    assert!(
        matches!(stream.dict.get(b"Filter"), Ok(Object::Name(name)) if name.as_slice() == b"CCITTFaxDecode")
    );
    assert_eq!(stream.dict.get(b"Width").ok(), Some(&Object::Integer(1400)));
    assert_eq!(stream.dict.get(b"Height").ok(), Some(&Object::Integer(1050)));

    // The rewritten payload must still be a decodable G4 stream with the
    // declared dimensions.
    let mut rows = 0u32;
    let decoded = fax::decoder::decode_g4(
        stream.content.iter().copied(),
        1400,
        Some(1050),
        |_| rows += 1,
    );
    assert!(decoded.is_some(), "output G4 payload must decode");
    assert_eq!(rows, 1050);

    assert!(
        extracted_text(Path::new(&response.output_path)).contains(FIXTURE_TEXT),
        "text must survive the rewrite"
    );
}

#[test]
fn ccitt_g3_input_is_skipped_unchanged() {
    let dir = tempfile::tempdir().expect("tempdir");
    let image = bilevel_scan_image(1600, 1200, FIXTURE_SEED);
    let g4_bytes = encode_ccitt_g4(&image);
    let path = write_fixture(
        dir.path(),
        "ccitt-g3.pdf",
        &build_ccitt_pdf_bytes(g4_bytes.clone(), 1600, 1200, 0),
    );

    let response = compress_pdf_with_progress(
        path.to_str().unwrap(),
        g4_settings(),
        noop_cancel_flag(),
        |_| {},
    )
    .expect("compression must succeed");
    assert_eq!(
        response.images_recompressed, 0,
        "Group 3 input is not decodable and must stay untouched"
    );
    assert!(response.images_skipped >= 1);

    let reloaded = Document::load(&response.output_path).expect("output must be a valid PDF");
    let streams = image_streams(&reloaded);
    assert_eq!(streams.len(), 1);
    let stream = streams[0];
    let Object::Dictionary(parms) = stream.dict.get(b"DecodeParms").unwrap() else {
        panic!("DecodeParms must survive");
    };
    assert_eq!(parms.get(b"K").ok(), Some(&Object::Integer(0)));
    assert_eq!(stream.content, g4_bytes, "payload must be byte-identical");
}

#[test]
fn target_size_mode_with_bilevel_g4() {
    let dir = tempfile::tempdir().expect("tempdir");
    let jpeg = encode_jpeg(bilevel_scan_rgb_image(2000, 1500, FIXTURE_SEED), 88);
    let path = write_fixture(
        dir.path(),
        "bilevel-target.pdf",
        &build_pdf_bytes(jpeg, 2000, 1500),
    );
    let original = fs::metadata(&path).expect("fixture metadata").len();
    let target = original * 40 / 100;

    let response = compress_pdf_to_target_size(
        path.to_str().unwrap(),
        target,
        g4_settings(),
        noop_cancel_flag(),
        &mut |_| {},
    )
    .expect("target-size compression must succeed");
    assert!(
        (response.compressed_size_bytes as u64) <= target,
        "G4 output must fit the budget ({} > {})",
        response.compressed_size_bytes,
        target
    );

    let reloaded = Document::load(&response.output_path).expect("output must be a valid PDF");
    assert!(
        image_streams(&reloaded).iter().any(|stream| matches!(
            stream.dict.get(b"Filter"),
            Ok(Object::Name(name)) if name.as_slice() == b"CCITTFaxDecode"
        )),
        "the winning round must have materialized a G4 stream"
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
    assert!(
        output_len <= target,
        "output {output_len} must fit {target}"
    );
    assert!(response
        .notices
        .iter()
        .any(|n| n.code == "compress.note.targetSizeMet"));
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
    assert!(response
        .notices
        .iter()
        .any(|n| n.code == "compress.warning.targetSizeMissed"));
}

// ---------------------------------------------------------------------------
// Skip-policy adaptive behavior (image-heavy documents, grayscale forcing)
// ---------------------------------------------------------------------------

/// Build a PDF with `count` distinct small gradient JPEG images (one per
/// page), each sized to land between the tiny and small skip thresholds.
fn build_many_small_image_pdf(dir: &Path, count: usize) -> PathBuf {
    use crate::testutil::{encode_jpeg, gradient_rgb_image};

    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let mut kids = Vec::new();

    for index in 0..count {
        let image = gradient_rgb_image(800, 600, 0xA1B2_C3D4u32.wrapping_add(index as u32));
        let jpeg = encode_jpeg(image, 90);
        let image_id = doc.add_object(Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Image",
                "Width" => 800,
                "Height" => 600,
                "ColorSpace" => "DeviceRGB",
                "BitsPerComponent" => 8,
                "Filter" => "DCTDecode",
            },
            jpeg,
        ));
        let resources_id = doc.add_object(dictionary! {
            "XObject" => dictionary! { "Im0" => image_id },
        });
        let content_id = doc.add_object(Stream::new(
            dictionary! {},
            b"q 500 0 0 375 48 400 cm /Im0 Do Q\n".to_vec(),
        ));
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
            "Count" => count as i64,
        }),
    );
    let catalog_id = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });
    doc.trailer.set("Root", catalog_id);

    let path = dir.join("many-small-images.pdf");
    doc.save(&path).expect("save many-image fixture");
    path
}

fn balanced_settings() -> CompressionSettings {
    CompressionSettings::from_sources(
        None,
        CompressionSettingsOverrides {
            preset: Some("balanced".to_string()),
            ..Default::default()
        },
    )
}

#[test]
fn image_heavy_documents_recompress_small_streams() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = build_many_small_image_pdf(dir.path(), 30);

    // Sanity: every image sits between the tiny floor and the small-stream
    // skip threshold, within the balanced edge — the exact band the lift
    // is designed to unlock.
    let document = Document::load(&path).expect("load fixture");
    let sizes: Vec<usize> = document
        .objects
        .values()
        .filter_map(|object| match object {
            Object::Stream(stream) if stream.dict.get(b"Filter").is_ok() => {
                Some(stream.content.len())
            }
            _ => None,
        })
        .collect();
    assert_eq!(sizes.len(), 30);
    for size in sizes {
        assert!(
            size > super::encode::TINY_JPEG_STREAM_BYTES && size <= 64 * 1024,
            "fixture image of {size} bytes is outside the small-stream band"
        );
    }

    let response = compress_pdf_with_progress(
        path.to_str().unwrap(),
        balanced_settings(),
        noop_cancel_flag(),
        |_| {},
    )
    .expect("compression must succeed");
    assert_eq!(
        response.images_recompressed, 30,
        "image-heavy documents must re-encode compact small images"
    );
    assert!(response.compressed_size_bytes < response.original_size_bytes);
}

#[test]
fn small_document_keeps_skipping_small_streams() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = build_many_small_image_pdf(dir.path(), 4);

    let response = compress_pdf_with_progress(
        path.to_str().unwrap(),
        balanced_settings(),
        noop_cancel_flag(),
        |_| {},
    )
    .expect("compression must succeed");
    assert_eq!(
        response.images_recompressed, 0,
        "below the image-count threshold, small within-target images stay skipped"
    );
}

#[test]
fn grayscale_mode_forces_small_image_recompression() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = build_many_small_image_pdf(dir.path(), 4);
    let settings = CompressionSettings::from_sources(
        None,
        CompressionSettingsOverrides {
            preset: Some("balanced".to_string()),
            grayscale: Some(true),
            ..Default::default()
        },
    );

    let response = compress_pdf_with_progress(
        path.to_str().unwrap(),
        settings,
        noop_cancel_flag(),
        |_| {},
    )
    .expect("compression must succeed");
    assert_eq!(
        response.images_recompressed, 4,
        "an explicit grayscale request must override the small-stream skip"
    );

    // The rewritten images must be DeviceGray.
    let output = Document::load(&response.output_path).expect("reload output");
    let gray_images = output
        .objects
        .values()
        .filter_map(|object| match object {
            Object::Stream(stream) => Some(stream),
            _ => None,
        })
        .filter(|stream| {
            stream
                .dict
                .get(b"Subtype")
                .ok()
                .and_then(|value| value.as_name().ok())
                == Some(b"Image".as_slice())
        })
        .filter(|stream| {
            stream
                .dict
                .get(b"ColorSpace")
                .ok()
                .and_then(|value| value.as_name().ok())
                == Some(b"DeviceGray".as_slice())
        })
        .count();
    assert_eq!(gray_images, 4);
}

// ---------------------------------------------------------------------------
// Output-not-smaller guard
// ---------------------------------------------------------------------------

#[test]
fn compress_never_writes_a_larger_output() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = fresh_fixture(dir.path());
    let original_bytes = fs::read(&path).expect("read original");

    let mut document = Document::load(&path).expect("load fixture");
    document.prune_objects();
    let mut stats = crate::pdf::compressor::CompressionStats::default();
    let output_path = dir.path().join("would-be-output.pdf");

    // Claim an original far smaller than any serialization can be.
    let response = super::compressor::save_and_build_response(
        &mut document,
        &output_path,
        64,
        Instant::now(),
        &maximum_settings(),
        &mut stats,
    )
    .expect("response must be built");

    assert!(!response.output_was_smaller);
    assert_eq!(response.output_path, "");
    assert_eq!(response.saved_bytes, 0.0);
    assert_eq!(response.savings_percent, 0.0);
    assert!(
        !output_path.exists(),
        "nothing may be written when it cannot win"
    );
    assert!(response
        .notices
        .iter()
        .any(|notice| notice.code == "compress.warning.outputNotSmaller"));
    assert_eq!(fs::read(&path).expect("re-read original"), original_bytes);
}

// ---------------------------------------------------------------------------
// Analyzer estimate honesty (unsupported codecs excluded)
// ---------------------------------------------------------------------------

#[test]
fn analysis_estimate_excludes_undecodable_codecs() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = fresh_fixture(dir.path());

    // Relabel the fixture image as JPXDecode — a codec with no safe
    // re-encode path — so nothing image-based is actionable anymore.
    let mut document = Document::load(&path).expect("load fixture");
    for object in document.objects.values_mut() {
        if let Object::Stream(stream) = object {
            if stream
                .dict
                .get(b"Subtype")
                .ok()
                .and_then(|value| value.as_name().ok())
                == Some(b"Image".as_slice())
            {
                stream.dict.set("Filter", Object::Name(b"JPXDecode".to_vec()));
            }
        }
    }
    document.save(&path).expect("save relabeled fixture");

    let analysis = analyze_pdf_with_progress(path.to_str().unwrap(), |_| {})
        .expect("analysis must succeed");
    assert!(analysis
        .notices
        .iter()
        .any(|notice| notice.code == "analysis.warning.unsupportedImageCodecs"));
    // With the only image excluded, the estimate must fall to its floor band
    // instead of promising double-digit image savings.
    assert!(
        analysis.estimated_savings_percent < 10.0,
        "estimate must exclude undecodable image bytes, got {}",
        analysis.estimated_savings_percent
    );
}
