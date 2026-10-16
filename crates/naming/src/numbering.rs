//! Numbering modes (Python `download/task/manager.py` numbering block).

use crate::format::Value;

/// NumberingType values (`formatter::numbering_type` mirrors the constants;
/// this module takes the mode as a plain i64 so callers can pass options
/// straight through).
pub const FROM_SPECIFIED: i64 = 0;
pub const USE_PARSE_LIST: i64 = 1;
pub const CONTINUOUS: i64 = 2;

/// Mutable numbering state for one naming pass.
#[derive(Debug, Clone, PartialEq)]
pub struct NumberingState {
    pub global: i64,
    pub current: Option<i64>,
}

impl Default for NumberingState {
    fn default() -> Self {
        Self {
            global: 1,
            current: None,
        }
    }
}

/// Token for restoring state after a failed preview render. Holds the
/// `(global, current)` snapshot taken *after* the numbering step.
#[derive(Debug, Clone, PartialEq)]
pub enum NumberingToken {
    /// Snapshot taken; restore only if state hasn't moved on since.
    Active { global: i64, current: Option<i64> },
    /// `number` came from episode info: nothing to restore.
    None,
}

impl NumberingState {
    /// The Python flow computes the number, then unconditionally advances
    /// `global` (every item counts, even one whose naming later fails).
    pub fn next_number(
        &mut self,
        mode: i64,
        episode_number: Option<&Value>,
    ) -> (Value, NumberingToken) {
        let number = match mode {
            CONTINUOUS => Value::Int(self.global),
            FROM_SPECIFIED => {
                let value = Value::Int(self.current.unwrap_or(0));
                self.current = Some(self.current.unwrap_or(0) + 1);
                value
            }
            _ => episode_number
                .cloned()
                .unwrap_or(Value::Text(String::new())),
        };

        self.global += 1;

        let token = if mode == USE_PARSE_LIST {
            NumberingToken::None
        } else {
            NumberingToken::Active {
                global: self.global,
                current: self.current,
            }
        };
        (number, token)
    }

    /// Restores state for a rolled-back item. A stale token (state has
    /// since moved on) is a no-op: rewinding an unrelated item would
    /// renumber everything after it.
    pub fn rollback(&mut self, token: &NumberingToken) {
        let NumberingToken::Active { global, current } = token else {
            return;
        };
        if self.global == *global && self.current == *current {
            // Undo the global advance (+1) and the specified-number advance.
            self.global = global - 1;
            self.current = current.map(|c| c - 1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn continuous_mode() {
        let mut state = NumberingState::default();
        let (n1, _) = state.next_number(CONTINUOUS, None);
        let (n2, _) = state.next_number(CONTINUOUS, None);
        assert_eq!(n1, Value::Int(1));
        assert_eq!(n2, Value::Int(2));
        assert_eq!(state.global, 3);
    }

    #[test]
    fn from_specified_advances_current() {
        let mut state = NumberingState {
            global: 7,
            current: Some(5),
        };
        let (n, _) = state.next_number(FROM_SPECIFIED, None);
        assert_eq!(n, Value::Int(5));
        assert_eq!(state.current, Some(6));
        assert_eq!(state.global, 8);
    }

    #[test]
    fn use_parse_list_reads_episode() {
        let mut state = NumberingState::default();
        let (n, token) = state.next_number(USE_PARSE_LIST, Some(&Value::Text("3".into())));
        assert_eq!(n, Value::Text("3".into()));
        assert_eq!(token, NumberingToken::None);
        // global still advances unconditionally
        assert_eq!(state.global, 2);
    }

    #[test]
    fn rollback_restores_only_fresh_state() {
        let mut state = NumberingState {
            global: 4,
            current: Some(2),
        };
        let (_, token) = state.next_number(FROM_SPECIFIED, None);
        // fresh: restore
        state.rollback(&token);
        assert_eq!((state.global, state.current), (4, Some(2)));

        // stale: state moved on, no-op
        let mut state = NumberingState {
            global: 4,
            current: Some(2),
        };
        let (_, token) = state.next_number(FROM_SPECIFIED, None);
        state.next_number(FROM_SPECIFIED, None);
        state.rollback(&token);
        assert_eq!((state.global, state.current), (6, Some(4)));
    }
}
