//! About dialog content (port of `src/gui/dialog/main_window/about.py`).
//!
//! Data + i18n only; the screen in `ui/screens.rs` renders it, so the
//! layout lives with the other screens.

/// A labeled link button of the about footer.
#[derive(Debug, Clone)]
pub struct AboutLink {
    pub source: &'static str,
    pub url: &'static str,
}

pub const DOC_URL: &str = "https://bili23.scott-sloan.cn/doc/introduction.html";
pub const GITHUB_URL: &str = "https://github.com/ScottSloan/Bili23-Downloader";
pub const SPONSOR_URL: &str = "https://bili23.scott-sloan.cn/doc/about.html";

/// The footer links in Python's order: terms, privacy, documentation,
/// github, sponsor (terms/privacy open the in-app pages; the urls are
/// the doc-site fallbacks).
pub fn links() -> [AboutLink; 5] {
    [
        AboutLink {
            source: "Terms of Use",
            url: DOC_URL,
        },
        AboutLink {
            source: "Privacy Policy",
            url: DOC_URL,
        },
        AboutLink {
            source: "Documentation",
            url: DOC_URL,
        },
        AboutLink {
            source: "Github",
            url: GITHUB_URL,
        },
        AboutLink {
            source: "Sponsor",
            url: SPONSOR_URL,
        },
    ]
}

/// `{year}` of the copyright line (dynamic, per Python
/// `datetime.now().year`).
pub fn copyright_year() -> i32 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| 1970 + (d.as_secs() / 31_557_600) as i32)
        .unwrap_or(2026)
}

const CTX: &str = "AboutDialog";

/// Localized "About {app_name}" title.
pub fn title(c: &services::i18n::Catalog) -> String {
    c.tr(CTX, "About {app_name}")
        .replace("{app_name}", "Bili23 Downloader")
}

/// The localized body lines: version, license, copyright, sponsor
/// note (Python `tr()` order).
pub fn body_lines(c: &services::i18n::Catalog) -> [String; 4] {
    [
        c.tr(CTX, "Version {app_version}")
            .replace("{app_version}", crate::ui::dialogs::update::app_version()),
        c.tr(CTX, "This software is free and open-source, licensed under the GNU General Public License v3 (GPLv3).").to_owned(),
        c.tr(CTX, "Copyright © 2022-{year} Scott Sloan. All Rights Reserved.")
            .replace("{year}", &copyright_year().to_string()),
        c.tr(CTX, "If this project saved you time or solved your problem, consider buying the author a coffee! Don't forget to star the repository on GitHub to support open-source development.").to_owned(),
    ]
}

/// Localized link label.
pub fn link_label(c: &services::i18n::Catalog, link: &AboutLink) -> String {
    c.tr(CTX, link.source).to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use services::i18n::{catalog, ResolvedLanguage};

    #[test]
    fn title_and_version_fill_placeholders() {
        let c = catalog(ResolvedLanguage::ZhCn);
        assert_eq!(title(c), "关于 Bili23 Downloader");
        let body = body_lines(c);
        assert!(body[0].contains(crate::ui::dialogs::update::app_version()));
        assert!(body[2].contains("2022-"));
        assert!(body[2].contains(&copyright_year().to_string()));
    }

    #[test]
    fn links_cover_the_five_actions() {
        let ls = links();
        assert_eq!(ls.len(), 5);
        assert_eq!(ls[3].url, GITHUB_URL);
        assert_eq!(ls[4].url, SPONSOR_URL);
    }

    #[test]
    fn link_labels_translate() {
        let c = catalog(ResolvedLanguage::ZhCn);
        assert_eq!(link_label(c, &links()[3]), "Github");
        assert_eq!(link_label(c, &links()[4]), "赞助作者");
    }
}
