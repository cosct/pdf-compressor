//! Compression settings normalization — merges frontend payloads with backend defaults.
//! 压缩设置规范化 — 合并前端载荷与后端默认值。
//!
//! Resolves the preset, applies per-field overrides, and clamps all values
//! to valid ranges before passing them to the compression engine.
//! 解析预设，应用逐字段覆盖，在传递给压缩引擎前将所有值夹紧到有效范围内。

use crate::models::CompressionSettingsPayload;

#[derive(Debug, Clone, Copy)]
pub enum CompressionPreset {
    Conservative,
    Balanced,
    Maximum,
}

/// Reference edge for percent-based size caps when a document carries no
/// readable image dimensions (or none at all): the same fallback the GUI
/// panel uses (`DEFAULT_REFERENCE_IMAGE_EDGE_PX` in compressionSettings.ts).
pub const DEFAULT_REFERENCE_IMAGE_EDGE_PX: u32 = 3200;

/// Lower/upper clamp for the resolved absolute cap, in pixels.
const MIN_RESOLVED_EDGE_PX: u16 = 100;
const MAX_RESOLVED_EDGE_PX: u16 = 8_000;

/// Resolve a percent-of-reference-edge cap into an absolute pixel value,
/// rounded to hundreds — the exact formula the GUI panel uses
/// (`calculateMaxImageSizePx`), so a preset's effective numbers are identical
/// wherever they get resolved.
pub fn percent_to_px(percent: u16, reference_edge: u32) -> u16 {
    let calculated = f64::from(reference_edge) * f64::from(percent) / 100.0;
    let rounded = (calculated / 100.0).round() * 100.0;
    rounded.clamp(
        f64::from(MIN_RESOLVED_EDGE_PX),
        f64::from(MAX_RESOLVED_EDGE_PX),
    ) as u16
}

impl CompressionPreset {
    pub fn as_label(self) -> &'static str {
        match self {
            Self::Conservative => "conservative",
            Self::Balanced => "balanced",
            Self::Maximum => "maximum",
        }
    }

    pub fn from_optional_str(value: Option<&str>) -> Self {
        match value.map(|item| item.to_ascii_lowercase()) {
            Some(value) if value.contains("conservative") || value.contains("light") => {
                Self::Conservative
            }
            Some(value) if value.contains("maximum") || value.contains("aggressive") => {
                Self::Maximum
            }
            _ => Self::Balanced,
        }
    }

    /// The preset table — the single source of truth shared by the GUI panel,
    /// the right-click quick mode, and the CLI (0.9.0 unification). Values
    /// mirror `frontend/src/config/preset-defaults.json`; changing one side
    /// must change the other (and the user-guide table).
    pub fn default_quality(self) -> u8 {
        match self {
            Self::Conservative => 72,
            Self::Balanced => 60,
            Self::Maximum => 46,
        }
    }

    /// Size cap as a percentage of the document's largest image edge
    /// (adaptive: big scans keep more pixels, small images stay untouched).
    pub fn default_max_image_size_percent(self) -> u16 {
        match self {
            Self::Conservative => 84,
            Self::Balanced => 68,
            Self::Maximum => 52,
        }
    }

    /// Legacy absolute-cap view of the preset table, resolved against the
    /// default reference edge — for callers without document context (the
    /// analyzer's no-images fallback).
    pub fn default_max_image_size_px(self) -> u16 {
        percent_to_px(
            self.default_max_image_size_percent(),
            DEFAULT_REFERENCE_IMAGE_EDGE_PX,
        )
    }

    /// Metadata stripping per preset: conservative keeps document metadata
    /// (the safest posture), the others remove it.
    pub fn default_strip_metadata(self) -> bool {
        match self {
            Self::Conservative => false,
            Self::Balanced | Self::Maximum => true,
        }
    }

    /// Font subsetting per preset: only the most aggressive preset opts in
    /// (inert in builds without the `subset-fonts` feature).
    pub fn default_subset_fonts(self) -> bool {
        matches!(self, Self::Maximum)
    }
}

/// Output codec for images whose decoded plane is (near-)bilevel, i.e. scans
/// of text documents. Continuous-tone images always stay on JPEG.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BilevelCodec {
    /// Standard JPEG re-encode — today's behavior.
    #[default]
    Jpeg,
    /// CCITT Group 4 (ITU T.6): lossless bi-level coding, dramatically smaller
    /// than JPEG for black-and-white scans.
    CcittG4,
}

