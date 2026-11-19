use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ThemePreference {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LocalePreference {
    #[default]
    System,
    English,
    ChineseSimplified,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Preferences {
    pub theme: ThemePreference,
    pub locale: LocalePreference,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefs_roundtrip_and_missing_fields_default_to_system() {
        let prefs = Preferences {
            theme: ThemePreference::Dark,
            locale: LocalePreference::ChineseSimplified,
        };
        let json = serde_json::to_string(&prefs).unwrap();
        assert_eq!(serde_json::from_str::<Preferences>(&json).unwrap(), prefs);
        assert_eq!(
            serde_json::from_str::<Preferences>("{}").unwrap(),
            Preferences::default()
        );
    }
}
