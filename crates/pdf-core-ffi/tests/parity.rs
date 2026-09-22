//! Contract pins: DTO parity, error-mapping totality, preset-table mirror.
//! 契约钉子：DTO 对齐、错误映射全覆盖、预设表镜像。
//!
//! These freeze the mobile mirror against the engine's facts: the settings
//! field set, the 11→8 error collapse, and the preset table values read
//! from `CompressionPreset`. If either side drifts, these fail — the same
//! discipline as the desktop's locales/bindings pins.

use std::io;

use pdf_core::models::CompressionSettingsPayload;
use pdf_core::{AppError, CompressionSettings, CompressionSettingsOverrides};
use pdf_core_ffi::{build_features, preset_defaults, FfiError, FfiSettings};

// ---------------------------------------------------------------------------
// Error mapping: total over the engine's 11 variants
// ---------------------------------------------------------------------------

#[test]
fn error_mapping_covers_every_engine_variant() {
    let expectations: [(AppError, FfiError); 11] = [
        (
            AppError::MissingInput("/missing.pdf".into()),
            FfiError::MissingInput,
        ),
        (
            AppError::InvalidPdfPath("/not-a.pdf".into()),
            FfiError::InvalidPdf,
        ),
        (
            AppError::InputTooLarge {
                size_bytes: 600 * 1024 * 1024,
                limit_bytes: 512 * 1024 * 1024,
            },
            FfiError::InputTooLarge {
                size_bytes: 600 * 1024 * 1024,
                limit_bytes: 512 * 1024 * 1024,
            },
        ),
        (AppError::Encrypted, FfiError::EncryptedPdf),
        (AppError::PasswordRequired, FfiError::PasswordRequired),
        (AppError::WrongPassword, FfiError::WrongPassword),
        (
            AppError::Io(io::Error::other("probe")),
            FfiError::Engine {
                detail: "probe".to_string(),
            },
        ),
        (
            AppError::Cancelled("task-1".to_string()),
            FfiError::Cancelled,
        ),
        (
            AppError::Config("probe".to_string()),
            FfiError::Engine {
                detail: "probe".to_string(),
            },
        ),
        (
            AppError::Opener("probe".to_string()),
            FfiError::Engine {
                detail: "probe".to_string(),
            },
        ),
        (
            AppError::PdfBuild("probe".to_string()),
            FfiError::Engine {
                detail: "probe".to_string(),
            },
        ),
    ];
    for (engine, expected) in expectations {
        let mapped = FfiError::from(engine);
        assert!(
            matches_same_shape(&mapped, &expected),
            "engine error mapped to {mapped:?}, expected {expected:?}"
        );
    }
}

/// Shape compare without deriving PartialEq on the whole enum (the fielded
/// variants compare their fields).
fn matches_same_shape(actual: &FfiError, expected: &FfiError) -> bool {
    match (actual, expected) {
        (FfiError::MissingInput, FfiError::MissingInput)
        | (FfiError::InvalidPdf, FfiError::InvalidPdf)
        | (FfiError::EncryptedPdf, FfiError::EncryptedPdf)
        | (FfiError::PasswordRequired, FfiError::PasswordRequired)
        | (FfiError::WrongPassword, FfiError::WrongPassword)
        | (FfiError::Cancelled, FfiError::Cancelled) => true,
        (
            FfiError::InputTooLarge {
                size_bytes: a_size,
                limit_bytes: a_limit,
            },
            FfiError::InputTooLarge {
                size_bytes: e_size,
                limit_bytes: e_limit,
            },
        ) => a_size == e_size && a_limit == e_limit,
        (FfiError::Engine { detail: a }, FfiError::Engine { detail: e }) => a == e,
        _ => false,
    }
}

#[test]
fn ffi_error_codes_stay_within_the_engine_taxonomy() {
    // The Kotlin layer resolves these against strings.xml keys that mirror
    // the desktop locales; a code outside the closed set would render as a
    // raw key on the device.
    let codes = [
        FfiError::MissingInput.code(),
        FfiError::InvalidPdf.code(),
        FfiError::InputTooLarge {
            size_bytes: 1,
            limit_bytes: 2,
        }
        .code(),
        FfiError::EncryptedPdf.code(),
        FfiError::PasswordRequired.code(),
        FfiError::WrongPassword.code(),
        FfiError::Cancelled.code(),
        FfiError::Engine {
            detail: String::new(),
        }
        .code(),
    ];
    let closed_set = [
        "error.missingInput",
        "error.invalidPdfPath",
        "error.inputTooLarge",
        "error.encryptedPdf",
        "error.passwordRequired",
        "error.wrongPassword",
        "error.io",
        "error.cancelled",
        "error.config",
        "error.opener",
        "error.pdfBuild",
    ];
    for code in codes {
        assert!(
            closed_set.contains(&code),
            "{code} drifted out of the engine's error taxonomy"
        );
    }
}

// ---------------------------------------------------------------------------
// Settings DTO parity
// ---------------------------------------------------------------------------

