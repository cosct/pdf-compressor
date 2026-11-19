//! UI-facing settings: includes controls that are not engine fields (the
//! preview reference edge and optional target-size budget), and owns the
//! frontend's normalization rules before converting to pdf-core.

use serde::{Deserialize, Deserializer, Serialize, Serializer, ser::SerializeStruct};

use pdf_core::{
    BilevelCodec, CompressionSettings, CompressionSettingsOverrides,
    models::CompressionSettingsPayload,
};

pub const MIN_IMAGE_QUALITY: u8 = 1;
pub const MAX_IMAGE_QUALITY: u8 = 100;
pub const MIN_IMAGE_SIZE_PERCENT: u16 = 5;
pub const MAX_IMAGE_SIZE_PERCENT: u16 = 100;
pub const MIN_IMAGE_SIZE_PX: u32 = 100;
pub const MAX_IMAGE_SIZE_PX: u32 = 20_000;
pub const DEFAULT_REFERENCE_IMAGE_EDGE_PX: u32 = 3_200;
pub const MIN_TARGET_SIZE_MB: f64 = 0.1;
pub const MAX_TARGET_SIZE_MB: f64 = 2_048.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub enum Preset {
    Conservative,
    #[default]
    Balanced,
    Maximum,
    Custom,
}

impl Preset {
    pub fn parse(value: &str) -> Self {
        match value {
            "conservative" => Self::Conservative,
            "maximum" => Self::Maximum,
            "custom" => Self::Custom,
            _ => Self::Balanced,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Conservative => "conservative",
            Self::Balanced => "balanced",
            Self::Maximum => "maximum",
            Self::Custom => "custom",
        }
    }

    pub fn engine_preset(self) -> &'static str {
        match self {
            Self::Conservative => "conservative",
            Self::Maximum | Self::Custom => "maximum",
            Self::Balanced => "balanced",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    pub preset: Preset,
    pub image_quality: u8,
    pub max_image_size_percent: u16,
    pub reference_max_image_edge_px: Option<u32>,
    pub optimize_images: bool,
    pub compress_streams: bool,
    pub strip_metadata: bool,
    pub grayscale: bool,
    pub bilevel_codec: BilevelCodec,
    pub subset_fonts: bool,
    pub cmyk_conversion: bool,
    pub output_dir: Option<String>,
    pub target_file_size_mb: Option<f64>,
}

impl Default for Settings {
    fn default() -> Self {
        Self::for_preset(Preset::Balanced)
    }
}

impl Settings {
    pub fn for_preset(preset: Preset) -> Self {
        let (image_quality, max_image_size_percent, strip_metadata, subset_fonts) = match preset {
            Preset::Conservative => (72, 84, false, false),
            Preset::Balanced => (60, 68, true, false),
            Preset::Maximum | Preset::Custom => (46, 52, true, true),
        };
        Self {
            preset,
            image_quality,
            max_image_size_percent,
            reference_max_image_edge_px: None,
            optimize_images: true,
            compress_streams: true,
            strip_metadata,
            grayscale: false,
            bilevel_codec: BilevelCodec::CcittG4,
            subset_fonts,
            cmyk_conversion: true,
            output_dir: None,
            target_file_size_mb: None,
        }
    }

    /// Apply the UI's clamp/default rules. The engine independently applies
    /// its narrower safe range (quality >= 10, edge <= 8000) when dispatching.
    pub fn normalize(self) -> Self {
        let output_dir = self
            .output_dir
            .as_deref()
            .map(str::trim)
            .filter(|dir| !dir.is_empty())
            .map(str::to_owned);
        let target_file_size_mb = self.target_file_size_mb.and_then(|value| {
            (value.is_finite() && value > 0.0)
                .then_some(value.clamp(MIN_TARGET_SIZE_MB, MAX_TARGET_SIZE_MB))
        });
        let image_quality = self
            .image_quality
            .clamp(MIN_IMAGE_QUALITY, MAX_IMAGE_QUALITY);
        let max_image_size_percent = self
            .max_image_size_percent
            .clamp(MIN_IMAGE_SIZE_PERCENT, MAX_IMAGE_SIZE_PERCENT);
        Self {
            preset: self.preset,
            image_quality,
            max_image_size_percent,
            reference_max_image_edge_px: self.reference_max_image_edge_px.filter(|edge| *edge > 0),
            optimize_images: self.optimize_images,
            compress_streams: self.compress_streams,
            strip_metadata: self.strip_metadata,
            grayscale: self.grayscale,
            bilevel_codec: self.bilevel_codec,
            subset_fonts: self.subset_fonts,
            cmyk_conversion: self.cmyk_conversion,
            output_dir,
            target_file_size_mb,
        }
    }

