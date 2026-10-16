//! Episode numbering (Python `__get_number` / `__rollback_number` plus
//! `runtime.naming` state). The take-and-increment must be atomic under
//! one lock; rollback only restores when nobody else took a number since.

use crate::status::NumberingType;

/// `runtime.naming` numbering counters.
#[derive(Debug, Clone, Default)]
pub struct NamingState {
    pub global_starting_number: i64,
    pub current_starting_number: Option<i64>,
}

impl NamingState {
    pub fn new() -> Self {
        Self {
            global_starting_number: 1,
            current_starting_number: None,
        }
    }
}

/// Token for rolling back a taken number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NumberingToken {
    before: (i64, i64),
    after: (i64, i64),
}

/// The numbering counters themselves; `take` and `rollback` must be
/// called under one external lock to be atomic (as in Python).
#[derive(Debug)]
pub struct Numbering {
    pub state: NamingState,
}

impl Default for Numbering {
    fn default() -> Self {
        Self {
            state: NamingState::new(),
        }
    }
}

impl Numbering {
    /// `__get_number` + the unconditional `global_starting_number += 1`
    /// that follows it in `create()`. Caller holds the numbering lock.
    pub fn take(
        &mut self,
        mode: NumberingType,
        episode_number: Option<&str>,
    ) -> (String, NumberingToken) {
        let before = self.counters();
        let number = match mode {
            NumberingType::Continuous => self.state.global_starting_number.to_string(),
            NumberingType::FromSpecified => {
                let current = self.state.current_starting_number.unwrap_or(1);
                self.state.current_starting_number = Some(current + 1);
                current.to_string()
            }
            NumberingType::UseParseList => episode_number.unwrap_or_default().to_string(),
        };
        self.state.global_starting_number += 1;
        let after = self.counters();
        (number, NumberingToken { before, after })
    }

    /// `__rollback_number`: restore the counters only if they still equal
    /// what they were right after our take (nobody else numbered since).
    /// A hole in the sequence is preferred over a duplicate number.
    pub fn rollback(&mut self, token: NumberingToken) {
        if self.counters() == token.after {
            self.state.global_starting_number = token.before.0;
            self.state.current_starting_number = if token.before.1 == 0 {
                None
            } else {
                Some(token.before.1)
            };
        }
    }

    /// (global, current) with `None` flattened to 0 for comparison.
    fn counters(&self) -> (i64, i64) {
        (
            self.state.global_starting_number,
            self.state.current_starting_number.unwrap_or(0),
        )
    }
}
