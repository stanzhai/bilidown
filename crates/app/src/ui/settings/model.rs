//! Settings view models (port of `src/gui/interface/setting.py`,
//! `src/gui/component/setting/card.py`, and the naming-rule dialogs
//! `src/gui/dialog/setting/rule_list.py` / `edit_rule.py`).
//!
//! Everything here is plain Rust over [`config::ConfigStore`] plus the
//! `naming` crate: no gpui. The screen module only renders state and routes
//! gestures back into these models, so every behavior (cascades, gating,
//! write-through, rule CRUD, validation order) is unit-testable headless.
//!
//! Write-through semantics follow the Python app: the settings interface
//! has no OK button — every change lands in the config store and is saved
//! atomically immediately.

use std::path::Path;

use naming::conventions::{
    self, ConventionType, SampleShape, VariableListFactory, MIXED_ENTRY_TYPES,
};
use naming::formatter::FileNameFormatter;
use naming::naming_rules::{self, RuleEntry, RuleStore, DEFAULT_RULES};
use naming::template::compile_rule;
use serde_json::{json, Value};

// ---------- config accessors ----------

/// Read a bool (missing/typed-wrong → `fallback`).
pub fn get_bool(cfg: &config::ConfigStore, group: &str, item: &str) -> bool {
    cfg.get(group, item)
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

/// Read an int (missing → `fallback`).
pub fn get_int(cfg: &config::ConfigStore, group: &str, item: &str, fallback: i64) -> i64 {
    cfg.get(group, item)
        .and_then(|v| v.as_i64())
        .unwrap_or(fallback)
}

/// Read a string (missing → `""`).
pub fn get_str(cfg: &config::ConfigStore, group: &str, item: &str) -> String {
    cfg.get(group, item)
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
        .to_string()
}

/// Write one value through the store and save atomically. Returns the
/// save error (if any) so the screen can toast it — the in-memory value
/// is already updated either way, exactly like qfluentwidgets.
pub fn set_and_save(
    cfg: &mut config::ConfigStore,
    group: &str,
    item: &str,
    value: Value,
) -> anyhow::Result<()> {
    cfg.set(group, item, value);
    cfg.save()
}

// ---------- naming-rule bridge: config.json <-> RuleStore ----------

/// The rules saved in config (falling back to the built-ins when the value
/// is missing or malformed — Python `load_rules` semantics).
pub fn load_rules(cfg: &config::ConfigStore) -> Vec<RuleEntry> {
    parse_rule_list(cfg.get("File Naming", "naming_rule_list").as_ref())
        .unwrap_or_else(default_entries)
}

/// Parse `naming_rule_list` from raw JSON.
pub fn parse_rule_list(value: Option<&Value>) -> Option<Vec<RuleEntry>> {
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

/// Built-in default entries (`load_default_rules`).
pub fn default_entries() -> Vec<RuleEntry> {
    DEFAULT_RULES
        .iter()
        .map(|(id, name, type_, rule)| RuleEntry {
            id: (*id).to_string(),
            name: (*name).to_string(),
            type_: *type_,
            rule: (*rule).to_string(),
            default: true,
        })
        .collect()
}

/// Serialize entries back to the config value shape.
pub fn rule_list_value(entries: &[RuleEntry]) -> Value {
    Value::Array(
        entries
            .iter()
            .map(|e| {
                json!({
                    "id": e.id, "name": e.name, "type": e.type_,
                    "rule": e.rule, "default": e.default,
                })
            })
            .collect(),
    )
}

/// Save the rule list through the config store (atomic).
pub fn save_rules(cfg: &mut config::ConfigStore, entries: &[RuleEntry]) -> anyhow::Result<()> {
    cfg.set("File Naming", "naming_rule_list", rule_list_value(entries));
    cfg.save()
}

/// Rule display name: built-in translation keys pass through `display_name`
/// (English until a `Catalog` is attached by the screen); user names are
/// literal.
pub fn display_name(entry: &RuleEntry) -> String {
    naming_rules::display_name(entry)
}

// ---------- EditRuleDialog model ----------

/// Which widget the editor error belongs to (validation order returns it so
/// the screen can highlight the right control, like Python's
/// `validate() -> (valid, widget, message)`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorField {
    Name,
    Rule,
}

/// Live state of the rule editor (Python `EditRuleDialog`, minus widgets).
#[derive(Debug, Clone)]
pub struct RuleEditorModel {
    /// The entry as last committed/loaded.
    pub loaded: RuleEntry,
    /// Text of the name box (display name, not the stored key).
    pub name_text: String,
    /// Selected convention type id.
    pub type_: i64,
    /// Text of the advanced rule box.
    pub rule_text: String,
    /// "Set as default rule for this type" checkbox.
    pub default_checked: bool,
}

impl RuleEditorModel {
    /// `load()`: populate fields; a default rule locks type and default.
    pub fn load(entry: &RuleEntry) -> Self {
        let is_default = entry.default;
        Self {
            loaded: entry.clone(),
            name_text: display_name(entry),
            type_: entry.type_,
            rule_text: entry.rule.clone(),
            default_checked: is_default,
        }
    }

    /// Name as stored: unchanged built-ins keep their translation key.
    pub fn resolve_name(&self) -> String {
        if self.name_text == display_name(&self.loaded) {
            self.loaded.name.clone()
        } else {
            self.name_text.clone()
        }
    }

    /// `dump()`: the edited entry (never mutates the loaded one).
    pub fn dump(&self) -> RuleEntry {
        RuleEntry {
            id: self.loaded.id.clone(),
            name: self.resolve_name(),
            type_: self.type_,
            rule: self.rule_text.clone(),
            default: self.default_checked,
        }
    }

    /// `validate_rule()`: pure rule-string check, Python's exact order.
    pub fn validate_rule(&self) -> Result<(), String> {
        let rule = self.rule_text.as_str();
        if rule.is_empty() {
            return Err("Naming rule cannot be empty".into());
        }
        if rule.starts_with('/')
            || rule.starts_with('.')
            || rule.ends_with('/')
            || rule.ends_with('.')
        {
            return Err("Rule must not start or end with '/' or '.'".into());
        }
        let template = match compile_rule(rule, true) {
            Ok(t) => t,
            Err(e) => {
                return Err(format!("Unmatched '<' or '>' at position {}", e.position));
            }
        };
        let illegal =
            |c: char| matches!(c, '<' | '>' | ':' | '"' | '\\' | '|' | '?' | '*') || c.is_control();
        if template
            .literal_texts()
            .iter()
            .any(|t| t.chars().any(illegal))
        {
            return Err("Rule contains illegal characters".into());
        }
        let factory = VariableListFactory::new();
        let known: Vec<String> = factory
            .build_variable_data(self.convention(), SampleShape::Single)
            .keys()
            .cloned()
            .collect();
        if let Some(name) = template
            .field_names()
            .into_iter()
            .find(|n| !known.contains(n))
        {
            return Err(format!("Unknown variable: {name}"));
        }
        if template.has_empty_optional_segment() {
            return Err("An optional segment must contain at least one variable".into());
        }
        if self.render(SampleShape::Single).is_none() {
            return Err("Invalid naming rule".into());
        }
        Ok(())
    }

    /// Full `validate(other_names)`: name first, then the rule.
    pub fn validate(
        &self,
        other_names: &std::collections::BTreeSet<String>,
    ) -> Result<(), (EditorField, String)> {
        if self.name_text.is_empty() {
            return Err((EditorField::Name, "Rule name cannot be empty".into()));
        }
        if other_names.contains(self.name_text.as_str()) {
            return Err((
                EditorField::Name,
                "Another rule of this type already uses this name".into(),
            ));
        }
        self.validate_rule().map_err(|m| (EditorField::Rule, m))
    }

    fn convention(&self) -> ConventionType {
        ConventionType::ALL
            .into_iter()
            .find(|t| t.id() == self.type_)
            .unwrap_or(ConventionType::Normal)
    }

    /// `RulePreviewPanel.render`: preview path for one shape through the
    /// real formatter. `None` when it cannot render (shown as "—").
    pub fn render(&self, shape: SampleShape) -> Option<String> {
        if self.rule_text.is_empty() {
            return None;
        }
        let factory = VariableListFactory::new();
        let data = factory.build_variable_data(self.convention(), shape);
        let store = RuleStore::new();
        let formatter = FileNameFormatter::new(data, &store);
        let result = formatter.format(Some(self.type_), 0, &self.rule_text)?;
        let path = Path::new(&result);
        if path.parent() == Some(Path::new("")) || path.parent() == Some(Path::new(".")) {
            Some(result)
        } else {
            Some(format!(
                "{}{}{}",
                path.parent().unwrap().display(),
                std::path::MAIN_SEPARATOR,
                path.file_name()?.to_string_lossy()
            ))
        }
    }

    /// Preview rows: `(label_key, path)` for each supported shape, plus the
    /// mixed-entry note row for source types. Labels are translation
    /// sources from `RulePreviewPanel`.
    pub fn preview_rows(&self) -> Vec<(String, Option<String>)> {
        let type_ = self.convention();
        let mut rows: Vec<(String, Option<String>)> = conventions::supported_shapes(type_)
            .iter()
            .map(|shape| (shape_label(type_, *shape).to_string(), self.render(*shape)))
            .collect();
        if let Some((_, mixed)) = MIXED_ENTRY_TYPES.iter().find(|(t, _)| *t == type_) {
            rows.push((
                format!(
                    "{}\u{1f}",
                    mixed
                        .iter()
                        .map(|t| t.id().to_string())
                        .collect::<Vec<_>>()
                        .join("/")
                ),
                None,
            ));
        }
        rows
    }
}

/// `RulePreviewPanel.shape_label` — returns the English source string.
fn shape_label(type_: ConventionType, shape: SampleShape) -> &'static str {
    if type_ == ConventionType::Collection {
        return match shape {
            SampleShape::Collection => "Multi-part entry",
            _ => "Single-part entry",
        };
    }
    match shape {
        SampleShape::Multi => "Multi-part video",
        SampleShape::Collection => "Collection",
        SampleShape::Single => "Single video",
    }
}

