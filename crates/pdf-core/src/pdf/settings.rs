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
    pub output_dir: Option<String>,
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
            output_dir: overrides.output_dir.or(payload_output_dir),
        }
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
