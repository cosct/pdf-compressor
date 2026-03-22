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

    fn default_quality(self) -> u8 {
        match self {
            Self::Conservative => 82,
            Self::Balanced => 72,
            Self::Maximum => 58,
        }
    }

    fn default_dpi(self) -> u16 {
        match self {
            Self::Conservative => 2400,
            Self::Balanced => 1800,
            Self::Maximum => 1400,
        }
    }
}

/// Normalized backend settings used by the compression service.
#[derive(Debug, Clone, Copy)]
pub struct CompressionSettings {
    pub preset: CompressionPreset,
    pub image_quality: u8,
    pub max_image_size_px: u16,
    pub optimize_images: bool,
    pub compress_streams: bool,
    pub strip_metadata: bool,
}

impl CompressionSettings {
    pub fn from_sources(
        payload: Option<CompressionSettingsPayload>,
        preset: Option<String>,
        image_quality: Option<u8>,
        max_image_size_px: Option<u16>,
        optimize_images: Option<bool>,
        compress_streams: Option<bool>,
        strip_metadata: Option<bool>,
    ) -> Self {
        let payload_preset = payload.as_ref().and_then(|value| value.preset.as_deref());
        let resolved_preset =
            CompressionPreset::from_optional_str(preset.as_deref().or(payload_preset));

        let payload_quality = payload.as_ref().and_then(|value| value.image_quality);
        let payload_max_image_size = payload.as_ref().and_then(|value| value.max_image_size_px);
        let payload_optimize_images = payload.as_ref().and_then(|value| value.optimize_images);
        let payload_compress_streams = payload.as_ref().and_then(|value| value.compress_streams);
        let payload_strip_metadata = payload.as_ref().and_then(|value| value.strip_metadata);

        Self {
            preset: resolved_preset,
            image_quality: image_quality
                .or(payload_quality)
                .unwrap_or(resolved_preset.default_quality())
                .clamp(35, 90),
            max_image_size_px: max_image_size_px
                .or(payload_max_image_size)
                .unwrap_or(resolved_preset.default_dpi())
                .clamp(800, 3200),
            optimize_images: optimize_images.or(payload_optimize_images).unwrap_or(true),
            compress_streams: compress_streams
                .or(payload_compress_streams)
                .unwrap_or(true),
            strip_metadata: strip_metadata.or(payload_strip_metadata).unwrap_or(true),
        }
    }
}
