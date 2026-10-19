//! Window-state memory, ported from `MainWindow.apply_window_state` /
//! `get_saved_window_state` / `save_window_state`
//! (`src/gui/interface/main_window.py`).
//!
//! State persists in `config.json` under `Behavior/window_state` as
//! `{x, y, width, height, maximized}`; restoring requires
//! `Behavior/remember_window_state`. The UI layer owns the actual window;
//! this module is the pure save/validate/restore logic.

use config::ConfigStore;
use serde_json::{json, Value};

/// Default window size (`MainWindow.__init__`: `resize(950, 600)` +
/// `setMinimumSize(950, 600)`).
pub const DEFAULT_WIDTH: i64 = 950;
pub const DEFAULT_HEIGHT: i64 = 600;
/// Height of the dragable title bar used for on-screen validation.
pub const TITLE_BAR_HEIGHT: i64 = 40;

/// A validated window state ready to apply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowState {
    pub x: i64,
    pub y: i64,
    pub width: i64,
    pub height: i64,
    pub maximized: bool,
}

/// A screen's available (work-area) rect, for on-screen validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScreenRect {
    pub x: i64,
    pub y: i64,
    pub width: i64,
    pub height: i64,
}

impl ScreenRect {
    /// Rectangle intersection test (`QRect.intersects`): the title bar
    /// `(x, y, width, 40)` must overlap some screen's work area.
    pub fn title_bar_visible(&self, x: i64, y: i64, width: i64) -> bool {
        // title bar rect
        let (ax, ay, aw, ah) = (x, y, width, TITLE_BAR_HEIGHT);
        let (bx, by, bw, bh) = (self.x, self.y, self.width, self.height);
        ax < bx + bw && bx < ax + aw && ay < by + bh && by < ay + ah
    }
}

/// Read and validate the saved window state; `None` = use defaults
/// (2/3 of the main display, centered; see [`default_state`]).
/// Validation, in Python order:
pub fn saved_state(cfg: &ConfigStore, screens: &[ScreenRect]) -> Option<WindowState> {
    if !cfg
        .get("Behavior", "remember_window_state")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        return None;
    }
    let state = cfg.get("Behavior", "window_state")?.as_object()?.clone();
    let as_int = |k: &str| -> Option<i64> {
        let v = state.get(k)?;
        match v {
            Value::Number(n) => n.as_i64(),
            // Python int() coerces numeric strings; keep parity
            Value::String(s) => s.trim().parse::<i64>().ok(),
            _ => None,
        }
    };
    let x = as_int("x")?;
    let y = as_int("y")?;
    let width = as_int("width")?;
    let height = as_int("height")?;
    if width < DEFAULT_WIDTH || height < DEFAULT_HEIGHT {
        return None;
    }
    if !screens.iter().any(|s| s.title_bar_visible(x, y, width)) {
        return None;
    }
    Some(WindowState {
        x,
        y,
        width,
        height,
        maximized: state
            .get("maximized")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
    })
}

/// First-run fallback (no saved state): 2/3 of the given display's
/// bounds, floored at the 950×600 minimum, centered on that display.
pub fn default_state(screen: ScreenRect) -> WindowState {
    let width = screen.width * 2 / 3;
    let height = screen.height * 2 / 3;
    let width = width.max(DEFAULT_WIDTH);
    let height = height.max(DEFAULT_HEIGHT);
    WindowState {
        x: screen.x + (screen.width - width) / 2,
        y: screen.y + (screen.height - height) / 2,
        width,
        height,
        maximized: false,
    }
}

/// Persist a window state. Mirrors the guards in `save_window_state`:
/// no-op when `remember_window_state` is off, when the window was never
/// shown (`shown = false`), or when the geometry is non-positive. When
/// `maximized` is set, `normal` carries the last non-maximized geometry —
/// Python uses that instead of the full-screen rect.
pub fn save_state(
    cfg: &mut ConfigStore,
    shown: bool,
    geometry: WindowState,
    normal_geometry: Option<WindowState>,
) {
    if !cfg
        .get("Behavior", "remember_window_state")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        return;
    }
    if !shown {
        return;
    }
    let rect = if geometry.maximized {
        normal_geometry.unwrap_or(geometry)
    } else {
        geometry
    };
    if rect.width <= 0 || rect.height <= 0 {
        return;
    }
    cfg.set(
        "Behavior",
        "window_state",
        json!({
            "x": rect.x,
            "y": rect.y,
            "width": rect.width,
            "height": rect.height,
            "maximized": geometry.maximized,
        }),
    );
}
