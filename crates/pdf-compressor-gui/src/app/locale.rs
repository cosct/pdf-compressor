//! Small static locale catalog used by the native shell while the full UI is
//! being migrated. YAML catalogs beside the crate remain source material for
//! the later complete i18n pass.

use super::prefs::LocalePreference;
use std::sync::{OnceLock, RwLock};

static CURRENT_LOCALE: OnceLock<RwLock<LocalePreference>> = OnceLock::new();
fn current_locale() -> LocalePreference {
    *CURRENT_LOCALE
        .get_or_init(|| RwLock::new(LocalePreference::System))
        .read()
        .expect("locale lock poisoned")
}

#[derive(Debug, Clone, Copy)]
pub enum Text {
    Title,
    AddPdfs,
    CompressSelected,
    CompressPending,
    Cancel,
    Conservative,
    Balanced,
    Maximum,
    Custom,
    ApplyToAll,
    ChooseFolder,
    OutputDirectory,
    TargetMb,
    ApplyDirectory,
    ApplyTarget,
    Quality,
    MaxEdge,
    Images,
    Streams,
    StripMetadata,
    Grayscale,
    SubsetFonts,
    Cmyk,
    SystemTheme,
    Light,
    Dark,
    SystemLanguage,
    English,
    Chinese,
    PdfPassword,
    RetryAnalysis,
    Dismiss,
    Remove,
    AddFilesHint,
    ReadyToAnalyze,
    CompressionFinished,
    NoSmallerOutput,
    SavedPercent,
    AnalyzeSummary,
    Progress,
    QueueLocked,
    FilePickerError,
    DirectoryPickerError,
    QueueRestoreError,
    MissingRestore,
    ScanPipeline,
    SettingsRecommended,
    OnlySessionOutputs,
    PresetLoadError,
    On,
    Off,
}

