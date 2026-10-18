//! Screens: parse (in `super::parse`), downloads (in `super::downloads`),
//! favorites, settings, about.

pub mod about {
    use gpui_kit::component::button::Button;
    use gpui_kit::{div, prelude::*, Context, ParentElement};

    use crate::ui::dialogs::about;

    /// About screen (port of `gui/dialog/main_window/about.py`).
    /// Content and i18n live in `ui::dialogs::about`; this renders it.
    pub fn render_about(cx: &mut Context<crate::ui::Shell>) -> impl IntoElement {
        let cfg =
            config::ConfigStore::open().unwrap_or_else(|_| config::ConfigStore::with_paths("", ""));
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
                Button::new(("about-link", i))
                    .label(label.clone())
                    .on_click(cx.listener(move |_, _, _, cx| {
                        cx.open_url(url);
                    })),
            );
        }
        col.child(row)
    }
}

pub mod favorites {
    use gpui_kit::{div, prelude::*};

    /// Favorites screen stub.
    #[derive(Default)]
    pub struct FavoritesScreen;

    impl FavoritesScreen {
        pub fn render(&mut self, cx: &Context<crate::ui::Shell>) -> impl IntoElement {
            let lang = cx
                .read_global::<crate::ui::ShellI18n, _>(|i18n, _| i18n.lang)
                .resolve();
            let label = services::i18n::catalog(lang)
                .tr("MainWindow", "Favorites")
                .to_owned();
            div().p_6().child(label)
        }
    }
}

/// Screen states owned by the shell (rendering mutates them).
pub struct Screens {
    pub favorites: favorites::FavoritesScreen,
    pub settings: super::settings::SettingsScreen,
}
