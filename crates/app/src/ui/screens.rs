//! Screens: parse (in `super::parse`), downloads (in `super::downloads`),
//! favorites, settings.

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
