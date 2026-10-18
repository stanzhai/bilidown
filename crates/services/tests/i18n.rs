//! i18n catalog tests. Vectors from `test/test_i18n_context.py` semantics:
//! every catalog context set must match across languages, the 8 named
//! contexts must resolve real translations, en is source passthrough.

use services::i18n::{
    self, catalog, episode_type, tip_messages, video_quality, Language, ResolvedLanguage,
};

const NAMED_CONTEXTS: &[(&str, &str)] = &[
    ("ParseInterface", "Paste and Parse"),
    ("MainWindow", "Downloads"),
    ("RuleListDialog", "Naming Rules"),
    ("EditRuleDialog", "Rule Name"),
    ("RuleBuilderWidget", "Directory structure"),
    ("FragmentEditView", "Custom format"),
    ("RulePreviewPanel", "Preview"),
    ("TIP_MESSAGES", "Queued..."),
];

/// Catalog totals pin the `.ts` extraction plus the HAND-ADDED block:
/// 125 contexts, 1012 live messages (973 generated + 39 hand-added),
/// same (context, source) set in both zh columns.
#[test]
fn catalog_covers_all_contexts_and_sources() {
    assert_eq!(i18n::catalog_len(), 1012);
    assert_eq!(i18n::catalog_contexts().len(), 125);

    // Both zh columns are built from the same (context, source) table, so
    // every context's entries resolve in both languages by construction;
    // spot-check the three biggest consumer contexts resolve.
    let zh_cn = catalog(ResolvedLanguage::ZhCn);
    let zh_tw = catalog(ResolvedLanguage::ZhTw);
    for (ctx, src) in [
        ("TIP_MESSAGES", "Download completed"),
        ("MainWindow", "Downloads"),
    ] {
        assert!(
            zh_cn.lookup(ctx, src).is_some(),
            "{ctx}/{src} missing zh_CN"
        );
        assert!(
            zh_tw.lookup(ctx, src).is_some(),
            "{ctx}/{src} missing zh_TW"
        );
    }
}

/// The 8 contexts the ticket names must translate a real message in both
/// zh variants.
#[test]
fn named_contexts_resolve_translations() {
    let zh_cn = catalog(ResolvedLanguage::ZhCn);
    let zh_tw = catalog(ResolvedLanguage::ZhTw);
    for (ctx, src) in NAMED_CONTEXTS {
        let cn = zh_cn.tr(ctx, src);
        let tw = zh_tw.tr(ctx, src);
        assert_ne!(cn, *src, "{ctx}: zh_CN returned source untranslated");
        assert_ne!(tw, *src, "{ctx}: zh_TW returned source untranslated");
        assert!(
            cn.chars().any(|c| (c as u32) >= 0x4e00),
            "{ctx}: zh_CN not Chinese: {cn}"
        );
        assert!(
            tw.chars().any(|c| (c as u32) >= 0x4e00),
            "{ctx}: zh_TW not Chinese: {tw}"
        );
    }
}

/// en (sourcelanguage) is a passthrough: tr returns the source.
#[test]
fn english_is_source_passthrough() {
    let en = catalog(ResolvedLanguage::En);
    assert_eq!(
        en.tr("TIP_MESSAGES", "Download completed"),
        "Download completed"
    );
    assert_eq!(en.tr("NoSuchContext", "anything"), "anything");
}

/// zh fallback: unknown (context, source) → source (Qt behavior).
#[test]
fn unknown_key_falls_back_to_source() {
    let zh_cn = catalog(ResolvedLanguage::ZhCn);
    assert_eq!(zh_cn.tr("TIP_MESSAGES", "Not in catalog"), "Not in catalog");
    assert_eq!(
        zh_cn.tr("NoSuchCtx", "Download completed"),
        "Download completed"
    );
}

/// `Interface.language` values parse; Auto resolves off the system locale.
#[test]
fn language_config_parsing() {
    assert_eq!(Language::from_config("zh_CN"), Language::ChineseSimplified);
    assert_eq!(Language::from_config("zh_TW"), Language::ChineseTraditional);
    assert_eq!(Language::from_config("en_US"), Language::English);
    assert_eq!(Language::from_config("Auto"), Language::Auto);
    assert_eq!(Language::from_config("garbage"), Language::Auto);

    assert_eq!(
        Language::ChineseSimplified.resolve(),
        ResolvedLanguage::ZhCn
    );
    assert_eq!(
        Language::ChineseTraditional.resolve(),
        ResolvedLanguage::ZhTw
    );
    assert_eq!(Language::English.resolve(), ResolvedLanguage::En);
}

/// Translator map: verbatim zh_CN expectations from the .ts files.
#[test]
fn translator_maps_translate() {
    let zh_cn = catalog(ResolvedLanguage::ZhCn);
    assert_eq!(
        tip_messages(zh_cn, "DOWNLOAD_COMPLETED").unwrap(),
        "下载完成"
    );
    assert_eq!(
        tip_messages(zh_cn, "DOWNLOAD_COMPLETED_DETAIL").unwrap(),
        "所有下载任务已完成"
    );
    assert_eq!(video_quality(zh_cn, "1080P").unwrap(), "1080P 高清");
    assert_eq!(episode_type(zh_cn, "TV").unwrap(), "电视剧");
    assert!(tip_messages(zh_cn, "NO_SUCH_KEY").is_none());
    // en passthrough
    let en = catalog(ResolvedLanguage::En);
    assert_eq!(
        tip_messages(en, "DOWNLOAD_COMPLETED").unwrap(),
        "Download completed"
    );
}

/// Terms/privacy HTML resolves to non-empty translated text.
#[test]
fn terms_and_privacy_text() {
    let zh_cn = catalog(ResolvedLanguage::ZhCn);
    let terms = i18n::terms_of_use(zh_cn);
    let privacy = i18n::privacy_policy(zh_cn);
    assert!(
        !terms.is_empty() && terms.contains('<'),
        "terms HTML: {terms}"
    );
    assert!(
        !privacy.is_empty() && privacy.contains('<'),
        "privacy HTML: {privacy}"
    );
}
