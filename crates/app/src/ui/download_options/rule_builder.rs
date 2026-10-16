//! Rule Builder view-model (ticket 21): the chip editor over
//! `naming::model` (`src/gui/component/rule_builder.py`).
//!
//! A rule is directory levels + a file-name level; each level is a row of
//! fragments (chips). Edits mutate the `RuleModel` directly and serialize
//! back with [`RuleModel::to_rule`] — round-trip through `parse_rule` is
//! lossless, so `rule()` is exactly what the engine would parse.

use naming::model::{parse_rule, Fragment, Level, RuleModel, UnsupportedRule};

/// Directory-level cap (`RuleBuilderWidget.MAX_LEVELS`) — deeper trees hit
/// Windows' path limit and fail on save, which is far harder to debug.
pub const MAX_LEVELS: usize = 8;

/// Format presets (`rule_format.py`): dates and numbers. The first preset is
/// the default applied when a variable changes (a date spec on a number
/// would fail to render).
pub const DATETIME_PRESETS: [&str; 5] =
    ["%Y-%m-%d", "%Y-%m", "%Y", "%Y-%m-%d_%H-%M-%S", "%Y年%m月%d日"];
pub const NUMBER_PRESETS: [&str; 4] = ["", "02d", "03d", "04d"];

pub fn presets_for(variable: &str) -> &'static [&'static str] {
    match naming::conventions::variable_type(variable) {
        naming::conventions::VariableType::Datetime => &DATETIME_PRESETS,
        naming::conventions::VariableType::Number => &NUMBER_PRESETS,
        naming::conventions::VariableType::Text => &[],
    }
}

/// The editor state: the model plus which fragment panel is open.
#[derive(Debug, Clone, Default)]
pub struct RuleBuilderModel {
    pub model: RuleModel,
    /// (level index, fragment index) of the chip whose edit panel is open.
    pub editing: Option<(usize, usize)>,
    /// `set_rule` failed — the visual editor is disabled; the caller falls
    /// back to raw-text editing (never silently drops unparseable parts).
    pub unsupported: Option<UnsupportedRule>,
}

impl RuleBuilderModel {
    /// `set_rule`: load a rule string; `None`/empty → one empty level.
    pub fn set_rule(&mut self, rule: &str) -> bool {
        self.unsupported = None;
        self.editing = None;
        if rule.is_empty() {
            self.model = RuleModel { levels: vec![Level::default()] };
            return true;
        }
        match parse_rule(rule) {
            Ok(model) => {
                self.model = model;
                if self.model.levels.is_empty() {
                    self.model.levels.push(Level::default());
                }
                true
            }
            Err(e) => {
                self.unsupported = Some(e);
                false
            }
        }
    }

    /// `rule()`: serialize the current tree.
    pub fn rule(&self) -> String {
        self.model.to_rule()
    }

    fn fragment_mut(&mut self, level: usize, frag: usize) -> Option<&mut Fragment> {
        self.model.levels.get_mut(level)?.fragments.get_mut(frag)
    }

    /// Fragment editor writes (`FragmentEditView.on_edited`): text fragments
    /// take `text`; variables take spec/prefix/suffix.
    pub fn edit_text(&mut self, level: usize, frag: usize, text: String) {
        if let Some(f) = self.fragment_mut(level, frag) {
            if f.is_text() {
                f.text = text;
            }
        }
    }

    pub fn edit_variable(
        &mut self,
        level: usize,
        frag: usize,
        spec: String,
        prefix: String,
        suffix: String,
    ) {
        if let Some(f) = self.fragment_mut(level, frag) {
            if !f.is_text() {
                f.spec = spec;
                f.prefix = prefix;
                f.suffix = suffix;
            }
        }
    }

    /// `on_variable_changed`: a new variable's old spec almost never fits
    /// (a date spec on a number won't render); fall to its first preset.
    pub fn change_variable(&mut self, level: usize, frag: usize, variable: String) {
        if let Some(f) = self.fragment_mut(level, frag) {
            if !f.is_text() {
                f.variable = Some(variable.clone());
                f.spec = presets_for(&variable).first().copied().unwrap_or("").to_string();
            }
        }
    }