impl BilevelCodec {
    pub fn from_optional_str(value: Option<&str>) -> Self {
        match value.map(|item| item.to_ascii_lowercase()) {
            // "ccitt-g4" / "g4" / "bilevel" style labels all select G4.
            Some(value) if value.contains("g4") || value.contains("ccitt") => Self::CcittG4,
            _ => Self::Jpeg,
        }
    }

    pub fn as_label(self) -> &'static str {
        match self {
            Self::Jpeg => "jpeg",
            Self::CcittG4 => "ccitt-g4",
        }
    }

    pub fn uses_ccitt(self) -> bool {
        matches!(self, Self::CcittG4)
    }
}

/// Normalized backend settings used by the compression service.
#[derive(Debug, Clone)]
pub struct CompressionSettings {
    pub preset: CompressionPreset,
    pub image_quality: u8,
    /// Absolute maximum image edge in pixels. When [`Self::max_image_size_percent`]
    /// is set this holds a provisional value (percent against the default
    /// reference edge) that the compression entry points re-resolve against
    /// the document's own largest image edge once loaded.
    pub max_image_size_px: u16,
    /// Percent-of-document-max-image-edge cap. `None` when an absolute pixel
    /// value was supplied explicitly (`--max-edge`, a saved quick profile
    /// with pixels); `Some` otherwise, including the preset-table default.
    pub max_image_size_percent: Option<u16>,
    pub optimize_images: bool,
    pub compress_streams: bool,
    pub strip_metadata: bool,
    /// Re-encode color images as 8-bit grayscale (roughly halves image bytes
    /// for black-and-white scans). No effect on images that are already gray.
    pub grayscale: bool,
    /// Output codec for near-bilevel planes (see `BilevelCodec`).
    pub bilevel_codec: BilevelCodec,
    /// Shrink embedded Type0/CIDFontType2 TrueType fonts to the used glyphs
    /// (opt-in; `subset-fonts` feature). Non-eligible fonts are untouched.
    pub subset_fonts: bool,
    /// Convert CMYK images (ICC N=4, `DeviceCMYK`, CMYK-based Indexed, CMYK
    /// JPEG, 4-component JPX) to RGB for re-encoding. **Default on since
    /// 0.8.0**: release artifacts have shipped the calibrated `cmyk-cms`
    /// conversion since 0.7.0, so the opt-in guard has served its purpose.
    /// Builds without the feature refuse the color conversion at
    /// [`Self::converts_cmyk`] regardless of this setting — their naive
    /// ink-subtraction formula drifts ≈7.6 dB off color-managed renderers,
    /// and an untouched image beats a wrong-colored one. Grayscale/G4
    /// requests imply the conversion in every build — any color-to-luma
    /// collapse is already lossy by intent and the CMYK→gray deviation is
    /// far below the color one.
    pub cmyk_conversion: bool,
    pub output_dir: Option<String>,
}

#[derive(Debug, Default)]
pub struct CompressionSettingsOverrides {
    pub preset: Option<String>,
    pub image_quality: Option<u8>,
    pub max_image_size_px: Option<u16>,
    pub max_image_size_percent: Option<u16>,
    pub optimize_images: Option<bool>,
    pub compress_streams: Option<bool>,
    pub strip_metadata: Option<bool>,
    pub grayscale: Option<bool>,
    pub bilevel_codec: Option<BilevelCodec>,
    pub subset_fonts: Option<bool>,
    pub cmyk_conversion: Option<bool>,
    pub output_dir: Option<String>,
}

impl CompressionSettingsOverrides {
    /// Field-wise merge: `self` wins wherever set, `fallback` fills the rest
    /// (explicit CLI flags over the persisted quick profile, for instance).
    pub fn or_else(self, fallback: Self) -> Self {
        Self {
            preset: self.preset.or(fallback.preset),
            image_quality: self.image_quality.or(fallback.image_quality),
            max_image_size_px: self.max_image_size_px.or(fallback.max_image_size_px),
            max_image_size_percent: self
                .max_image_size_percent
                .or(fallback.max_image_size_percent),
            optimize_images: self.optimize_images.or(fallback.optimize_images),
            compress_streams: self.compress_streams.or(fallback.compress_streams),
            strip_metadata: self.strip_metadata.or(fallback.strip_metadata),
            grayscale: self.grayscale.or(fallback.grayscale),
            bilevel_codec: self.bilevel_codec.or(fallback.bilevel_codec),
            subset_fonts: self.subset_fonts.or(fallback.subset_fonts),
            cmyk_conversion: self.cmyk_conversion.or(fallback.cmyk_conversion),
            output_dir: self.output_dir.or(fallback.output_dir),
        }
    }
}