// ---------- RuleListDialog model ----------

/// The naming-rules window (Python `RuleListDialog`). Owns the working
/// copy; the embedded editor is [`RuleEditorModel`].
#[derive(Debug, Clone)]
pub struct RuleListModel {
    /// Committed working copy.
    pub rules: Vec<RuleEntry>,
    /// Saved baseline for the `dirty` check.
    saved: Vec<RuleEntry>,
    pub current_index: Option<usize>,
    pub editor: RuleEditorModel,
}

impl RuleListModel {
    pub fn new(cfg: &config::ConfigStore) -> Self {
        Self::from_rules(load_rules(cfg))
    }

    /// Test/direct constructor.
    pub fn from_rules(rules: Vec<RuleEntry>) -> Self {
        let mut model = Self {
            saved: rules.clone(),
            rules,
            current_index: None,
            editor: RuleEditorModel::load(&RuleEntry {
                id: String::new(),
                name: String::new(),
                type_: ConventionType::Normal.id(),
                rule: String::new(),
                default: false,
            }),
        };
        model.select(0);
        model
    }

    /// `dirty`: computed from the working copy vs the saved baseline —
    /// never a sticky flag.
    pub fn dirty(&self) -> bool {
        self.rules != self.saved
    }

    /// Names of other rules of the editor's (possibly just changed) type.
    pub fn other_names(&self) -> std::collections::BTreeSet<String> {
        let mut names = std::collections::BTreeSet::new();
        if let Some(ix) = self.current_index {
            for (i, entry) in self.rules.iter().enumerate() {
                if i != ix && entry.type_ == self.editor.type_ {
                    names.insert(display_name(entry));
                }
            }
        }
        names
    }