    /// `add_fragment` — variable fragments insert with the variable's first
    /// preset as spec.
    pub fn add_variable(&mut self, level: usize, variable: String) {
        let spec = presets_for(&variable).first().copied().unwrap_or("").to_string();
        if let Some(l) = self.model.levels.get_mut(level) {
            l.fragments.push(Fragment { variable: Some(variable), spec, ..Default::default() });
        }
    }

    pub fn add_text(&mut self, level: usize, text: String) {
        if let Some(l) = self.model.levels.get_mut(level) {
            l.fragments.push(Fragment { text, ..Default::default() });
        }
    }

    /// `move_fragment`.
    pub fn move_fragment(&mut self, level: usize, frag: usize, offset: i64) {
        let Some(fragments) = self.model.levels.get_mut(level).map(|l| &mut l.fragments) else {
            return;
        };
        let target = frag as i64 + offset;
        if frag >= fragments.len() || target < 0 || target as usize >= fragments.len() {
            return;
        }
        fragments.swap(frag, target as usize);
    }

    /// `on_remove_fragment`.
    pub fn remove_fragment(&mut self, level: usize, frag: usize) {
        self.editing = None;
        if let Some(l) = self.model.levels.get_mut(level) {
            if frag < l.fragments.len() {
                l.fragments.remove(frag);
            }
        }
    }

    /// `on_add_level`: insert before the file-name level (always last).
    pub fn add_level(&mut self) {
        if self.model.levels.len() >= MAX_LEVELS {
            return;
        }
        let at = self.model.levels.len().saturating_sub(1);
        self.model.levels.insert(at, Level::default());
    }

    /// `on_remove_level`: the file-name level is never removable.
    pub fn remove_level(&mut self, level: usize) {
        if self.model.levels.len() > 1 && level + 1 < self.model.levels.len() {
            self.model.levels.remove(level);
            self.editing = None;
        }
    }

    /// `on_move_level`: cannot cross the file-name level.
    pub fn move_level(&mut self, level: usize, offset: i64) {
        let target = level as i64 + offset;
        if target < 0 || target as usize >= self.model.levels.len() - 1 {
            return;
        }
        self.model.levels.swap(level, target as usize);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn builder(rule: &str) -> RuleBuilderModel {
        let mut b = RuleBuilderModel::default();
        b.set_rule(rule);
        b
    }

    #[test]
    fn round_trip_parse_edit_serialize() {
        let mut b = builder("{parent_title}/P{p:02d}-{leaf_title}");
        b.add_text(0, "x".into());
        b.move_fragment(0, 2, -1);
        b.move_level(0, 0);
        b.add_level();
        let out = b.rule();
        let mut re = RuleBuilderModel::default();
        assert!(re.set_rule(&out));
        assert_eq!(re.rule(), out);
    }

    #[test]
    fn unsupported_rule_disables_editor() {
        let mut b = RuleBuilderModel::default();
        assert!(!b.set_rule("{{{garbage"));
        assert!(b.unsupported.is_some());
    }

    #[test]
    fn variable_change_resets_spec_to_first_preset() {
        let mut b = builder("{leaf_title}");
        b.change_variable(0, 0, "pub_time".into());
        assert_eq!(b.model.levels[0].fragments[0].spec, "%Y-%m-%d");
        b.change_variable(0, 0, "number".into());
        assert_eq!(b.model.levels[0].fragments[0].spec, "");
    }

    #[test]
    fn file_name_level_is_protected() {
        let mut b = builder("{parent_title}/{leaf_title}");
        b.remove_level(1);
        assert_eq!(b.model.levels.len(), 2);
        b.move_level(1, 1);
        assert_eq!(b.rule(), "{parent_title}/{leaf_title}");
    }

    #[test]
    fn level_cap_is_enforced() {
        let mut b = builder("{leaf_title}");
        for _ in 0..MAX_LEVELS + 2 {
            b.add_level();
        }
        assert!(b.model.levels.len() <= MAX_LEVELS);
    }
}