pub fn text(id: Text) -> String {
    let chinese = matches!(current_locale(), LocalePreference::ChineseSimplified);
    let value = match (chinese, id) {
        (true, Text::Title) => "PDF 压缩器",
        (false, Text::Title) => "PDF Compressor",
        (true, Text::AddPdfs) => "添加 PDF",
        (false, Text::AddPdfs) => "Add PDFs",
        (true, Text::CompressSelected) => "压缩选中项",
        (false, Text::CompressSelected) => "Compress selected",
        (true, Text::CompressPending) => "压缩待处理项",
        (false, Text::CompressPending) => "Compress pending",
        (true, Text::Cancel) => "取消",
        (false, Text::Cancel) => "Cancel",
        (true, Text::Conservative) => "保守",
        (false, Text::Conservative) => "Conservative",
        (true, Text::Balanced) => "均衡",
        (false, Text::Balanced) => "Balanced",
        (true, Text::Maximum) => "最大压缩",
        (false, Text::Maximum) => "Maximum",
        (true, Text::Custom) => "自定义",
        (false, Text::Custom) => "Custom",
        (true, Text::ApplyToAll) => "应用到全部任务",
        (false, Text::ApplyToAll) => "Apply settings to all",
        (true, Text::ChooseFolder) => "选择文件夹…",
        (false, Text::ChooseFolder) => "Choose folder…",
        (true, Text::OutputDirectory) => "输出目录",
        (false, Text::OutputDirectory) => "Output directory",
        (true, Text::TargetMb) => "目标大小（MB）",
        (false, Text::TargetMb) => "Target size (MB)",
        (true, Text::ApplyDirectory) => "应用目录",
        (false, Text::ApplyDirectory) => "Apply directory",
        (true, Text::ApplyTarget) => "应用目标大小",
        (false, Text::ApplyTarget) => "Apply target",
        (true, Text::Quality) => "质量 %{value}",
        (false, Text::Quality) => "Quality %{value}",
        (true, Text::MaxEdge) => "最大边长 %{value}%",
        (false, Text::MaxEdge) => "Max edge %{value}%",
        (true, Text::Images) => "图片优化：%{state}",
        (false, Text::Images) => "Images: %{state}",
        (true, Text::Streams) => "流压缩：%{state}",
        (false, Text::Streams) => "Streams: %{state}",
        (true, Text::StripMetadata) => "清除元数据：%{state}",
        (false, Text::StripMetadata) => "Strip metadata: %{state}",
        (true, Text::Grayscale) => "灰度：%{state}",
        (false, Text::Grayscale) => "Grayscale: %{state}",
        (true, Text::SubsetFonts) => "字体子集化：%{state}",
        (false, Text::SubsetFonts) => "Subset fonts: %{state}",
        (true, Text::Cmyk) => "CMYK 转换：%{state}",
        (false, Text::Cmyk) => "CMYK conversion: %{state}",
        (true, Text::SystemTheme) => "跟随系统主题",
        (false, Text::SystemTheme) => "System theme",
        (true, Text::Light) => "浅色",
        (false, Text::Light) => "Light",
        (true, Text::Dark) => "深色",
        (false, Text::Dark) => "Dark",
        (true, Text::SystemLanguage) => "跟随系统语言",
        (false, Text::SystemLanguage) => "System language",
        (true, Text::English) => "English",
        (false, Text::English) => "English",
        (true, Text::Chinese) => "简体中文",
        (false, Text::Chinese) => "简体中文",
        (true, Text::PdfPassword) => "PDF 密码",
        (false, Text::PdfPassword) => "PDF password",
        (true, Text::RetryAnalysis) => "重试分析",
        (false, Text::RetryAnalysis) => "Retry analysis",
        (true, Text::Dismiss) => "关闭",
        (false, Text::Dismiss) => "Dismiss",
        (true, Text::Remove) => "移除",
        (false, Text::Remove) => "Remove",
        (true, Text::AddFilesHint) => "添加 PDF 文件以开始。",
        (false, Text::AddFilesHint) => "Add PDF files to begin.",
        (true, Text::ReadyToAnalyze) => "准备分析。",
        (false, Text::ReadyToAnalyze) => "Ready to analyze.",
        (true, Text::CompressionFinished) => "压缩完成。",
        (false, Text::CompressionFinished) => "Compression finished.",
        (true, Text::NoSmallerOutput) => "原文件已经更小，因此没有写入输出文件。",
        (false, Text::NoSmallerOutput) => "The original is already smaller; no output was written.",
        (true, Text::SavedPercent) => "已节省 %{percent}%，保存到 %{path}",
        (false, Text::SavedPercent) => "Saved %{percent}% to %{path}",
        (true, Text::AnalyzeSummary) => "%{pages} 页 · %{images} 张图片 · %{kind} · 推荐 %{preset}",
        (false, Text::AnalyzeSummary) => {
            "%{pages} pages · %{images} images · %{kind} · recommended %{preset}"
        }
        (true, Text::Progress) => "%{phase} · %{percent}%",
        (false, Text::Progress) => "%{phase} · %{percent}%",
        (true, Text::QueueLocked) => "压缩运行时队列已锁定。",
        (false, Text::QueueLocked) => "The queue is locked while compression is running.",
        (true, Text::FilePickerError) => "文件选择器：%{error}",
        (false, Text::FilePickerError) => "File picker: %{error}",
        (true, Text::DirectoryPickerError) => "目录选择器：%{error}",
        (false, Text::DirectoryPickerError) => "Directory picker: %{error}",
        (true, Text::QueueRestoreError) => "恢复队列：%{error}",
        (false, Text::QueueRestoreError) => "Queue restore: %{error}",
        (true, Text::MissingRestore) => "恢复队列时跳过了 %{count} 个不存在的 PDF 文件。",
        (false, Text::MissingRestore) => {
            "Skipped %{count} missing PDF file(s) while restoring the queue."
        }
        (true, Text::ScanPipeline) => "扫描件：已启用图片优化和流压缩。",
        (false, Text::ScanPipeline) => {
            "Scan-heavy document: image and stream optimization are enabled."
        }
        (true, Text::SettingsRecommended) => "当前设置与推荐设置一致。",
        (false, Text::SettingsRecommended) => {
            "The selected settings match the current recommendation."
        }
        (true, Text::OnlySessionOutputs) => "只能打开本次会话生成的文件。",
        (false, Text::OnlySessionOutputs) => {
            "Only files produced during this session can be opened."
        }
        (true, Text::PresetLoadError) => "加载预设设置失败。",
        (false, Text::PresetLoadError) => "Failed to load preset settings.",
        (true, Text::On) => "开",
        (false, Text::On) => "on",
        (true, Text::Off) => "关",
        (false, Text::Off) => "off",
    };
    value.to_owned()
}

pub fn text_with(id: Text, values: &[(&str, &str)]) -> String {
    values.iter().fold(text(id), |text, (key, value)| {
        text.replace(&format!("%{{{key}}}"), value)
    })
}
pub fn set_locale(preference: LocalePreference) {
    *CURRENT_LOCALE
        .get_or_init(|| RwLock::new(LocalePreference::System))
        .write()
        .expect("locale lock poisoned") = preference;
    gpui_component::set_locale(match preference {
        LocalePreference::ChineseSimplified => "zh-CN",
        LocalePreference::English | LocalePreference::System => "en",
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn locale_catalog_has_both_supported_languages() {
        set_locale(LocalePreference::English);
        assert_eq!(text(Text::AddPdfs), "Add PDFs");
        set_locale(LocalePreference::ChineseSimplified);
        assert_eq!(text(Text::AddPdfs), "添加 PDF");
        set_locale(LocalePreference::English);
    }
    #[test]
    fn interpolates_simple_display_values() {
        set_locale(LocalePreference::English);
        assert_eq!(
            text_with(Text::Progress, &[("phase", "analyzing"), ("percent", "25")]),
            "analyzing · 25%"
        );
    }
}