    /// Match the frontend's recommendation comparison: preset label, preview
    /// reference edge and output directory do not count; target size does.
    pub fn comparable_eq(&self, other: &Self) -> bool {
        let left = self.clone().normalize();
        let right = other.clone().normalize();
        left.image_quality == right.image_quality
            && left.max_image_size_percent == right.max_image_size_percent
            && left.optimize_images == right.optimize_images
            && left.compress_streams == right.compress_streams
            && left.strip_metadata == right.strip_metadata
            && left.grayscale == right.grayscale
            && left.bilevel_codec == right.bilevel_codec
            && left.subset_fonts == right.subset_fonts
            && left.cmyk_conversion == right.cmyk_conversion
            && left.target_file_size_mb == right.target_file_size_mb
    }

    pub fn recommended_for_analysis(&self, preset_name: &str, max_edge: u16) -> Self {
        let preset = Preset::parse(preset_name);
        let mut recommended = Self::for_preset(preset);
        recommended.reference_max_image_edge_px = (max_edge > 0).then_some(u32::from(max_edge));
        recommended.output_dir = self.output_dir.clone();
        recommended.target_file_size_mb = self.target_file_size_mb;
        recommended.normalize()
    }

    pub fn max_image_size_px_preview(&self) -> u32 {
        let base = self
            .reference_max_image_edge_px
            .filter(|edge| *edge > 0)
            .unwrap_or(DEFAULT_REFERENCE_IMAGE_EDGE_PX);
        let value = ((f64::from(base) * f64::from(self.max_image_size_percent) / 100.0 / 100.0)
            .round() as u32)
            * 100;
        value.clamp(MIN_IMAGE_SIZE_PX, MAX_IMAGE_SIZE_PX)
    }

    pub fn to_engine(&self, scanned: bool) -> CompressionSettings {
        let mut overrides = CompressionSettingsOverrides {
            preset: Some(self.preset.engine_preset().to_owned()),
            grayscale: Some(self.grayscale),
            bilevel_codec: Some(self.bilevel_codec),
            ..Default::default()
        };
        if scanned {
            overrides = overrides.scanned_pipeline();
        }
        CompressionSettings::from_sources(Some(self.to_engine_payload()), overrides)
    }

    pub fn to_engine_payload(&self) -> CompressionSettingsPayload {
        CompressionSettingsPayload {
            preset: Some(self.preset.engine_preset().to_owned()),
            image_quality: Some(self.image_quality),
            max_image_size_px: None,
            max_image_size_percent: Some(self.max_image_size_percent),
            optimize_images: Some(self.optimize_images),
            compress_streams: Some(self.compress_streams),
            strip_metadata: Some(self.strip_metadata),
            grayscale: Some(self.grayscale),
            bilevel_codec: Some(self.bilevel_codec.as_label().to_owned()),
            subset_fonts: Some(self.subset_fonts),
            cmyk_conversion: Some(self.cmyk_conversion),
            output_dir: self.output_dir.clone(),
        }
    }

    pub fn target_size_bytes(&self) -> Option<u64> {
        self.target_file_size_mb
            .filter(|value| value.is_finite() && *value > 0.0)
            .map(|mb| (mb * 1024.0 * 1024.0).round().max(1.0) as u64)
    }

    fn from_json_value(value: &serde_json::Value) -> Self {
        let mut settings = Self::default();
        let Some(object) = value.as_object() else {
            return settings;
        };
        if let Some(value) = object.get("preset").and_then(serde_json::Value::as_str) {
            settings.preset = Preset::parse(value);
        }
        let defaults = Self::for_preset(settings.preset);
        settings.image_quality = object
            .get("imageQuality")
            .and_then(serde_json::Value::as_f64)
            .filter(|value| value.is_finite())
            .map(|value| value.round().clamp(1.0, 100.0) as u8)
            .unwrap_or(defaults.image_quality);
        settings.max_image_size_percent = object
            .get("maxImageSizePercent")
            .and_then(serde_json::Value::as_f64)
            .filter(|value| value.is_finite())
            .map(|value| value.round().clamp(5.0, 100.0) as u16)
            .unwrap_or(defaults.max_image_size_percent);
        settings.reference_max_image_edge_px = object
            .get("referenceMaxImageEdgePx")
            .and_then(serde_json::Value::as_f64)
            .filter(|value| value.is_finite() && *value > 0.0)
            .map(|value| value.round().clamp(1.0, f64::from(u32::MAX)) as u32);
        settings.optimize_images = object
            .get("optimizeImages")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(true);
        settings.compress_streams = object
            .get("compressStreams")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(true);
        settings.strip_metadata = object
            .get("stripMetadata")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(true);
        settings.grayscale = object
            .get("grayscale")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        settings.bilevel_codec = match object
            .get("bilevelCodec")
            .and_then(serde_json::Value::as_str)
        {
            Some("jpeg") => BilevelCodec::Jpeg,
            _ => BilevelCodec::CcittG4,
        };
        settings.subset_fonts = object
            .get("subsetFonts")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        settings.cmyk_conversion = object
            .get("cmykConversion")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(true);
        settings.output_dir = object
            .get("outputDir")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned);
        settings.target_file_size_mb = object
            .get("targetFileSizeMb")
            .and_then(serde_json::Value::as_f64)
            .filter(|value| value.is_finite() && *value > 0.0)
            .map(|value| value.clamp(MIN_TARGET_SIZE_MB, MAX_TARGET_SIZE_MB));
        settings
    }
}

