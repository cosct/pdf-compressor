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

    pub fn default_quality(self) -> u8 {
        match self {
            Self::Conservative => 82,
            Self::Balanced => 72,
            Self::Maximum => 58,
        }
    }

    pub fn default_max_image_size_px(self) -> u16 {
        match self {
            Self::Conservative => 2400,
            Self::Balanced => 1800,
            Self::Maximum => 1400,
        }
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
    pub max_image_size_px: u16,
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

        Self {
            preset: resolved_preset,
            image_quality: overrides
                .image_quality
                .or(payload_quality)
                .unwrap_or(resolved_preset.default_quality())
                .clamp(10, 100),
            max_image_size_px: overrides
                .max_image_size_px
                .or(payload_max_image_size)
                .unwrap_or(resolved_preset.default_max_image_size_px())
                .clamp(100, 8_000),
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
                .unwrap_or(true),
            grayscale: overrides.grayscale.or(payload_grayscale).unwrap_or(false),
            bilevel_codec: overrides
                .bilevel_codec
                .or(payload_bilevel_codec)
                .unwrap_or_default(),
            subset_fonts: overrides
                .subset_fonts
                .or(payload_subset_fonts)
                .unwrap_or(false),
            cmyk_conversion: overrides
                .cmyk_conversion
                .or(payload_cmyk_conversion)
                .unwrap_or(true),
            output_dir: overrides.output_dir.or(payload_output_dir),
        }
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
