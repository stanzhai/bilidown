//! Terms-of-use gate, ported from `src/main.py` + the terms dialog:
//! on first launch `Application/accepted_terms` (default false) is false,
//! the app shows the terms/privacy gate before any util init, and only an
//! explicit accept persists `true` and lets startup continue.

use config::ConfigStore;

/// Gate verdict for startup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TermsGate {
    /// Terms already accepted: continue startup (post-terms checks).
    Accepted,
    /// Not accepted yet: show the terms gate UI before continuing.
    Pending,
}

/// Current gate state from `Application/accepted_terms`.
pub fn gate(cfg: &ConfigStore) -> TermsGate {
    let accepted = cfg
        .get("Application", "accepted_terms")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if accepted {
        TermsGate::Accepted
    } else {
        TermsGate::Pending
    }
}

/// Persist the user's accept. Called from the terms dialog's accept button;
/// the reject path exits the app and never writes.
pub fn accept(cfg: &mut ConfigStore) {
    cfg.set(
        "Application",
        "accepted_terms",
        serde_json::Value::Bool(true),
    );
}
