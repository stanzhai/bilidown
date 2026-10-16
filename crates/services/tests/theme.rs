//! Theme token tests: mode resolution, accent parsing, scaling, skin consts.

use services::theme::{
    accent_rgba, theme_from_config, ThemeModeSetting, ResolvedThemeMode, LIGHT_SKIN, DARK_SKIN,
    DEFAULT_ACCENT, DisplayScaling,
};

#[test]
fn theme_mode_config_parsing() {
    assert_eq!(ThemeModeSetting::from_config("Light"), ThemeModeSetting::Light);
    assert_eq!(ThemeModeSetting::from_config("Dark"), ThemeModeSetting::Dark);
    assert_eq!(ThemeModeSetting::from_config("Auto"), ThemeModeSetting::Auto);
    // Python default when the value is missing/invalid
    assert_eq!(ThemeModeSetting::from_config("garbage"), ThemeModeSetting::Auto);
}

#[test]
fn auto_resolves_against_system_appearance() {
    assert_eq!(ThemeModeSetting::Light.resolve(Some(true)), ResolvedThemeMode::Light);
    assert_eq!(ThemeModeSetting::Dark.resolve(Some(false)), ResolvedThemeMode::Dark);
    assert_eq!(ThemeModeSetting::Auto.resolve(Some(true)), ResolvedThemeMode::Dark);
    assert_eq!(ThemeModeSetting::Auto.resolve(Some(false)), ResolvedThemeMode::Light);
    // unknown system appearance → light (QPalette default)
    assert_eq!(ThemeModeSetting::Auto.resolve(None), ResolvedThemeMode::Light);
    assert!(ResolvedThemeMode::Dark.is_dark());
    assert!(!ResolvedThemeMode::Light.is_dark());
}

#[test]
fn accent_parsing_qfluentwidgets_formats() {
    // #RGB
    assert_eq!(accent_rgba("#0aa"), Some((0, 170, 170, 255)));
    // #RRGGBB
    assert_eq!(accent_rgba("#009faa"), Some((0x00, 0x9f, 0xaa, 255)));
    // #AARRGGBB (the Python default)
    assert_eq!(accent_rgba("#ff009faa"), Some((0x00, 0x9f, 0xaa, 0xff)));
    assert_eq!(DEFAULT_ACCENT, (0x00, 0x9f, 0xaa, 0xff));
    assert_eq!(accent_rgba("ff009faa"), None);
    assert_eq!(accent_rgba("#12345"), None);
    assert_eq!(accent_rgba("#zzzzzz"), None);
}

#[test]
fn display_scaling_parsing() {
    assert_eq!(DisplayScaling::from_config("1"), DisplayScaling::Factor(1.0));
    assert_eq!(DisplayScaling::from_config("1.25"), DisplayScaling::Factor(1.25));
    assert_eq!(DisplayScaling::from_config("2"), DisplayScaling::Factor(2.0));
    assert_eq!(DisplayScaling::from_config("Auto"), DisplayScaling::Auto);
    assert_eq!(DisplayScaling::from_config("garbage"), DisplayScaling::Auto);
    assert_eq!(DisplayScaling::Factor(1.5).resolve(None), 1.5);
    assert_eq!(DisplayScaling::Auto.resolve(None), 1.0);
    assert_eq!(DisplayScaling::Auto.resolve(Some(2.0)), 2.0);
    assert_eq!(DisplayScaling::Auto.resolve(Some(-1.0)), 1.0);
}

#[test]
fn qss_skin_palette_bits() {
    // light title bar: rgba(0,0,0,26)/rgba(0,0,0,51)
    assert_eq!(LIGHT_SKIN.title_hover, (0, 0, 0, 26));
    assert_eq!(LIGHT_SKIN.title_pressed, (0, 0, 0, 51));
    // dark close button: rgb(232,17,35)/rgb(241,112,122)
    assert_eq!(DARK_SKIN.close_hover, (232, 17, 35));
    assert_eq!(DARK_SKIN.close_pressed, (241, 112, 122));
    // dark dialog: border rgba(0,0,0,0.18) ≈ 46, surface rgba(255,255,255,0.0314) ≈ 8
    assert_eq!(DARK_SKIN.dialog_border, (0, 0, 0, 46));
    assert_eq!(DARK_SKIN.dialog_surface, (255, 255, 255, 8));
}

#[test]
fn theme_from_config_reads_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = config::ConfigStore::with_paths(dir.path(), dir.path().join("downloads"));
    // fresh store → defaults from defaults.json: Auto mode, #ff009faa, Auto scaling
    let (mode, accent, scaling) = theme_from_config(&cfg);
    assert_eq!(mode, ResolvedThemeMode::Light); // Auto + unknown system
    assert_eq!(accent, DEFAULT_ACCENT);
    assert_eq!(scaling, 1.0);

    let mut cfg = cfg;
    cfg.set("QFluentWidgets", "ThemeMode", serde_json::json!("Dark"));
    cfg.set("Interface", "display_scaling", serde_json::json!("1.5"));
    let (mode, _, scaling) = theme_from_config(&cfg);
    assert_eq!(mode, ResolvedThemeMode::Dark);
    assert_eq!(scaling, 1.5);
}
