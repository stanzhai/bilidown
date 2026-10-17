//! Naming rule store (Python `util/common/naming_rules.py` plus the
//! `naming_rule_list` default from `config.py`).
//!
//! Every accessor hands out independent copies: callers (the download
//! options dialog) mutate the returned entries to display translated names,
//! and that must never flow back into the store — the Python module guards
//! the same invariant with `deepcopy`.

/// One rule entry. `name` holds a translation key for the built-ins
/// (`DEFAULT_FOR_NORMAL` …) and the literal name for user rules.
#[derive(Debug, Clone, PartialEq)]
pub struct RuleEntry {
    pub id: String,
    pub name: String,
    pub type_: i64,
    pub rule: String,
    pub default: bool,
}

/// Parse `naming_rule_list` from raw JSON; `None` when missing, empty,
/// or malformed.
fn parse_rule_list(value: Option<&serde_json::Value>) -> Option<Vec<RuleEntry>> {
    let arr = value?.as_array()?;
    if arr.is_empty() {
        return None;
    }
    let mut out = Vec::with_capacity(arr.len());
    for entry in arr {
        let obj = entry.as_object()?;
        out.push(RuleEntry {
            id: obj.get("id")?.as_str()?.to_string(),
            name: obj.get("name")?.as_str()?.to_string(),
            type_: obj.get("type")?.as_i64()?,
            rule: obj.get("rule")?.as_str()?.to_string(),
            default: obj.get("default")?.as_bool()?,
        });
    }
    Some(out)
}

/// Built-in default rules (Python `DefaultValue.naming_rule_list`), all
/// `default = True`.
pub const DEFAULT_RULES: &[(&str, &str, i64, &str)] = &[
    ("a024c20c-5826-4e65-a1f5-802e3e2dbe4f", "DEFAULT_FOR_NORMAL", 11, "{leaf_title}"),
    ("2d98a265-e8e1-4b2a-8133-76bbc65c90fe", "DEFAULT_FOR_PART", 12, "{parent_title}/P{p}-{leaf_title}"),
    (
        "307906bd-86a2-4b6b-bd75-152a8c3e280b",
        "DEFAULT_FOR_COLLECTION",
        13,
        "{collection_title}/{section_title}/{parent_title}/{leaf_title}",
    ),
    ("1fe25f91-caf0-437e-b132-c9369261ff8b", "DEFAULT_FOR_INTERACTIVE_VIDEO", 14, "{parent_title}/{leaf_title}"),
    ("b1d4e8e3-ca17-4b41-87cf-cda45254701e", "DEFAULT_FOR_BANGUMI", 20, "{season_title}/{episode_title}"),
    ("d582ec37-d8c2-44cf-bbd7-b709ea5c2042", "DEFAULT_FOR_CHEESE", 30, "{series_title}/{episode_title}"),
    (
        "5913e25f-0bf3-4d3c-a608-8416af778a8a",
        "DEFAULT_FOR_FAVORITE",
        40,
        "{favorites_owner_id}_{favorites_owner}/{favorites_name}/{parent_title}/<P{p:02d}->{leaf_title}",
    ),
    (
        "8c48ac82-14c5-4d48-9de7-225d9b53513f",
        "DEFAULT_FOR_SPACE",
        50,
        "{space_owner_id}_{space_owner}/{parent_title}/<P{p:02d}->{leaf_title}",
    ),
    (
        "307ccc8e-ad2f-4195-94f0-162ee9ff1ac0",
        "DEFAULT_FOR_HISTORY",
        60,
        "{source_title}/{parent_title}/<P{p:02d}->{leaf_title}",
    ),
    (
        "0a72a82b-5684-448e-9db1-a342de933d3e",
        "DEFAULT_FOR_WATCH_LATER",
        70,
        "{source_title}/{parent_title}/<P{p:02d}->{leaf_title}",
    ),
    ("4d28285d-65ca-4c5c-bbb3-b3b5b570c52a", "DEFAULT_FOR_WEEKLY", 80, "{source_title}/{leaf_title}"),
    ("dc77bd15-be21-4847-856e-68bb3035042f", "DEFAULT_FOR_AUDIO", 90, "{source_title}/{uploader} - {leaf_title}"),
];

