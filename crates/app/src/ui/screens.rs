//! Screens: parse (in `super::parse`), downloads (in `super::downloads`),
//! favorites, settings, about.

pub mod about {
    use gpui_kit::{div, prelude::*};

    pub fn render_about() -> impl IntoElement {
        div().p_6().child(crate::version_line())
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

/// Screen states owned by the shell (rendering mutates them).
pub struct Screens {
    pub favorites: favorites::FavoritesScreen,
    pub settings: super::settings::SettingsScreen,
}
