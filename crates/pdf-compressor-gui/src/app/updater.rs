//! Updater contract independent of any packaging backend.
//!
//! The GPUI shell must not hard-code or duplicate the Tauri signing key. This
//! module only validates the signed-release metadata shape and chooses a
//! platform target; download, minisign verification and installation remain a
//! packaging adapter concern.

use std::collections::BTreeMap;

use semver::Version;
use serde::Deserialize;

pub const DEFAULT_ENDPOINT: &str =
    "https://github.com/cosct/pdf-compressor/releases/latest/download/latest.json";

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct UpdateManifest {
    pub version: String,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default)]
    pub pub_date: Option<String>,
    #[serde(default)]
    pub platforms: BTreeMap<String, PlatformArtifact>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct PlatformArtifact {
    pub url: String,
    pub signature: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateInfo {
    pub version: Version,
    pub notes: Option<String>,
    pub artifact: PlatformArtifact,
    pub target_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateDecision {
    Available(UpdateInfo),
    Current,
    InvalidManifest(String),
    UnsupportedPlatform { tried: Vec<String> },
}

pub fn parse_manifest(raw: &str) -> Result<UpdateManifest, String> {
    let manifest: UpdateManifest = serde_json::from_str(raw).map_err(|error| error.to_string())?;
    if manifest.version.trim().is_empty() {
        return Err("update manifest has no version".into());
    }
    Ok(manifest)
}

pub fn select_target(
    manifest: &UpdateManifest,
    current: &Version,
    os: &str,
    arch: &str,
    installer: Option<&str>,
) -> UpdateDecision {
    let remote = match Version::parse(&manifest.version) {
        Ok(version) => version,
        Err(error) => return UpdateDecision::InvalidManifest(format!("invalid version: {error}")),
    };
    if remote <= *current {
        return UpdateDecision::Current;
    }
    let mut tried = Vec::new();
    if let Some(installer) = installer {
        tried.push(format!("{os}-{arch}-{installer}"));
    }
    tried.push(format!("{os}-{arch}"));
    for key in &tried {
        if let Some(artifact) = manifest.platforms.get(key) {
            if artifact.url.starts_with("https://") && !artifact.signature.trim().is_empty() {
                return UpdateDecision::Available(UpdateInfo {
                    version: remote,
                    notes: manifest.notes.clone(),
                    artifact: artifact.clone(),
                    target_key: key.clone(),
                });
            }
            return UpdateDecision::InvalidManifest(format!("invalid artifact for {key}"));
        }
    }
    UpdateDecision::UnsupportedPlatform { tried }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest() -> UpdateManifest {
        parse_manifest(
            r#"{"version":"2.0.0","notes":"notes","platforms":{"linux-x86_64-deb":{"url":"https://example.test/app.deb","signature":"sig"},"linux-x86_64":{"url":"https://example.test/app","signature":"sig"}}}"#,
        )
        .unwrap()
    }

    #[test]
    fn target_specific_artifact_wins_over_generic_platform() {
        let decision = select_target(
            &manifest(),
            &Version::parse("1.0.0").unwrap(),
            "linux",
            "x86_64",
            Some("deb"),
        );
        let UpdateDecision::Available(info) = decision else {
            panic!("expected update")
        };
        assert_eq!(info.target_key, "linux-x86_64-deb");
    }

    #[test]
    fn generic_target_is_fallback_and_darwin_missing_is_explicit() {
        let decision = select_target(
            &manifest(),
            &Version::parse("1.0.0").unwrap(),
            "linux",
            "x86_64",
            Some("appimage"),
        );
        let UpdateDecision::Available(info) = decision else {
            panic!("expected generic update")
        };
        assert_eq!(info.target_key, "linux-x86_64");
        assert!(matches!(
            select_target(
                &manifest(),
                &Version::parse("1.0.0").unwrap(),
                "darwin",
                "aarch64",
                Some("app")
            ),
            UpdateDecision::UnsupportedPlatform { .. }
        ));
    }

    #[test]
    fn equal_or_older_version_is_current() {
        assert_eq!(
            select_target(
                &manifest(),
                &Version::parse("2.0.0").unwrap(),
                "linux",
                "x86_64",
                None
            ),
            UpdateDecision::Current
        );
        assert_eq!(
            select_target(
                &manifest(),
                &Version::parse("3.0.0").unwrap(),
                "linux",
                "x86_64",
                None
            ),
            UpdateDecision::Current
        );
    }

    #[test]
    fn malformed_or_non_https_artifacts_fail_closed() {
        assert!(parse_manifest(r#"{"version":"not-semver","platforms":{}}"#).is_ok());
        let mut invalid = manifest();
        invalid.platforms.get_mut("linux-x86_64").unwrap().url = "http://insecure.test/app".into();
        assert!(matches!(
            select_target(
                &invalid,
                &Version::parse("1.0.0").unwrap(),
                "linux",
                "x86_64",
                None
            ),
            UpdateDecision::InvalidManifest(_)
        ));
    }
}
