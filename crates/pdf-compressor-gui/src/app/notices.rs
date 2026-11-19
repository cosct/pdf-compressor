use std::collections::BTreeMap;

use pdf_core::{AppError, AppErrorPayload, models::BackendNotice};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NoticeTone {
    Neutral,
    Success,
    Warning,
    Danger,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Notice {
    pub code: String,
    pub tone: NoticeTone,
    pub values: BTreeMap<String, String>,
    pub fallback: String,
}

impl From<BackendNotice> for Notice {
    fn from(notice: BackendNotice) -> Self {
        Self {
            code: notice.code,
            tone: match notice.level.as_str() {
                "success" => NoticeTone::Success,
                "warning" => NoticeTone::Warning,
                "danger" | "error" => NoticeTone::Danger,
                _ => NoticeTone::Neutral,
            },
            values: notice.values,
            fallback: notice.fallback,
        }
    }
}

impl From<AppErrorPayload> for Notice {
    fn from(error: AppErrorPayload) -> Self {
        Self {
            tone: if matches!(
                error.code.as_str(),
                "error.passwordRequired" | "error.wrongPassword"
            ) {
                NoticeTone::Warning
            } else {
                NoticeTone::Danger
            },
            code: error.code,
            values: error.values,
            fallback: error.fallback,
        }
    }
}

impl From<AppError> for Notice {
    fn from(error: AppError) -> Self {
        Self::from_error(&error)
    }
}

impl Notice {
    pub fn from_error(error: &AppError) -> Self {
        let code = error.code().to_owned();
        let tone = if matches!(
            code.as_str(),
            "error.passwordRequired" | "error.wrongPassword"
        ) {
            NoticeTone::Warning
        } else {
            NoticeTone::Danger
        };
        let fallback = error.to_string();
        let mut values = BTreeMap::new();
        if matches!(
            code.as_str(),
            "error.io" | "error.config" | "error.opener" | "error.pdfBuild"
        ) {
            values.insert("detail".to_owned(), fallback.clone());
        }
        Self {
            code,
            tone,
            values,
            fallback,
        }
    }
}

impl Notice {
    pub fn is_password_error(&self) -> bool {
        matches!(
            self.code.as_str(),
            "error.passwordRequired" | "error.wrongPassword"
        )
    }

    pub fn is_wrong_password(&self) -> bool {
        self.code == "error.wrongPassword"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_keep_stable_codes_and_password_classification() {
        let required = Notice::from(AppError::PasswordRequired);
        assert_eq!(required.code, "error.passwordRequired");
        assert!(required.is_password_error());
        assert!(!required.is_wrong_password());
        let wrong = Notice::from(AppError::WrongPassword);
        assert!(wrong.is_password_error() && wrong.is_wrong_password());
        assert!(!Notice::from(AppError::Encrypted).is_password_error());
    }
}
