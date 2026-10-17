//! Small i18n bridge for the settings screen: resolves the app language
//! once (gpui closures only get `&mut App`, so the resolved language and
//! catalog are captured up front) and formats the translator-map entries
//! this screen needs.

use services::i18n::{self, ResolvedLanguage};

/// Active language + the resolved catalog handle, captured at screen build.
#[derive(Debug, Clone, Copy)]
pub struct Lang {
    pub resolved: ResolvedLanguage,
}

impl Default for Lang {
    fn default() -> Self {
        Self {
            resolved: ResolvedLanguage::En,
        }
    }
}

impl Lang {
    /// Resolve from the `Interface.language` config value.
    pub fn from_config(value: &str) -> Self {
        Self {
            resolved: i18n::Language::from_config(value).resolve(),
        }
    }

    /// Translate `(context, source)`; English source passes through.
    pub fn tr<'a>(&self, ctx: &str, source: &'a str) -> &'a str {
        i18n::catalog(self.resolved).tr(ctx, source)
    }

    /// `Translator.CONVENTION_TYPE` (key → label).
    pub fn convention_type(&self, key: &str) -> String {
        i18n::convention_type(i18n::catalog(self.resolved), key)
            .unwrap_or_else(|| convention_type_en(key).to_string())
    }

    /// `Translator.DEFAULT_RULE_NAMES` (key → display name).
    pub fn default_rule_name(&self, key: &str) -> String {
        i18n::default_rule_names(i18n::catalog(self.resolved), key)
            .unwrap_or_else(|| key.to_string())
    }

    /// `Translator.VARIABLE_DESCRIPTION` (key → description).
    pub fn variable_description(&self, key: &str) -> String {
        i18n::variable_description(i18n::catalog(self.resolved), key)
            .unwrap_or_else(|| key.to_string())
    }
}

/// English fallback for CONVENTION_TYPE keys (mirrors the i18n source
/// strings; keeps the model layer translator-free).
pub fn convention_type_en(key: &str) -> &'static str {
    match key {
        "NORMAL" => "Single Video",
        "PART" => "Multi-part Series",
        "COLLECTION" => "Collection",
        "INTERACTIVE_VIDEO" => "Interactive Video",
        "BANGUMI" => "Film & TV",
        "CHEESE" => "Courses",
        "FAVORITE" => "Favorites",
        "SPACE" => "Profile",
        "HISTORY" => "History",
        "WATCH_LATER" => "Watch Later",
        "WEEKLY" => "Weekly Picks",
        "AUDIO" => "Music",
        _ => "",
    }
}