#[test]
fn settings_mirror_roundtrips_every_engine_field() {
    let settings = FfiSettings {
        preset: Some("maximum".to_string()),
        image_quality: Some(55),
        max_image_size_px: Some(2048),
        max_image_size_percent: Some(60),
        optimize_images: Some(false),
        compress_streams: Some(false),
        strip_metadata: Some(true),
        grayscale: Some(true),
        bilevel_codec: Some("jpeg".to_string()),
        subset_fonts: Some(true),
        cmyk_conversion: Some(false),
    };
    let payload: CompressionSettingsPayload = settings.to_payload();

    // Field-for-field: the mobile mirror must carry every knob the engine's
    // payload accepts except output_dir (SAF owns output locations).
    assert_eq!(payload.preset.as_deref(), Some("maximum"));
    assert_eq!(payload.image_quality, Some(55));
    assert_eq!(payload.max_image_size_px, Some(2048));
    assert_eq!(payload.max_image_size_percent, Some(60));
    assert_eq!(payload.optimize_images, Some(false));
    assert_eq!(payload.compress_streams, Some(false));
    assert_eq!(payload.strip_metadata, Some(true));
    assert_eq!(payload.grayscale, Some(true));
    assert_eq!(payload.bilevel_codec.as_deref(), Some("jpeg"));
    assert_eq!(payload.subset_fonts, Some(true));
    assert_eq!(payload.cmyk_conversion, Some(false));
    assert!(
        payload.output_dir.is_none(),
        "the ffi layer must never steer the engine's filesystem output"
    );
}

#[test]
fn settings_resolve_through_the_engine_normalizer_unchanged() {
    // The ffi layer must add no normalization of its own: the resolved
    // engine settings come from from_sources verbatim. Pin the balanced
    // preset defaults (quality 60 / percent 68) plus two explicit overrides.
    let resolved = CompressionSettings::from_sources(
        Some(FfiSettings::default().to_payload()),
        CompressionSettingsOverrides::default(),
    );
    assert_eq!(resolved.image_quality, 60);
    assert_eq!(resolved.max_image_size_percent, Some(68));
    assert!(resolved.optimize_images);
    assert!(resolved.compress_streams);
    // The unset preset resolves to balanced, and balanced strips metadata
    // (only conservative keeps it — see CompressionPreset::default_strip_metadata).
    assert!(resolved.strip_metadata);
    assert!(!resolved.grayscale);
    assert!(!resolved.subset_fonts);
    assert!(resolved.cmyk_conversion);

    let resolved = CompressionSettings::from_sources(
        Some(
            FfiSettings {
                image_quality: Some(40),
                max_image_size_px: Some(1200),
                ..FfiSettings::default()
            }
            .to_payload(),
        ),
        CompressionSettingsOverrides::default(),
    );
    assert_eq!(resolved.image_quality, 40);
    assert_eq!(resolved.max_image_size_px, 1200);
    assert_eq!(
        resolved.max_image_size_percent, None,
        "an explicit absolute cap switches the percent semantics off"
    );
}

// ---------------------------------------------------------------------------
// Preset table mirror (single source of truth: CompressionPreset)
// ---------------------------------------------------------------------------

#[test]
fn preset_defaults_pin_the_engine_table() {
    let presets = preset_defaults();
    let names: Vec<&str> = presets.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["conservative", "balanced", "maximum"]);

    // Values pinned to the 0.9.0 unified table (see settings.rs and
    // frontend/src/config/preset-defaults.json — all three must move together).
    let by_name = |name: &str| {
        presets
            .iter()
            .find(|p| p.name == name)
            .unwrap_or_else(|| panic!("missing preset {name}"))
    };
    let conservative = by_name("conservative");
    assert_eq!(conservative.image_quality, 72);
    assert_eq!(conservative.max_image_size_percent, 84);
    assert_eq!(conservative.max_image_size_px, 2700);
    assert!(!conservative.strip_metadata);
    assert!(!conservative.subset_fonts);

    let balanced = by_name("balanced");
    assert_eq!(balanced.image_quality, 60);
    assert_eq!(balanced.max_image_size_percent, 68);
    assert_eq!(balanced.max_image_size_px, 2200);
    assert!(balanced.strip_metadata);
    assert!(!balanced.subset_fonts);

    let maximum = by_name("maximum");
    assert_eq!(maximum.image_quality, 46);
    assert_eq!(maximum.max_image_size_percent, 52);
    assert_eq!(maximum.max_image_size_px, 1700);
    assert!(maximum.strip_metadata);
    assert!(maximum.subset_fonts);
}

#[test]
fn build_features_mirror_the_engine_flags() {
    let features = build_features();
    // The ffi crate pins the engine to default-off + ccitt + subset-fonts
    // (its dependency declaration), so the record must say exactly that —
    // the C decoder paths (jpx, cmyk-cms) are absent on mobile until the
    // phase-two feature work opts in.
    assert!(features.ccitt);
    assert!(features.subset_fonts);
    assert!(!features.jpx);
    assert!(!features.cmyk_cms);
}
