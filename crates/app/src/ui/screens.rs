//! Screens: parse (in `super::parse`), downloads, favorites, settings,
//! about.

pub mod about {
    use gpui_kit::component::button::Button;
    use gpui_kit::{Context, ParentElement, div, prelude::*};

    use crate::ui::dialogs::about;

    /// About screen (port of `gui/dialog/main_window/about.py`).
    /// Content and i18n live in `ui::dialogs::about`; this renders it.
    pub fn render_about(cx: &mut Context<crate::ui::Shell>) -> impl IntoElement {
        let cfg = config::ConfigStore::open()
            .unwrap_or_else(|_| config::ConfigStore::with_paths("", ""));
        let catalog = crate::ui::dialogs::active_catalog(&cfg);
        let title = about::title(catalog);
        let body = about::body_lines(catalog);
        let links: Vec<(String, &'static str)> = about::links()
            .iter()
            .map(|l| (about::link_label(catalog, l), l.url))
            .collect();

        let mut col = div().flex().flex_col().gap_3().p_6().child(title);
        for line in body {
            col = col.child(div().child(line));
        }
        let mut row = div().flex().gap_2();
        for (i, (label, url)) in links.iter().enumerate() {
            let url = *url;
            row = row.child(
                Button::new(("about-link", i)).label(label.clone()).on_click(
                    cx.listener(move |_, _, _, cx| {
                        cx.open_url(url);
                    }),
                ),
            );
        }
        col.child(row)
    }
}

pub mod downloads {
    use gpui_kit::{div, prelude::*};

    /// Downloads screen stub (ticket 22).
    #[derive(Default)]
    pub struct DownloadsScreen;

    impl DownloadsScreen {
        pub fn render(&mut self) -> impl IntoElement {
            div().p_6().child("Downloads")
        }
    }
}

pub mod favorites {
    use gpui_kit::{div, prelude::*};

    /// Favorites screen stub.
    #[derive(Default)]
    pub struct FavoritesScreen;

    impl FavoritesScreen {
        pub fn render(&mut self) -> impl IntoElement {
            div().p_6().child("Favorites")
        }
    }
}

pub mod settings {
    use gpui_kit::component::button::Button;
    use gpui_kit::{div, prelude::*};

    /// Settings screen stub (ticket 24 fills the real cards).
    #[derive(Default)]
    pub struct SettingsScreen;

    impl SettingsScreen {
        pub fn render(&mut self, _window: &mut gpui_kit::Window, cx: &mut gpui_kit::Context<crate::ui::Shell>) -> impl IntoElement {
            // ponytail: two buttons stand in for T24's ContentWidget
            // cards so the style dialogs (ticket 25) are reachable.
            let cfg = config::ConfigStore::open()
                .unwrap_or_else(|_| config::ConfigStore::with_paths("", ""));
            let cfg_d = config::ConfigStore::open()
                .unwrap_or_else(|_| config::ConfigStore::with_paths("", ""));
            div()
                .p_6()
                .flex()
                .gap_2()
                .child(
                    Button::new("danmaku-style")
                        .label(crate::ui::dialogs::styles::danmaku_caption(
                            crate::ui::dialogs::active_catalog(&cfg),
                        ))
                        .on_click(cx.listener(move |_, _, window, cx| {
                            let cfg = config::ConfigStore::open()
                                .unwrap_or_else(|_| config::ConfigStore::with_paths("", ""));
                            crate::ui::dialogs::styles::open_danmaku_style(cfg, window, cx);
                        })),
                )
                .child(
                    Button::new("subtitle-style")
                        .label(crate::ui::dialogs::styles::subtitle_caption(
                            crate::ui::dialogs::active_catalog(&cfg_d),
                        ))
                        .on_click(cx.listener(move |_, _, window, cx| {
                            let cfg = config::ConfigStore::open()
                                .unwrap_or_else(|_| config::ConfigStore::with_paths("", ""));
                            crate::ui::dialogs::styles::open_subtitle_style(cfg, window, cx);
                        })),
                )
        }
    }
}

/// Screen states owned by the shell (rendering mutates them).
#[derive(Default)]
pub struct Screens {
    pub downloads: downloads::DownloadsScreen,
    pub favorites: favorites::FavoritesScreen,
    pub settings: settings::SettingsScreen,
}