impl Serialize for Settings {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("CompressionSettings", 13)?;
        state.serialize_field("preset", self.preset.as_str())?;
        state.serialize_field("imageQuality", &self.image_quality)?;
        state.serialize_field("maxImageSizePercent", &self.max_image_size_percent)?;
        state.serialize_field("referenceMaxImageEdgePx", &self.reference_max_image_edge_px)?;
        state.serialize_field("optimizeImages", &self.optimize_images)?;
        state.serialize_field("compressStreams", &self.compress_streams)?;
        state.serialize_field("stripMetadata", &self.strip_metadata)?;
        state.serialize_field("grayscale", &self.grayscale)?;
        state.serialize_field("bilevelCodec", &self.bilevel_codec.as_label())?;
        state.serialize_field("subsetFonts", &self.subset_fonts)?;
        state.serialize_field("cmykConversion", &self.cmyk_conversion)?;
        state.serialize_field("outputDir", &self.output_dir)?;
        state.serialize_field("targetFileSizeMb", &self.target_file_size_mb)?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for Settings {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        Ok(Self::from_json_value(&value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_balanced_frontend_profile() {
        let settings = Settings::default();
        assert_eq!(settings.image_quality, 60);
        assert_eq!(settings.max_image_size_percent, 68);
        assert!(settings.optimize_images && settings.compress_streams);
        assert!(settings.strip_metadata && settings.cmyk_conversion);
        assert_eq!(settings.bilevel_codec, BilevelCodec::CcittG4);
    }

    #[test]
    fn malformed_saved_values_fall_back_and_normalize() {
        let settings: Settings = serde_json::from_str(
            r#"{"preset":"nonsense","imageQuality":null,"maxImageSizePercent":2,"outputDir":"  ","targetFileSizeMb":9999,"bilevelCodec":"bad"}"#,
        )
        .unwrap();
        let settings = settings.normalize();
        assert_eq!(settings.preset, Preset::Balanced);
        assert_eq!(settings.image_quality, 60);
        assert_eq!(settings.max_image_size_percent, 5);
        assert_eq!(settings.output_dir, None);
        assert_eq!(settings.target_file_size_mb, Some(MAX_TARGET_SIZE_MB));
        assert_eq!(settings.bilevel_codec, BilevelCodec::CcittG4);
    }

    #[test]
    fn settings_map_to_engine_and_scanned_policy_forces_only_two_fields() {
        let settings = Settings {
            optimize_images: false,
            compress_streams: false,
            grayscale: true,
            output_dir: Some("/tmp/out".to_owned()),
            target_file_size_mb: Some(5.0),
            ..Settings::default()
        };
        let standard = settings.to_engine(false);
        assert!(!standard.optimize_images && !standard.compress_streams);
        let scanned = settings.to_engine(true);
        assert!(scanned.optimize_images && scanned.compress_streams);
        assert!(scanned.grayscale);
        assert_eq!(scanned.output_dir.as_deref(), Some("/tmp/out"));
        assert_eq!(settings.target_size_bytes(), Some(5 * 1024 * 1024));
    }

    #[test]
    fn target_size_and_reference_preview_follow_ui_rounding() {
        let settings = Settings {
            reference_max_image_edge_px: Some(2400),
            max_image_size_percent: 68,
            target_file_size_mb: Some(0.1),
            ..Settings::default()
        };
        assert_eq!(settings.max_image_size_px_preview(), 1600);
        assert_eq!(settings.target_size_bytes(), Some(104_858));
    }
}
