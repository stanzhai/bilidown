//! Theme: maps the config keys the Python build's PersonalizationCard
//! controls (`QFluentWidgets.ThemeMode` / `ThemeColor`,
//! `Interface.display_scaling`) onto the values the gpui-kit UI uses.
//!
//! qss skin deltas (`src/res/qss/{light,dark}/`) are exposed as palette
//! consts for the UI layer; gpui's own theme provides the base palette.

use config::ConfigStore;

/// `QFluentWidgets.ThemeMode` serialized value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeModeSetting {
    Light,
    Dark,
    /// Follow system appearance.
    Auto,
}

impl ThemeModeSetting {
    pub fn from_config(v: &str) -> Self {
        match v {
            "Light" => Self::Light,
            "Dark" => Self::Dark,
            _ => Self::Auto, // Python default
        }
    }

    /// Resolve `Auto` against the OS appearance. `system_dark = None` means
    /// "unknown" → light, matching QPalette default.
    pub fn resolve(self, system_dark: Option<bool>) -> ResolvedThemeMode {
        match self {
            Self::Light => ResolvedThemeMode::Light,
            Self::Dark => ResolvedThemeMode::Dark,
            Self::Auto => match system_dark {
                Some(true) => ResolvedThemeMode::Dark,
                _ => ResolvedThemeMode::Light,
            },
        }
    }
}

/// Concrete mode after Auto resolution (gpui `ThemeMode` has no Auto).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolvedThemeMode {
    Light,
    Dark,
}

impl ResolvedThemeMode {
    pub fn is_dark(self) -> bool {
        self == Self::Dark
    }
}

/// `Interface.display_scaling` serialized value.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DisplayScaling {
    Factor(f32),
    /// Follow OS setting.
    Auto,
}

impl DisplayScaling {
    pub fn from_config(v: &str) -> Self {
        match v.parse::<f32>() {
            Ok(f) if f.is_finite() && f > 0.0 => Self::Factor(f),
            _ => Self::Auto, // Python default "Auto"
        }
    }

    /// Resolve `Auto` against the OS scale factor; unknown → 1.0.
    pub fn resolve(self, system_scale: Option<f32>) -> f32 {
        match self {
            Self::Factor(f) => f,
            Self::Auto => system_scale
                .filter(|f| f.is_finite() && *f > 0.0)
                .unwrap_or(1.0),
        }
    }
}

/// qss-derived skin palette (the only meaningful bits of
/// `src/res/qss/{light,dark}/`); see ticket 19 research.
#[derive(Debug, Clone, Copy)]
pub struct SkinTokens {
    /// TitleBar button hover overlay (light: rgba(0,0,0,26)).
    pub title_hover: (u8, u8, u8, u8),
    /// TitleBar button pressed overlay (light: rgba(0,0,0,51)).
    pub title_pressed: (u8, u8, u8, u8),
    /// Close button hover (dark: rgb(232,17,35)).
    pub close_hover: (u8, u8, u8),
    /// Close button pressed (dark: rgb(241,112,122)).
    pub close_pressed: (u8, u8, u8),
    /// Dialog border.
    pub dialog_border: (u8, u8, u8, u8),
    /// Dialog surface.
    pub dialog_surface: (u8, u8, u8, u8),
}

/// Light skin: `src/res/qss/light/*.qss`.
pub const LIGHT_SKIN: SkinTokens = SkinTokens {
    title_hover: (0, 0, 0, 26),
    title_pressed: (0, 0, 0, 51),
    close_hover: (232, 17, 35),
    close_pressed: (241, 112, 122),
    dialog_border: (0, 0, 0, 17),
    dialog_surface: (255, 255, 255, 128),
};

/// Dark skin: `src/res/qss/dark/*.qss`.
pub const DARK_SKIN: SkinTokens = SkinTokens {
    title_hover: (255, 255, 255, 26),
    title_pressed: (255, 255, 255, 51),
    close_hover: (232, 17, 35),
    close_pressed: (241, 112, 122),
    dialog_border: (0, 0, 0, 46),       // rgba(0,0,0,0.18)
    dialog_surface: (255, 255, 255, 8), // rgba(255,255,255,0.0314)
};

/// Accent color parsed from `QFluentWidgets.ThemeColor`
/// (`#ff009faa` default). Returns RGBA; `None` when the config value is
/// not a color (the config layer already normalized it, so this is only
/// defensive).
pub fn accent_rgba(v: &str) -> Option<(u8, u8, u8, u8)> {
    let hex = v.strip_prefix('#')?;
    match hex.len() {
        3 => {
            let r = u8::from_str_radix(&hex[0..1], 16).ok()?;
            let g = u8::from_str_radix(&hex[1..2], 16).ok()?;
            let b = u8::from_str_radix(&hex[2..3], 16).ok()?;
            Some((r * 17, g * 17, b * 17, 255))
        }
        6 => {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            Some((r, g, b, 255))
        }
        8 => {
            let a = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let r = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let g = u8::from_str_radix(&hex[4..6], 16).ok()?;
            let b = u8::from_str_radix(&hex[6..8], 16).ok()?;
            Some((r, g, b, a))
        }
        _ => None,
    }
}

/// Effective accent for the UI: the configured `ThemeColor` if it parses,
/// else the Python default `#ff009faa`.
pub const DEFAULT_ACCENT: (u8, u8, u8, u8) = (0x00, 0x9f, 0xaa, 0xff);

/// Read the full theme configuration from the config store.
pub fn theme_from_config(cfg: &ConfigStore) -> (ResolvedThemeMode, (u8, u8, u8, u8), f32) {
    let mode = ThemeModeSetting::from_config(
        &cfg.get("QFluentWidgets", "ThemeMode")
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or("Auto".into()),
    );
    let color = cfg
        .get("QFluentWidgets", "ThemeColor")
        .and_then(|v| v.as_str().map(str::to_owned))
        .and_then(|v| accent_rgba(&v))
        .unwrap_or(DEFAULT_ACCENT);
    let scaling = DisplayScaling::from_config(
        &cfg.get("Interface", "display_scaling")
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or("Auto".into()),
    );
    // Auto resolution needs the OS; the service returns the unresolved Auto
    // defaults (light + 1.0) and the UI layer re-resolves with window
    // appearance once a window exists.
    let mode = mode.resolve(None);
    let scaling = scaling.resolve(None);
    (mode, color, scaling)
}