    /// `_commit_editor()`: validate, write back, then converge the default
    /// flag per type. Err carries the validation failure.
    pub fn commit_editor(&mut self) -> Result<(), (EditorField, String)> {
        let Some(ix) = self.current_index else {
            return Ok(());
        };
        self.editor.validate(&self.other_names())?;
        let entry = self.editor.dump();
        self.rules[ix] = entry.clone();
        if entry.default {
            self.set_default_rule(&entry.id, entry.type_);
        }
        Ok(())
    }

    /// `_set_default_rule`: exactly one default per type.
    fn set_default_rule(&mut self, rule_id: &str, rule_type: i64) {
        for entry in &mut self.rules {
            if entry.type_ == rule_type {
                entry.default = entry.id == rule_id;
            }
        }
    }

    /// `_select_row`: select an index (clamped), reloading the editor only
    /// when the entry actually changed.
    pub fn select(&mut self, index: usize) {
        if self.rules.is_empty() {
            self.current_index = None;
            return;
        }
        let index = index.min(self.rules.len() - 1);
        let changed = Some(index) != self.current_index;
        self.current_index = Some(index);
        if changed {
            self.editor = RuleEditorModel::load(&self.rules[index]);
        }
    }

    /// User gesture: switch selection (commits first; on failure the caller
    /// must restore the old selection — returns the failing index).
    pub fn on_select(&mut self, index: usize) -> Result<(), (usize, (EditorField, String))> {
        let previous = self.current_index;
        if let Err(e) = self.commit_editor() {
            return Err((previous.unwrap_or(0), e));
        }
        self.select(index);
        Ok(())
    }

    /// `on_add_rule` → `_append_rule`: new rule follows the current type,
    /// seeded from that type's built-in default.
    pub fn add_rule(&mut self, new_id: String) -> Result<(), (usize, (EditorField, String))> {
        self.commit_editor()
            .map_err(|e| (self.current_index.unwrap_or(0), e))?;
        let type_id = self
            .current_index
            .and_then(|ix| self.rules.get(ix))
            .map(|e| e.type_)
            .unwrap_or_else(|| ConventionType::Normal.id());
        let seed = default_entries()
            .into_iter()
            .find(|e| e.type_ == type_id)
            .map(|e| e.rule)
            .unwrap_or_else(|| "{leaf_title}".to_string());
        let entry = RuleEntry {
            id: new_id,
            name: "NEW_RULE".to_string(),
            type_: type_id,
            rule: seed,
            default: false,
        };
        self.rules.push(entry.clone());
        let last = self.rules.len() - 1;
        self.current_index = Some(last);
        self.editor = RuleEditorModel::load(&entry);
        Ok(())
    }

