use std::collections::BTreeMap;

use pdf_core::{
    AppError,
    models::{PresetProfilePayload, PresetUserConfigPayload},
};

use super::settings::{Preset, Settings};

#[derive(Debug, Clone)]
pub struct Presets {
    profiles: BTreeMap<Preset, Settings>,
    version: u32,
}

impl Default for Presets {
    fn default() -> Self {
        let mut profiles = BTreeMap::new();
        for preset in [Preset::Conservative, Preset::Balanced, Preset::Maximum] {
            profiles.insert(preset, Settings::for_preset(preset));
        }
        profiles.insert(Preset::Custom, Settings::for_preset(Preset::Custom));
        Self {
            profiles,
            version: 3,
        }
    }
}

impl Presets {
    pub fn from_payload(config: PresetUserConfigPayload) -> Self {
        let mut profiles = Self::default().profiles;
        for (name, profile) in config.presets {
            let preset = Preset::parse(&name);
            if preset.as_str() != name {
                continue;
            }
            let Some(settings) = profiles.get_mut(&preset) else {
                continue;
            };
            apply_profile(settings, profile);
        }
        Self {
            profiles,
            version: config.version.max(3),
        }
    }

    pub fn settings_for(&self, preset: Preset) -> Settings {
        self.profiles
            .get(&preset)
            .cloned()
            .unwrap_or_else(|| Settings::for_preset(preset))
    }

    pub fn replace_profile(&mut self, preset: Preset, settings: Settings) {
        self.profiles.insert(preset, settings.normalize());
    }

    pub fn to_payload(&self) -> PresetUserConfigPayload {
        let mut presets = std::collections::BTreeMap::new();
        for preset in [
            Preset::Conservative,
            Preset::Balanced,
            Preset::Maximum,
            Preset::Custom,
        ] {
            let Some(settings) = self.profiles.get(&preset) else {
                continue;
            };
            presets.insert(
                preset.as_str().to_owned(),
                PresetProfilePayload {
                    image_quality: settings.image_quality,
                    max_image_size_percent: settings.max_image_size_percent as u8,
                    optimize_images: Some(settings.optimize_images),
                    compress_streams: Some(settings.compress_streams),
                    strip_metadata: Some(settings.strip_metadata),
                    grayscale: Some(settings.grayscale),
                    bilevel_codec: Some(settings.bilevel_codec.as_label().to_owned()),
                    subset_fonts: Some(settings.subset_fonts),
                    cmyk_conversion: Some(settings.cmyk_conversion),
                },
            );
        }
        PresetUserConfigPayload {
            version: self.version,
            presets,
        }
    }

    pub fn load_default() -> Result<Self, AppError> {
        let path = pdf_core::preset_config::resolve_preset_config_path()?;
        let payload = pdf_core::preset_config::read_preset_config_at(&path)?;
        Ok(Self::from_payload(payload))
    }

    pub fn save_default(&self) -> Result<(), AppError> {
        let path = pdf_core::preset_config::resolve_preset_config_path()?;
        pdf_core::preset_config::write_preset_config_at(&path, &self.to_payload())
    }

    pub fn clear_default() -> Result<(), AppError> {
        let path = pdf_core::preset_config::resolve_preset_config_path()?;
        pdf_core::preset_config::clear_preset_config_at(&path)
    }
}

fn apply_profile(settings: &mut Settings, profile: PresetProfilePayload) {
    settings.image_quality = profile.image_quality.clamp(1, 100);
    settings.max_image_size_percent = u16::from(profile.max_image_size_percent.clamp(5, 100));
    settings.optimize_images = profile.optimize_images.unwrap_or(settings.optimize_images);
    settings.compress_streams = profile
        .compress_streams
        .unwrap_or(settings.compress_streams);
    settings.strip_metadata = profile.strip_metadata.unwrap_or(settings.strip_metadata);
    settings.grayscale = profile.grayscale.unwrap_or(settings.grayscale);
    settings.bilevel_codec = profile
        .bilevel_codec
        .as_deref()
        .map(|value| pdf_core::BilevelCodec::from_optional_str(Some(value)))
        .unwrap_or(settings.bilevel_codec);
    settings.subset_fonts = profile.subset_fonts.unwrap_or(settings.subset_fonts);
    settings.cmyk_conversion = profile.cmyk_conversion.unwrap_or(settings.cmyk_conversion);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_in_profiles_match_frontend_and_custom_starts_from_maximum() {
        let profiles = Presets::default();
        assert_eq!(
            profiles.settings_for(Preset::Conservative).image_quality,
            72
        );
        assert_eq!(
            profiles
                .settings_for(Preset::Balanced)
                .max_image_size_percent,
            68
        );
        assert!(profiles.settings_for(Preset::Maximum).subset_fonts);
        assert_eq!(profiles.settings_for(Preset::Custom).image_quality, 46);
    }

    #[test]
    fn config_overrides_builtin_profiles_and_roundtrips() {
        let mut config = PresetUserConfigPayload::default();
        config.presets.insert(
            "balanced".into(),
            PresetProfilePayload {
                image_quality: 49,
                max_image_size_percent: 61,
                grayscale: Some(true),
                ..Default::default()
            },
        );
        let profiles = Presets::from_payload(config);
        let balanced = profiles.settings_for(Preset::Balanced);
        assert_eq!(balanced.image_quality, 49);
        assert_eq!(balanced.max_image_size_percent, 61);
        assert!(balanced.grayscale);
        let roundtrip = Presets::from_payload(profiles.to_payload());
        assert_eq!(roundtrip.settings_for(Preset::Balanced).image_quality, 49);
    }
}