/// Display names for the built-in rule keys (Python `Translator.
/// DEFAULT_RULE_NAMES`, English fallback — the test environment runs without
/// a translator installed, and only "not the key itself" is observable).
pub const DEFAULT_RULE_NAMES: &[(&str, &str)] = &[
    ("DEFAULT_FOR_NORMAL", "Preset: Single Video"),
    ("DEFAULT_FOR_PART", "Preset: Multi-part Series"),
    ("DEFAULT_FOR_COLLECTION", "Preset: Collection"),
    ("DEFAULT_FOR_INTERACTIVE_VIDEO", "Preset: Interactive Video"),
    ("DEFAULT_FOR_BANGUMI", "Preset: Film & TV"),
    ("DEFAULT_FOR_CHEESE", "Preset: Courses"),
    ("DEFAULT_FOR_FAVORITE", "Preset: Favorites"),
    ("DEFAULT_FOR_SPACE", "Preset: Profile"),
    ("DEFAULT_FOR_AUDIO", "Preset: Music"),
    ("DEFAULT_FOR_HISTORY", "Preset: History"),
    ("DEFAULT_FOR_WEEKLY", "Preset: Weekly Picks"),
    ("DEFAULT_FOR_WATCH_LATER", "Preset: Watch Later"),
    ("NEW_RULE", "New rule"),
];

fn default_entries() -> Vec<RuleEntry> {
    DEFAULT_RULES
        .iter()
        .map(|(id, name, type_, rule)| RuleEntry {
            id: id.to_string(),
            name: name.to_string(),
            type_: *type_,
            rule: rule.to_string(),
            default: true,
        })
        .collect()
}

/// The active rule list. Defaults to the built-ins until the config crate
/// (ticket 02) takes over loading/saving.
// ponytail: process-global store; swap for injected config once crates/config lands.
#[derive(Clone)]
pub struct RuleStore {
    rules: Vec<RuleEntry>,
}

impl Default for RuleStore {
    fn default() -> Self {
        Self {
            rules: default_entries(),
        }
    }
}

impl RuleStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Build from a config `naming_rule_list` value; falls back to the
    /// built-ins when the value is missing or malformed (Python
    /// `load_rules` semantics).
    pub fn from_config_value(value: Option<&serde_json::Value>) -> Self {
        let mut store = Self::new();
        if let Some(entries) = parse_rule_list(value) {
            store.save_rules(entries);
        }
        store
    }

    /// Independent copy of the active rule list.
    pub fn load_rules(&self) -> Vec<RuleEntry> {
        self.rules.clone()
    }

    /// Independent copy of the built-in defaults ("reset to defaults").
    pub fn load_default_rules(&self) -> Vec<RuleEntry> {
        default_entries()
    }

    pub fn save_rules(&mut self, rule_list: Vec<RuleEntry>) {
        self.rules = rule_list;
    }

    /// Rule template for `type_id`'s default entry, if any.
    pub fn get_rule_from_config(&self, type_id: Option<i64>) -> Option<String> {
        self.rules
            .iter()
            .find(|entry| Some(entry.type_) == type_id && entry.default)
            .map(|entry| entry.rule.clone())
    }

    /// Rule template by id.
    pub fn get_rule_by_id(&self, rule_id: &str) -> Option<String> {
        self.rules
            .iter()
            .find(|entry| entry.id == rule_id)
            .map(|entry| entry.rule.clone())
    }

    /// Entries for a type, copied (callers mutate `name` for display).
    pub fn get_rule_list_from_type(&self, type_id: Option<i64>) -> Vec<RuleEntry> {
        self.rules
            .iter()
            .filter(|entry| Some(entry.type_) == type_id)
            .cloned()
            .collect()
    }

    pub fn get_rule_list_from_attribute(&self, attribute: i64) -> Vec<RuleEntry> {
        let type_id = crate::formatter::get_type_id_from_attribute(attribute);
        self.get_rule_list_from_type(type_id)
    }
}

/// Rule display name: translate built-in keys, pass user names through.
/// Never writes the translation back — persisting it would freeze the name
/// in the current language and break the key lookup after a language switch.
pub fn display_name(entry: &RuleEntry) -> String {
    DEFAULT_RULE_NAMES
        .iter()
        .find(|(key, _)| *key == entry.name)
        .map(|(_, name)| name.to_string())
        .unwrap_or_else(|| entry.name.clone())
}