impl CompressionSettings {
    pub fn from_sources(
        payload: Option<CompressionSettingsPayload>,
        overrides: CompressionSettingsOverrides,
    ) -> Self {
        let payload_preset = payload.as_ref().and_then(|value| value.preset.as_deref());
        let resolved_preset =
            CompressionPreset::from_optional_str(overrides.preset.as_deref().or(payload_preset));

        let payload_quality = payload.as_ref().and_then(|value| value.image_quality);
        let payload_max_image_size = payload.as_ref().and_then(|value| value.max_image_size_px);
        let payload_max_image_size_percent = payload
            .as_ref()
            .and_then(|value| value.max_image_size_percent);
        let payload_optimize_images = payload.as_ref().and_then(|value| value.optimize_images);
        let payload_compress_streams = payload.as_ref().and_then(|value| value.compress_streams);
        let payload_strip_metadata = payload.as_ref().and_then(|value| value.strip_metadata);
        let payload_grayscale = payload.as_ref().and_then(|value| value.grayscale);
        let payload_subset_fonts = payload.as_ref().and_then(|value| value.subset_fonts);
        let payload_cmyk_conversion = payload.as_ref().and_then(|value| value.cmyk_conversion);
        let payload_bilevel_codec = payload
            .as_ref()
            .and_then(|value| value.bilevel_codec.as_deref())
            .map(|value| BilevelCodec::from_optional_str(Some(value)));
        let payload_output_dir = payload.and_then(|value| value.output_dir);

        // Size cap: an explicit absolute pixel value wins outright (CLI
        // `--max-edge`, saved quick profiles with pixels); otherwise the
        // percent semantics apply — an explicit percent, else the preset
        // table's. Percent mode keeps a provisional px against the default
        // reference edge; the compression entry points re-resolve it against
        // the loaded document's own largest image edge (the GUI's adaptive
        // design, now shared by every entry point).
        let explicit_max_image_size_px = overrides
            .max_image_size_px
            .or(payload_max_image_size)
            .map(|value| value.clamp(100, 8_000));
        let (max_image_size_px, max_image_size_percent) = match explicit_max_image_size_px {
            Some(px) => (px, None),
            None => {
                let percent = overrides
                    .max_image_size_percent
                    .or(payload_max_image_size_percent)
                    .unwrap_or_else(|| resolved_preset.default_max_image_size_percent())
                    .clamp(5, 100);
                (
                    percent_to_px(percent, DEFAULT_REFERENCE_IMAGE_EDGE_PX),
                    Some(percent),
                )
            }
        };

        Self {
            preset: resolved_preset,
            image_quality: overrides
                .image_quality
                .or(payload_quality)
                .unwrap_or_else(|| resolved_preset.default_quality())
                .clamp(10, 100),
            max_image_size_px,
            max_image_size_percent,
            optimize_images: overrides
                .optimize_images
                .or(payload_optimize_images)
                .unwrap_or(true),
            compress_streams: overrides
                .compress_streams
                .or(payload_compress_streams)
                .unwrap_or(true),
            strip_metadata: overrides
                .strip_metadata
                .or(payload_strip_metadata)
                .unwrap_or_else(|| resolved_preset.default_strip_metadata()),
            grayscale: overrides.grayscale.or(payload_grayscale).unwrap_or(false),
            bilevel_codec: overrides
                .bilevel_codec
                .or(payload_bilevel_codec)
                .unwrap_or_default(),
            subset_fonts: overrides
                .subset_fonts
                .or(payload_subset_fonts)
                .unwrap_or_else(|| resolved_preset.default_subset_fonts()),
            cmyk_conversion: overrides
                .cmyk_conversion
                .or(payload_cmyk_conversion)
                .unwrap_or(true),
            output_dir: overrides.output_dir.or(payload_output_dir),
        }
    }

    /// Re-resolve a percent-based cap against the document's largest image
    /// edge (called by the compression entry points right after loading, so
    /// big scans keep more pixels while small images stay untouched). No-op
    /// when an absolute pixel value was supplied explicitly.
    pub fn resolve_max_image_size_px(&mut self, reference_edge: Option<u32>) {
        let Some(percent) = self.max_image_size_percent else {
            return;
        };
        let reference = reference_edge.unwrap_or(DEFAULT_REFERENCE_IMAGE_EDGE_PX);
        self.max_image_size_px = percent_to_px(percent, reference);
    }

