//! Screens: parse (in `super::parse`), downloads, favorites, settings,
//! about.

pub mod about {
    use gpui_kit::{div, prelude::*};

    pub fn render_about() -> impl IntoElement {
        div().p_6().child(crate::version_line())
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
    use gpui_kit::{div, prelude::*};

    /// Settings screen stub (ticket 24).
    #[derive(Default)]
    pub struct SettingsScreen;

    impl SettingsScreen {
        pub fn render(&mut self) -> impl IntoElement {
            div().p_6().child("Settings")
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