    /// `on_duplicate_rule`: "{display_name} (copy)", not default.
    pub fn duplicate_rule(&mut self, new_id: String) -> Result<(), (usize, (EditorField, String))> {
        self.commit_editor()
            .map_err(|e| (self.current_index.unwrap_or(0), e))?;
        let Some(ix) = self.current_index else {
            return Ok(());
        };
        let mut entry = self.rules[ix].clone();
        entry.id = new_id;
        entry.name = format!("{} (copy)", display_name(&entry));
        entry.default = false;
        self.rules.push(entry.clone());
        let last = self.rules.len() - 1;
        self.current_index = Some(last);
        self.editor = RuleEditorModel::load(&entry);
        Ok(())
    }

    /// `on_delete_rule`: default rules are undeletable (Err explains).
    pub fn delete_rule(&mut self) -> Result<(), String> {
        let Some(ix) = self.current_index else {
            return Ok(());
        };
        if self.rules[ix].default {
            return Err("Only non-default naming rules can be deleted.".into());
        }
        self.rules.remove(ix);
        self.current_index = None;
        if !self.rules.is_empty() {
            self.select(ix.min(self.rules.len() - 1));
        }
        Ok(())
    }

    /// `on_restore_default_rule`: restore this row to its built-in value.
    pub fn restore_default_rule(&mut self) -> bool {
        let Some(ix) = self.current_index else {
            return false;
        };
        let Some(builtin) = default_entries()
            .into_iter()
            .find(|e| e.id == self.rules[ix].id)
        else {
            return false;
        };
        self.rules[ix] = builtin.clone();
        self.editor = RuleEditorModel::load(&builtin);
        true
    }

    /// `on_reset_to_default`: whole list back to built-ins.
    pub fn reset_to_default(&mut self) {
        self.rules = default_entries();
        self.current_index = None;
        self.select(0);
    }

    /// `on_item_moved`: reorder the working copy (after committing).
    pub fn move_rule(
        &mut self,
        from: usize,
        to: usize,
    ) -> Result<(), (usize, (EditorField, String))> {
        self.commit_editor()
            .map_err(|e| (self.current_index.unwrap_or(0), e))?;
        if from >= self.rules.len() || to >= self.rules.len() {
            return Ok(());
        }
        let entry = self.rules.remove(from);
        self.rules.insert(to, entry);
        self.current_index = Some(to);
        self.editor = RuleEditorModel::load(&self.rules[to]);
        Ok(())
    }

    /// `on_save`: commit + persist. The caller toasts on Ok.
    pub fn save(&mut self, cfg: &mut config::ConfigStore) -> anyhow::Result<()> {
        self.commit_editor().map_err(|(_, m)| anyhow::anyhow!(m))?;
        let entries = self.rules.clone();
        save_rules(cfg, &entries)?;
        self.saved = entries;
        Ok(())
    }

    /// `_display_entries`: committed copy overlaid with the live editor
    /// state, with default-exclusivity applied across the whole table.
    pub fn display_entries(&self) -> Vec<RuleEntry> {
        let mut entries = self.rules.clone();
        let Some(ix) = self.current_index else {
            return entries;
        };
        let mut live = self.editor.dump();
        if live.name.is_empty() {
            live.name = entries[ix].name.clone();
        }
        entries[ix] = live.clone();
        if live.default {
            for (i, entry) in entries.iter_mut().enumerate() {
                if i != ix && entry.type_ == live.type_ {
                    entry.default = false;
                }
            }
        }
        entries
    }

    /// `apply_filter` + `_matches`: which indices match the search text
    /// (name, type string, rule body; casefolded).
    pub fn filter_indices(&self, text: &str) -> Option<Vec<usize>> {
        let keywords = text.trim().to_lowercase();
        if keywords.is_empty() {
            return None;
        }
        Some(
            self.rules
                .iter()
                .enumerate()
                .filter(|(_, entry)| {
                    [
                        display_name(entry),
                        type_str(entry.type_),
                        entry.rule.clone(),
                    ]
                    .iter()
                    .any(|f| f.to_lowercase().contains(&keywords))
                })
                .map(|(i, _)| i)
                .collect(),
        )
    }
}

/// `_get_type_str` without a translator: the CONVENTION_TYPE key.
pub fn type_key(type_: i64) -> Option<&'static str> {
    ConventionType::ALL
        .into_iter()
        .find(|t| t.id() == type_)
        .and_then(type_key_of)
}

