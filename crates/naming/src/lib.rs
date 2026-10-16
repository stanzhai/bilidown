//! Naming engine: rule template compiler, visual-editor rule model, the 12
//! naming conventions with sample data, filename formatting with path
//! safety, numbering modes, and the task-naming snapshot flow.
//! (ticket 05; ports `util/format/rule_template.py`, `rule_model.py`,
//! `file_name.py`, `util/common/data/naming_convention.py`,
//! `naming_rules.py`, and the naming parts of `download/task/manager.py`)

pub mod conventions;
pub mod format;
pub mod formatter;
pub mod model;
pub mod naming_rules;
pub mod numbering;
pub mod template;
pub mod task_naming;

pub use format::Value;
