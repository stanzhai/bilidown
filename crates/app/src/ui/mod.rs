//! App UI modules: navigation shell + screens.

pub mod parse;
pub mod screens;

use gpui_kit::component::sidebar::{Sidebar, SidebarMenuItem};
use gpui_kit::{Context, Render, Window, div, prelude::*};

use parse::ParseScreen;

/// Which screen the shell shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Parse,
    Downloads,
    Favorites,
    About,
    Settings,
}

/// The main window navigation shell: sidebar + current screen
/// (port of `src/gui/interface/main_window.py`).
pub struct Shell {
    pub screen: Screen,
    pub parse: ParseScreen,
    pub screens: screens::Screens,
}

impl Shell {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            screen: Screen::Parse,
            parse: ParseScreen::new(window, cx),
            screens: screens::Screens::default(),
        }
    }
}

impl Render for Shell {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        use gpui_kit::assets::IconName;

        let screen = self.screen;
        div()
            .size_full()
            .flex()
            .child(
                Sidebar::new("nav").children(
                    [
                        (Screen::Parse, "Parse", IconName::Search),
                        (Screen::Downloads, "Downloads", IconName::Download),
                        (Screen::Favorites, "Favorites", IconName::Star),
                        (Screen::About, "About", IconName::Info),
                        (Screen::Settings, "Settings", IconName::Settings),
                    ]
                    .map(|(s, label, icon)| {
                        SidebarMenuItem::new(label)
                            .icon(icon)
                            .active(screen == s)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.screen = s;
                                cx.notify();
                            }))
                    }),
                ),
            )
            .child(match screen {
                Screen::Parse => self.parse.render(cx).into_any_element(),
                Screen::Downloads => self.screens.downloads.render().into_any_element(),
                Screen::Favorites => self.screens.favorites.render().into_any_element(),
                Screen::About => screens::about::render_about().into_any_element(),
                Screen::Settings => self.screens.settings.render().into_any_element(),
            })
    }
}