fn type_key_of(t: ConventionType) -> Option<&'static str> {
    Some(match t {
        ConventionType::Normal => "NORMAL",
        ConventionType::Part => "PART",
        ConventionType::Collection => "COLLECTION",
        ConventionType::InteractiveVideo => "INTERACTIVE_VIDEO",
        ConventionType::Bangumi => "BANGUMI",
        ConventionType::Cheese => "CHEESE",
        ConventionType::Favorite => "FAVORITE",
        ConventionType::Space => "SPACE",
        ConventionType::History => "HISTORY",
        ConventionType::WatchLater => "WATCH_LATER",
        ConventionType::Weekly => "WEEKLY",
        ConventionType::Audio => "AUDIO",
    })
}

/// English fallback for the type column (screen swaps in the catalog).
pub fn type_str(type_: i64) -> String {
    match type_key(type_) {
        Some(key) => super::i18n_bridge::convention_type_en(key).to_string(),
        None => String::new(),
    }
}

// ---------- MediaOptionsCard cascade ----------

/// The five media-option values (all in the `Download` group).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediaOptions {
    pub download_video_stream: bool,
    pub download_audio_stream: bool,
    pub merge_video_audio: bool,
    pub keep_original_files: bool,
    pub keep_original_files_type: i64,
}

impl MediaOptions {
    pub fn load(cfg: &config::ConfigStore) -> Self {
        Self {
            download_video_stream: get_bool(cfg, "Download", "download_video_stream"),
            download_audio_stream: get_bool(cfg, "Download", "download_audio_stream"),
            merge_video_audio: get_bool(cfg, "Download", "merge_video_audio"),
            keep_original_files: get_bool(cfg, "Download", "keep_original_files"),
            keep_original_files_type: get_int(cfg, "Download", "keep_original_files_type", 0),
        }
    }

    /// `save()` — five keys, one atomic save.
    pub fn save(&self, cfg: &mut config::ConfigStore) -> anyhow::Result<()> {
        cfg.set(
            "Download",
            "download_video_stream",
            Value::from(self.download_video_stream),
        );
        cfg.set(
            "Download",
            "download_audio_stream",
            Value::from(self.download_audio_stream),
        );
        cfg.set(
            "Download",
            "merge_video_audio",
            Value::from(self.merge_video_audio),
        );
        cfg.set(
            "Download",
            "keep_original_files",
            Value::from(self.keep_original_files),
        );
        cfg.set(
            "Download",
            "keep_original_files_type",
            Value::from(self.keep_original_files_type),
        );
        cfg.save()
    }

    /// `on_change_download_stream_options`: merge enabled = both streams;
    /// keep enabled = merge on; both cascade their checked state down.
    pub fn set_stream(&mut self, video: bool, audio: bool) {
        self.download_video_stream = video;
        self.download_audio_stream = audio;
        let enable = video && audio;
        self.merge_video_audio = enable;
        if !enable || !self.merge_video_audio {
            self.keep_original_files = false;
        }
    }

    /// `on_change_merge_option`: keep enabled = merge on.
    pub fn set_merge(&mut self, merge: bool) {
        self.merge_video_audio = merge;
        if !merge {
            self.keep_original_files = false;
        }
    }

    /// `on_change_keep_original_files_option`: the type combo's enable.
    pub fn set_keep(&mut self, keep: bool) {
        self.keep_original_files = keep;
    }

    pub fn merge_enabled(&self) -> bool {
        self.download_video_stream && self.download_audio_stream
    }

    pub fn keep_enabled(&self) -> bool {
        self.merge_enabled() && self.merge_video_audio
    }

    pub fn keep_type_enabled(&self) -> bool {
        self.keep_original_files
    }
}

// ---------- danmaku / subtitle / cover gating ----------

/// `update_embed_option_states`: embed requires download + ASS + MKV
/// container. Disabled only — never reset (the container is toggled back
/// and forth; runtime silently skips embedding as the backstop).
pub fn can_embed_subtitle_like(download: bool, format: &str, container: &str) -> bool {
    download && format == "ass" && container == "mkv"
}

/// Cover: `on_change_cover_format` + the toggles. avif cannot be embedded;
/// unchecking download or attach cascades the checkbox values down.
pub fn cover_cascade(
    download: bool,
    cover_type: &str,
    attach: bool,
    delete: bool,
) -> (bool, bool, bool, bool) {
    let can_embed = download && cover_type != "avif";
    let attach = if can_embed { attach } else { false };
    let delete = if can_embed && attach { delete } else { false };
    (can_embed, attach, delete, can_embed && attach)
}

// ---------- NumberSettingCard ----------

/// Global sequential starting number lives in runtime memory in Python
/// (`runtime.naming.global_starting_number`), not config.
#[derive(Debug, Clone, Default)]
pub struct NumberingModel {
    pub global_starting_number: i64,
}

// ---------- MCP card helpers ----------

