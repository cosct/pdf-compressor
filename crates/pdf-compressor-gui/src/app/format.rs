use std::path::Path;

pub fn is_pdf(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("pdf"))
}

/// `Path::file_name` on Unix treats Windows separators as ordinary
/// characters, but restored queue paths may have been written on Windows.
pub fn file_name(path: &Path) -> String {
    let text = path.to_string_lossy();
    text.rsplit(['/', '\\'])
        .find(|part| !part.is_empty())
        .unwrap_or(&text)
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn pdf_extension_is_case_insensitive_and_path_aware() {
        assert!(is_pdf(Path::new("a.pdf")));
        assert!(is_pdf(Path::new("a.PDF")));
        assert!(!is_pdf(Path::new("a.pdf.tmp")));
        assert!(!is_pdf(Path::new("folder.pdf/image.png")));
    }

    #[test]
    fn file_name_accepts_both_separator_styles() {
        assert_eq!(file_name(&PathBuf::from("/tmp/a.pdf")), "a.pdf");
        assert_eq!(file_name(&PathBuf::from(r"C:\docs\a.pdf")), "a.pdf");
    }
}
