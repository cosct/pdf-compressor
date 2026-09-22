//! strings.xml parity: the Android error strings are the mobile twin of the
//! desktop locales — the key set must stay in lockstep with the engine's
//! closed error taxonomy (and en/zh must carry identical key sets).
//! strings.xml 对齐钉子：安卓错误文案是桌面 locales 的移动镜像 —— 键集必须
//! 与引擎错误码闭集同步，且中英文键集一致。
//!
//! Same discipline as the desktop's `locales.spec.ts` pin and the
//! `error_code_taxonomy_is_a_pinned_closed_set` engine test: add at the end,
//! never rename.

use std::collections::BTreeSet;

use pdf_core_ffi::FfiError;

const EN: &str = include_str!("../../../android/app/src/main/res/values/strings.xml");
const ZH: &str = include_str!("../../../android/app/src/main/res/values-zh/strings.xml");

/// `<string name="…">` names in a strings.xml (order-insensitive).
fn string_keys(xml: &str) -> BTreeSet<&str> {
    let mut keys = BTreeSet::new();
    let mut rest = xml;
    while let Some(start) = rest.find("<string name=\"") {
        let after = &rest[start + "<string name=\"".len()..];
        let Some(end) = after.find('"') else { break };
        keys.insert(&after[..end]);
        rest = &after[end..];
    }
    keys
}

#[test]
fn error_string_keys_mirror_the_engine_taxonomy() {
    let en = string_keys(EN);
    let expected: BTreeSet<&str> = [
        "error_missingInput",
        "error_invalidPdfPath",
        "error_inputTooLarge",
        "error_encryptedPdf",
        "error_passwordRequired",
        "error_wrongPassword",
        "error_io",
        "error_cancelled",
        "error_config",
        "error_opener",
        "error_pdfBuild",
    ]
    .into_iter()
    .collect();
    assert!(
        en.is_superset(&expected),
        "strings.xml must carry every taxonomy key (missing: {:?})",
        expected.difference(&en).collect::<Vec<_>>()
    );
}

#[test]
fn every_error_string_key_belongs_to_the_taxonomy() {
    // No stray error_* keys: an unknown key means someone hand-added a
    // string no FfiError code will ever resolve.
    let en = string_keys(EN);
    let known = [
        "error_missingInput",
        "error_invalidPdfPath",
        "error_inputTooLarge",
        "error_encryptedPdf",
        "error_passwordRequired",
        "error_wrongPassword",
        "error_io",
        "error_cancelled",
        "error_config",
        "error_opener",
        "error_pdfBuild",
    ];
    for key in en {
        if key.starts_with("error_") {
            assert!(
                known.contains(&key),
                "strings.xml carries taxonomy-unknown key {key}"
            );
        }
    }
}

#[test]
fn zh_keys_match_en_keys_exactly() {
    assert_eq!(
        string_keys(EN),
        string_keys(ZH),
        "en and zh strings.xml must expose identical key sets"
    );
}

#[test]
fn ffi_error_codes_resolve_to_string_resources() {
    let en = string_keys(EN);
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
    for code in codes {
        let resource = code.replace('.', "_");
        assert!(
            en.contains(resource.as_str()),
            "FfiError code {code} resolves to no strings.xml key"
        );
    }
}