/// `_ensure_token` + `generate_token` seam: the MCP crate lands in ticket
/// 28; a UUID4 hex is the same shape the Python generator produces.
pub fn ensure_token(cfg: &mut config::ConfigStore) -> String {
    let existing = get_str(cfg, "MCP", "mcp_token");
    if !existing.is_empty() {
        return existing;
    }
    let token = uuid::Uuid::new_v4().simple().to_string();
    cfg.set("MCP", "mcp_token", Value::from(token.clone()));
    token
}

/// `on_copy_http_config` payload.
pub fn http_client_config(cfg: &config::ConfigStore, token: &str) -> Value {
    json!({
        "mcpServers": {
            "bilidown": {
                "type": "http",
                "url": format!("http://127.0.0.1:{}/mcp", get_int(cfg, "MCP", "mcp_port", 23330)),
                "headers": {"Authorization": format!("Bearer {token}")},
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_cfg() -> (config::ConfigStore, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        (config::ConfigStore::with_paths(dir.path(), "/tmp/dl"), dir)
    }

    // ---- bridge round-trip ----

    #[test]
    fn rule_list_round_trips_through_config() {
        let (mut cfg, _d) = temp_cfg();
        let mut entries = default_entries();
        entries[0].rule = "{leaf_title} [custom]".into();
        entries.push(RuleEntry {
            id: "x".into(),
            name: "Mine".into(),
            type_: 11,
            rule: "{bvid}".into(),
            default: false,
        });
        save_rules(&mut cfg, &entries).unwrap();
        let back = load_rules(&cfg);
        assert_eq!(back.len(), entries.len());
        assert_eq!(back[0].rule, "{leaf_title} [custom]");
        assert_eq!(back.last().unwrap().name, "Mine");
        // persisted JSON keeps the Python field names
        let raw = cfg.get("File Naming", "naming_rule_list").unwrap();
        assert_eq!(raw[0]["name"], "DEFAULT_FOR_NORMAL");
        assert!(raw[0]["default"].as_bool().unwrap());
    }

    #[test]
    fn malformed_rule_list_falls_back_to_defaults() {
        let (mut cfg, _d) = temp_cfg();
        cfg.set("File Naming", "naming_rule_list", Value::from("nonsense"));
        assert_eq!(load_rules(&cfg), default_entries());
        // missing key likewise
        cfg.set("File Naming", "naming_rule_list", Value::Null);
        assert_eq!(load_rules(&cfg).len(), 12);
    }

    // ---- editor validation order ----

    fn editor(rule: &str) -> RuleEditorModel {
        let mut e = RuleEditorModel::load(&RuleEntry {
            id: "t".into(),
            name: "T".into(),
            type_: 11,
            rule: String::new(),
            default: false,
        });
        e.rule_text = rule.into();
        e
    }

    #[test]
    fn validation_order_is_empty_name_then_rule_then_syntax() {
        let mut e = editor("{leaf_title}");
        e.name_text = String::new();
        assert_eq!(
            e.validate(&Default::default()).unwrap_err().0,
            EditorField::Name
        );

        e.name_text = "T".into();
        e.rule_text = String::new();
        assert_eq!(
            e.validate(&Default::default()).unwrap_err(),
            (EditorField::Rule, "Naming rule cannot be empty".into())
        );

        e.rule_text = "/{leaf_title}".into();
        assert!(e
            .validate(&Default::default())
            .unwrap_err()
            .1
            .starts_with("Rule must not start"));

        e.rule_text = "{leaf_title}<".into();
        assert!(e
            .validate(&Default::default())
            .unwrap_err()
            .1
            .starts_with("Unmatched"));

        e.rule_text = "{nope}".into();
        assert_eq!(
            e.validate(&Default::default()).unwrap_err().1,
            "Unknown variable: nope"
        );

        e.rule_text = "<->".into(); // optional segment with no variable
        assert!(e
            .validate(&Default::default())
            .unwrap_err()
            .1
            .contains("optional segment"));
    }

    #[test]
    fn duplicate_name_within_type_rejected_across_types_ok() {
        let mut e = editor("{leaf_title}");
        e.name_text = "Preset: Single Video".into();
        let mut others = std::collections::BTreeSet::new();
        others.insert("Preset: Single Video".to_string());
        assert!(e.validate(&others).is_err());
        others.clear();
        assert!(e.validate(&others).is_ok());
    }

    #[test]
    fn resolve_name_keeps_builtin_key_until_edited() {
        let entry = default_entries()[0].clone();
        let mut e = RuleEditorModel::load(&entry);
        assert_eq!(e.resolve_name(), "DEFAULT_FOR_NORMAL");
        e.name_text = "Renamed".into();
        assert_eq!(e.resolve_name(), "Renamed");
    }

    #[test]
    fn preview_rows_cover_supported_shapes_and_mixed_note() {
        let mut e = editor("{parent_title}/P{p}-{leaf_title}");
        e.type_ = ConventionType::Part.id();
        let rows = e.preview_rows();
        assert_eq!(rows.len(), 1); // Part only supports Multi
        assert!(rows[0].0.contains("Multi-part video"));

        e.type_ = ConventionType::History.id();
        let rows = e.preview_rows();
        assert_eq!(rows.len(), 4); // 3 shapes + mixed-entry note
        assert!(rows[3].1.is_none());

        e.type_ = ConventionType::Collection.id();
        let rows = e.preview_rows();
        let labels: Vec<&str> = rows.iter().map(|(l, _)| l.as_str()).collect();
        assert!(labels.contains(&"Multi-part entry"));
        assert!(labels.contains(&"Single-part entry"));
    }

    #[test]
    fn preview_renders_through_real_formatter() {
        let mut e = editor("{parent_title}/P{p}-{leaf_title}");
        e.type_ = ConventionType::Part.id();
        let path = e.render(SampleShape::Multi).unwrap();
        assert!(path.contains('/'), "folder/file split shown: {path}");
    }

    // ---- list model ----

    fn list() -> RuleListModel {
        RuleListModel::from_rules(default_entries())
    }

    #[test]
    fn commit_validates_then_converges_default_exclusivity() {
        let mut m = list();
        m.add_rule("new-1".into()).unwrap();
        m.editor.name_text = "Second normal rule".into();
        m.editor.default_checked = true;
        m.commit_editor().unwrap();
        let normals: Vec<bool> = m
            .display_entries()
            .iter()
            .filter(|e| e.type_ == 11)
            .map(|e| e.default)
            .collect();
        assert_eq!(normals, vec![false, true]);
    }

    #[test]
    fn add_seeds_rule_from_types_builtin_default() {
        let mut m = list();
        m.select(1); // PART
        m.add_rule("n2".into()).unwrap();
        let last = m.rules.last().unwrap();
        assert_eq!(last.type_, 12);
        assert_eq!(last.rule, "{parent_title}/P{p}-{leaf_title}");
        assert_eq!(last.name, "NEW_RULE");
        assert_eq!(m.current_index, Some(m.rules.len() - 1));
    }

    #[test]
    fn duplicate_names_copy_and_drops_default() {
        let mut m = list();
        m.duplicate_rule("d1".into()).unwrap();
        let last = m.rules.last().unwrap();
        assert_eq!(last.name, "Preset: Single Video (copy)");
        assert!(!last.default);
    }

    #[test]
    fn delete_blocked_for_default_and_reselects_neighbor() {
        let mut m = list();
        m.select(0);
        assert_eq!(
            m.delete_rule().unwrap_err(),
            "Only non-default naming rules can be deleted."
        );
        m.add_rule("a".into()).unwrap();
        let count = m.rules.len();
        m.delete_rule().unwrap();
        assert_eq!(m.rules.len(), count - 1);
    }

    #[test]
    fn restore_one_vs_reset_all() {
        let mut m = list();
        m.rules[0].rule = "tampered".into();
        m.select(0);
        assert!(m.restore_default_rule());
        assert_eq!(m.rules[0].rule, "{leaf_title}");
        // custom rule has no builtin
        m.add_rule("c".into()).unwrap();
        assert!(!m.restore_default_rule());
        m.rules[0].rule = "tampered again".into();
        m.reset_to_default();
        assert_eq!(m.rules, default_entries());
        assert!(!m.dirty());
    }

    #[test]
    fn move_rule_reorders_after_commit() {
        let mut m = list();
        assert!(m.move_rule(0, 2).is_ok());
        assert_eq!(m.rules[2].id, default_entries()[0].id);
        assert_eq!(m.current_index, Some(2));
    }

    #[test]
    fn dirty_is_computed_not_sticky() {
        let (mut cfg, _d) = temp_cfg();
        let mut m = list();
        assert!(!m.dirty());
        m.rules[0].rule = "x".into();
        assert!(m.dirty());
        m.rules[0].rule = "{leaf_title}".into();
        assert!(!m.dirty());
        m.editor.rule_text = "y".into();
        m.save(&mut cfg).unwrap();
        assert!(!m.dirty());
        assert_eq!(load_rules(&cfg)[0].rule, "y");
    }

    #[test]
    fn display_entries_overlay_live_editor_without_touching_copy() {
        let mut m = list();
        m.editor.name_text = "Live edit".into();
        m.editor.default_checked = true;
        let shown = m.display_entries();
        assert_eq!(shown[0].name, "Live edit");
        // committed copy untouched until commit
        assert_eq!(m.rules[0].name, "DEFAULT_FOR_NORMAL");
        // same-type default flag visually cleared
        assert!(shown[0].default);
    }

    #[test]
    fn empty_live_name_keeps_old_name_in_display() {
        let mut m = list();
        m.editor.name_text = String::new();
        assert_eq!(m.display_entries()[0].name, "DEFAULT_FOR_NORMAL");
    }

    #[test]
    fn filter_matches_name_type_and_rule_body() {
        let m = list();
        let hits = m.filter_indices("multi-part series").unwrap();
        assert_eq!(hits, vec![1], "only PART's type name matches");
        let hits = m.filter_indices("{leaf_title}");
        assert!(
            hits.as_ref().is_some_and(|h| h.len() > 1),
            "substring rule-body match: {hits:?}"
        );
        assert!(m.filter_indices("zzz-not-there").unwrap().is_empty());
        assert!(m.filter_indices("").is_none());
    }

    #[test]
    fn select_commits_or_bounces_back() {
        let mut m = list();
        m.editor.rule_text = String::new(); // invalid
        let err = m.on_select(1).unwrap_err();
        assert_eq!(err.0, 0, "bounces back to the row being edited");
        assert_eq!(m.current_index, Some(0));
        m.editor.rule_text = "{leaf_title}".into();
        m.on_select(1).unwrap();
        assert_eq!(m.current_index, Some(1));
    }

    // ---- media options cascade ----

    #[test]
    fn media_cascade_merges_only_when_both_streams() {
        let mut mo = MediaOptions {
            download_video_stream: true,
            download_audio_stream: true,
            merge_video_audio: true,
            keep_original_files: true,
            keep_original_files_type: 0,
        };
        mo.set_stream(false, true);
        assert!(!mo.merge_video_audio);
        assert!(!mo.keep_original_files, "keep cascades off with merge");
        mo.set_stream(true, true);
        assert!(mo.merge_video_audio, "merge re-checked when both on");
        assert!(!mo.keep_original_files, "stream change cleared keep");
        mo.set_merge(true);
        assert!(
            mo.keep_type_enabled() == mo.keep_original_files,
            "type combo follows keep checkbox"
        );
        mo.set_keep(true);
        assert!(mo.keep_enabled());
        assert!(mo.keep_type_enabled());
    }

    #[test]
    fn media_options_write_through_all_five_keys() {
        let (mut cfg, _d) = temp_cfg();
        let mut mo = MediaOptions::load(&cfg);
        mo.set_keep(true);
        mo.keep_original_files_type = 2;
        mo.save(&mut cfg).unwrap();
        let raw = std::fs::read_to_string(cfg.path()).unwrap();
        assert!(raw.contains("\"keep_original_files\": true"));
        assert!(raw.contains("\"keep_original_files_type\": 2"));
    }

    // ---- subtitle/danmaku/cover gating ----

    #[test]
    fn embed_gating_requires_ass_and_mkv_and_download() {
        assert!(can_embed_subtitle_like(true, "ass", "mkv"));
        assert!(!can_embed_subtitle_like(false, "ass", "mkv"));
        assert!(!can_embed_subtitle_like(true, "xml", "mkv"));
        assert!(!can_embed_subtitle_like(true, "ass", "mp4"));
    }

    #[test]
    fn cover_avif_unchecks_attach_and_delete() {
        let (can, attach, delete, _) = cover_cascade(true, "avif", true, true);
        assert!(!can && !attach && !delete);
        let (can, attach, delete, _) = cover_cascade(true, "png", true, true);
        assert!(can && attach && delete);
        let (can, attach, delete, _) = cover_cascade(false, "png", true, true);
        assert!(!can && !attach && !delete);
    }

    // ---- write-through ----

    #[test]
    fn set_and_save_persists_atomically() {
        let (mut cfg, _d) = temp_cfg();
        set_and_save(&mut cfg, "Behavior", "silent_start", Value::from(true)).unwrap();
        let raw = std::fs::read_to_string(cfg.path()).unwrap();
        assert!(raw.contains("\"silent_start\": true"));
        // reload sees it
        let reopened = config::ConfigStore::with_paths(cfg.path().parent().unwrap(), "/tmp/dl");
        assert!(get_bool(&reopened, "Behavior", "silent_start"));
    }

    #[test]
    fn mcp_token_is_generated_once_and_reused() {
        let (mut cfg, _d) = temp_cfg();
        let t1 = ensure_token(&mut cfg);
        assert_eq!(t1.len(), 32);
        let t2 = ensure_token(&mut cfg);
        assert_eq!(t1, t2);
        let conf = http_client_config(&cfg, &t1);
        assert_eq!(conf["mcpServers"]["bilidown"]["type"], "http");
    }

    #[test]
    fn type_str_uses_convention_names() {
        assert_eq!(type_str(11), "Single Video");
        assert_eq!(type_str(90), "Music");
        assert_eq!(type_str(-1), "");
    }
}
