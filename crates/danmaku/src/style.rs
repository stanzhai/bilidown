//! Danmaku Style — the `Additional.danmaku_style` config shape
//! (`DefaultValue.danmaku_style` in src/util/common/config.py), field for
//! field. The ASS renderer reads every option; the settings dialog (ticket
//! 24) writes it.

use serde::{Deserialize, Serialize};

/// All Danmaku Style options (config.json `Additional.danmaku_style`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DanmakuStyle {
    pub font: FontStyle,
    pub border: BorderStyle,
    pub advanced: AdvancedStyle,
    pub resolution: Resolution,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FontStyle {
    pub name: String,
    pub size: i32,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BorderStyle {
    pub border: f64,
    pub shadow: i32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AdvancedStyle {
    /// Percent of screen height the danmaku may occupy (e.g. 60 → 0.6).
    pub display_area: i32,
    /// Percent opacity (e.g. 80 → 0.8).
    pub opacity: i32,
    /// Scroll duration in seconds.
    pub scroll_duration: i64,
    /// Top/bottom fixed duration in seconds.
    pub static_duration: i64,
    /// Minimum pixel gap between danmaku on one track.
    pub minimum_gap: i32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Resolution {
    pub width: i32,
    pub height: i32,
}

impl Default for DanmakuStyle {
    /// `DefaultValue.danmaku_style` verbatim.
    fn default() -> Self {
        Self {
            font: FontStyle {
                name: "黑体".to_owned(),
                size: 36,
                bold: false,
                italic: false,
                underline: false,
                strike: false,
            },
            border: BorderStyle {
                border: 1.0,
                shadow: 0,
            },
            advanced: AdvancedStyle {
                display_area: 60,
                opacity: 80,
                scroll_duration: 10,
                static_duration: 5,
                minimum_gap: 100,
            },
            resolution: Resolution {
                width: 1280,
                height: 720,
            },
        }
    }
}