    /// Whether CMYK images may be converted for re-encoding under these
    /// settings: the explicit setting (on by default since 0.8.0), or an
    /// implied conversion request (grayscale / G4 bilevel output collapses
    /// color by design).
    ///
    /// Builds without the `cmyk-cms` feature refuse the color conversion
    /// even when the setting asks for it: their naive ink-subtraction
    /// formula deviates ≈7.6 dB from color-managed renderers (poppler
    /// baseline, 2026-09), so CMYK images stay untouched rather than being
    /// re-encoded wrong. Only the luma-collapse intents convert there —
    /// the CMYK→gray deviation is far below the intent's own loss.
    pub fn converts_cmyk(&self) -> bool {
        let collapse_intent = self.grayscale || self.bilevel_codec.uses_ccitt();
        // Flat form (no cfg! branch): every operator stays observable in
        // either build, so mutation testing in the default-feature leg can
        // reach both the refusal and the intent paths.
        let color_conversion_allowed = cfg!(feature = "cmyk-cms") && self.cmyk_conversion;
        color_conversion_allowed || collapse_intent
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload(
        preset: Option<&str>,
        quality: Option<u8>,
        max_px: Option<u16>,
    ) -> Option<CompressionSettingsPayload> {
        Some(CompressionSettingsPayload {
            preset: preset.map(str::to_string),
            image_quality: quality,
            max_image_size_px: max_px,
            max_image_size_percent: None,
            optimize_images: None,
            compress_streams: None,
            strip_metadata: None,
            grayscale: None,
            bilevel_codec: None,
            subset_fonts: None,
            cmyk_conversion: None,
            output_dir: None,
        })
    }

    #[test]
    fn preset_parsing_matches_known_labels() {
        assert!(matches!(
            CompressionPreset::from_optional_str(Some("Maximum")),
            CompressionPreset::Maximum
        ));
        assert!(matches!(
            CompressionPreset::from_optional_str(Some("aggressive-v2")),
            CompressionPreset::Maximum
        ));
        assert!(matches!(
            CompressionPreset::from_optional_str(Some("light")),
            CompressionPreset::Conservative
        ));
        assert!(matches!(
            CompressionPreset::from_optional_str(Some("unknown-preset")),
            CompressionPreset::Balanced
        ));
        assert!(matches!(
            CompressionPreset::from_optional_str(None),
            CompressionPreset::Balanced
        ));
    }

    #[test]
    fn from_sources_applies_preset_defaults() {
        let settings =
            CompressionSettings::from_sources(None, CompressionSettingsOverrides::default());
        assert_eq!(settings.preset.as_label(), "balanced");
        assert_eq!(
            settings.image_quality,
            CompressionPreset::Balanced.default_quality()
        );
        assert_eq!(
            settings.max_image_size_px,
            CompressionPreset::Balanced.default_max_image_size_px()
        );
        assert!(settings.optimize_images);
        assert!(settings.compress_streams);
        assert!(settings.strip_metadata);
        assert!(settings.output_dir.is_none());
    }

    #[test]
    fn preset_table_is_the_single_source_of_truth() {
        // 0.9.0 unification: quality/percent/booleans must mirror
        // frontend/src/config/preset-defaults.json exactly — same preset,
        // same effective parameters in the GUI, quick mode, and the CLI.
        let table = [
            (CompressionPreset::Conservative, 72, 84, false, false),
            (CompressionPreset::Balanced, 60, 68, true, false),
            (CompressionPreset::Maximum, 46, 52, true, true),
        ];
        for (preset, quality, percent, strip, subset) in table {
            assert_eq!(preset.default_quality(), quality, "{preset:?}");
            assert_eq!(
                preset.default_max_image_size_percent(),
                percent,
                "{preset:?}"
            );
            assert_eq!(preset.default_strip_metadata(), strip, "{preset:?}");
            assert_eq!(preset.default_subset_fonts(), subset, "{preset:?}");
        }
        // The provisional absolute view resolves against the default
        // reference edge, rounded to hundreds.
        assert_eq!(
            CompressionPreset::Conservative.default_max_image_size_px(),
            2700
        );
        assert_eq!(
            CompressionPreset::Balanced.default_max_image_size_px(),
            2200
        );
        assert_eq!(CompressionPreset::Maximum.default_max_image_size_px(), 1700);
    }

    #[test]
    fn percent_to_px_mirrors_the_gui_formula() {
        // round(edge × percent / 100 / 100) × 100, clamped [100, 8000].
        assert_eq!(percent_to_px(68, 3200), 2200);
        assert_eq!(percent_to_px(84, 3200), 2700);
        assert_eq!(percent_to_px(52, 2000), 1000);
        assert_eq!(percent_to_px(52, 2400), 1200);
        // Rounding to hundreds, never fractional edges.
        assert_eq!(percent_to_px(50, 1234), 600);
        // Clamped at both ends.
        assert_eq!(percent_to_px(1, 100), 100);
        assert_eq!(percent_to_px(100, 100_000), 8_000);
    }

    #[test]
    fn from_sources_resolves_percent_and_absolute_caps() {
        // Default: the preset table's percent, with a provisional px.
        let settings =
            CompressionSettings::from_sources(None, CompressionSettingsOverrides::default());
        assert_eq!(settings.max_image_size_percent, Some(68));
        assert_eq!(settings.max_image_size_px, 2200);

        // An explicit percent (override or payload) replaces the table's.
        let explicit = CompressionSettings::from_sources(
            None,
            CompressionSettingsOverrides {
                max_image_size_percent: Some(75),
                ..Default::default()
            },
        );
        assert_eq!(explicit.max_image_size_percent, Some(75));
        assert_eq!(explicit.max_image_size_px, 2400);

        // An absolute pixel value wins outright and drops percent mode —
        // `--max-edge` stays an absolute override in every entry point.
        let absolute = CompressionSettings::from_sources(
            Some(CompressionSettingsPayload {
                max_image_size_percent: Some(75),
                max_image_size_px: Some(1500),
                ..Default::default()
            }),
            CompressionSettingsOverrides {
                max_image_size_px: Some(1800),
                ..Default::default()
            },
        );
        assert_eq!(absolute.max_image_size_percent, None);
        assert_eq!(absolute.max_image_size_px, 1800);
    }

    #[test]
    fn resolve_max_image_size_px_adapts_to_the_document_edge() {
        let mut settings =
            CompressionSettings::from_sources(None, CompressionSettingsOverrides::default());
        settings.resolve_max_image_size_px(Some(2000));
        assert_eq!(settings.max_image_size_px, 1400, "68% of 2000 px");

        settings.resolve_max_image_size_px(Some(600));
        assert_eq!(settings.max_image_size_px, 400, "68% of 600 px");

        // No readable dimensions: the default reference edge.
        settings.resolve_max_image_size_px(None);
        assert_eq!(settings.max_image_size_px, 2200);

        // Absolute mode is untouched by resolution.
        let mut absolute = CompressionSettings::from_sources(
            None,
            CompressionSettingsOverrides {
                max_image_size_px: Some(1800),
                ..Default::default()
            },
        );
        absolute.resolve_max_image_size_px(Some(2000));
        assert_eq!(absolute.max_image_size_px, 1800);
    }

    #[test]
    fn from_sources_applies_preset_boolean_defaults() {
        let conservative = CompressionSettings::from_sources(
            None,
            CompressionSettingsOverrides {
                preset: Some("conservative".to_string()),
                ..Default::default()
            },
        );
        assert!(!conservative.strip_metadata, "conservative keeps metadata");
        assert!(!conservative.subset_fonts);

        let maximum = CompressionSettings::from_sources(
            None,
            CompressionSettingsOverrides {
                preset: Some("maximum".to_string()),
                ..Default::default()
            },
        );
        assert!(maximum.strip_metadata);
        assert!(maximum.subset_fonts, "maximum subsets fonts by default");

        // Explicit values override the table in both directions.
        let opted_out = CompressionSettings::from_sources(
            None,
            CompressionSettingsOverrides {
                preset: Some("maximum".to_string()),
                subset_fonts: Some(false),
                ..Default::default()
            },
        );
        assert!(!opted_out.subset_fonts);
    }

    #[test]
    fn from_sources_prefers_overrides_over_payload() {
        let settings = CompressionSettings::from_sources(
            payload(Some("conservative"), Some(60), Some(1200)),
            CompressionSettingsOverrides {
                image_quality: Some(75),
                ..Default::default()
            },
        );
        assert_eq!(settings.preset.as_label(), "conservative");
        assert_eq!(settings.image_quality, 75);
        assert_eq!(settings.max_image_size_px, 1200);
    }

    #[test]
    fn from_sources_clamps_out_of_range_values() {
        let low = CompressionSettings::from_sources(
            payload(None, Some(5), Some(50)),
            CompressionSettingsOverrides::default(),
        );
        assert_eq!(low.image_quality, 10);
        assert_eq!(low.max_image_size_px, 100);

        let high = CompressionSettings::from_sources(
            payload(None, Some(200), Some(65_000)),
            CompressionSettingsOverrides::default(),
        );
        assert_eq!(high.image_quality, 100);
        assert_eq!(high.max_image_size_px, 8_000);
    }

    #[test]
    fn from_sources_applies_grayscale_flag() {
        let grayscale = CompressionSettings::from_sources(
            None,
            CompressionSettingsOverrides {
                grayscale: Some(true),
                ..Default::default()
            },
        );
        assert!(grayscale.grayscale);

        let default =
            CompressionSettings::from_sources(None, CompressionSettingsOverrides::default());
        assert!(!default.grayscale);
    }

    #[test]
    fn cmyk_conversion_defaults_on_and_honors_opt_out() {
        let default =
            CompressionSettings::from_sources(None, CompressionSettingsOverrides::default());
        assert!(default.cmyk_conversion);

        let opted_out = CompressionSettings::from_sources(
            Some(CompressionSettingsPayload {
                cmyk_conversion: Some(false),
                ..Default::default()
            }),
            CompressionSettingsOverrides::default(),
        );
        assert!(!opted_out.cmyk_conversion);
    }

    #[test]
    fn converts_cmyk_follows_the_build_and_the_intent() {
        let settings = |grayscale: bool, g4: bool, cmyk: Option<bool>| {
            CompressionSettings::from_sources(
                None,
                CompressionSettingsOverrides {
                    grayscale: Some(grayscale),
                    bilevel_codec: Some(if g4 {
                        BilevelCodec::CcittG4
                    } else {
                        BilevelCodec::Jpeg
                    }),
                    cmyk_conversion: cmyk,
                    ..Default::default()
                },
            )
        };

        // Full matrix of (grayscale, g4, cmyk setting) x build. The
        // luma-collapse intents imply the conversion in every build; only
        // the pure color intent follows the build's capability.
        for (grayscale, g4) in [(false, false), (true, false), (false, true), (true, true)] {
            for cmyk in [None, Some(true), Some(false)] {
                let converts = settings(grayscale, g4, cmyk).converts_cmyk();
                if grayscale || g4 {
                    assert!(
                        converts,
                        "collapse intent must convert regardless of build/setting                          (grayscale={grayscale}, g4={g4}, cmyk={cmyk:?})"
                    );
                } else if cfg!(feature = "cmyk-cms") {
                    assert_eq!(
                        converts,
                        cmyk.unwrap_or(true),
                        "cms builds follow the setting/default (cmyk={cmyk:?})"
                    );
                } else {
                    assert!(
                        !converts,
                        "feature-off builds refuse the color path (cmyk={cmyk:?})"
                    );
                }
            }
        }
    }

    #[test]
    fn from_sources_applies_bilevel_codec() {
        let g4 = CompressionSettings::from_sources(
            None,
            CompressionSettingsOverrides {
                bilevel_codec: Some(BilevelCodec::CcittG4),
                ..Default::default()
            },
        );
        assert!(g4.bilevel_codec.uses_ccitt());

        let via_payload = CompressionSettings::from_sources(
            Some(CompressionSettingsPayload {
                bilevel_codec: Some("ccitt-g4".into()),
                ..Default::default()
            }),
            CompressionSettingsOverrides::default(),
        );
        assert!(via_payload.bilevel_codec.uses_ccitt());

        let default =
            CompressionSettings::from_sources(None, CompressionSettingsOverrides::default());
        assert_eq!(default.bilevel_codec.as_label(), "jpeg");
    }

    #[test]
    fn bilevel_codec_parsing_matches_known_labels() {
        assert!(BilevelCodec::from_optional_str(Some("ccitt-g4")).uses_ccitt());
        assert!(BilevelCodec::from_optional_str(Some("G4")).uses_ccitt());
        assert!(BilevelCodec::from_optional_str(Some("CCITT")).uses_ccitt());
        assert!(!BilevelCodec::from_optional_str(Some("jpeg")).uses_ccitt());
        assert!(!BilevelCodec::from_optional_str(None).uses_ccitt());
    }
}
