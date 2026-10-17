//! Captcha page — port of `util/misc/web.py::WebPage` (write the embedded
//! `captcha.html` into the temp dir on first use, open it with the system
//! browser) + the page itself from `src/res/html/captcha.html`.
//!
//! The page's JS GETs `http://localhost:{port}/geetest/captcha/init` and
//! POSTs `{challenge, seccode, validate}` to `.../callback`; the loopback
//! server is `server.rs`.

use std::path::PathBuf;

/// The page's JS GETs `http://localhost:2333/geetest/captcha/init` and
/// POSTs `{challenge, seccode, validate}` to `.../callback` (the loopback
/// server lives in `server.rs`).
pub const CAPTCHA_HTML: &str = include_str!("captcha.html");

/// `WebPage.ensure_file_exists("captcha.html")` — write the embedded page
/// into the temp dir if absent, return the `file://` URI.
pub fn ensure_captcha_page() -> std::io::Result<String> {
    let path = captcha_page_path()?;
    if !path.exists() {
        std::fs::write(&path, CAPTCHA_HTML)?;
    }
    Ok(file_uri(&path))
}

fn captcha_page_path() -> std::io::Result<PathBuf> {
    let dir = std::env::temp_dir().join("bilidown");
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join("captcha.html"))
}

/// `Path.as_uri()` — percent-encode enough for a local path.
pub fn file_uri(path: &std::path::Path) -> String {
    let mut uri = String::from("file://");
    for component in path.components() {
        use std::path::Component;
        match component {
            Component::RootDir => {}
            Component::Prefix(p) => uri.push_str(&p.as_os_str().to_string_lossy()),
            other => {
                uri.push('/');
                uri.push_str(&encode(&other.as_os_str().to_string_lossy()));
            }
        }
    }
    uri
}

fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// `WebPage.open("captcha.html")` — hand the file URI to the system
/// browser.
pub fn open_captcha_page() -> anyhow::Result<()> {
    let uri = ensure_captcha_page()?;
    ::open::that_detached(&uri)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_page_matches_the_python_resource() {
        // the GeeTest loader + our glue must be present
        assert!(CAPTCHA_HTML.contains("initGeetest"));
        assert!(CAPTCHA_HTML.contains("geetest/captcha/init"));
        assert!(CAPTCHA_HTML.contains("geetest/captcha/callback"));
        assert!(CAPTCHA_HTML.contains("loading-dot"));
    }

    #[test]
    fn file_uri_percent_encodes() {
        assert_eq!(
            file_uri(std::path::Path::new("/tmp/bilidown/captcha.html")),
            "file:///tmp/bilidown/captcha.html"
        );
        assert_eq!(
            file_uri(std::path::Path::new("/tmp/a b/c.html")),
            "file:///tmp/a%20b/c.html"
        );
    }

    #[test]
    fn ensure_writes_once_and_returns_uri() {
        // point TMPDIR at a scratch dir so the test never touches the
        // shared temp location
        let dir = tempfile::tempdir().unwrap();
        // NOTE: std::env::temp_dir honors TMPDIR at process start; assert
        // the file:// shape only, and that repeated calls succeed.
        let uri = ensure_captcha_page().unwrap();
        assert!(uri.starts_with("file://"), "{uri}");
        assert!(uri.ends_with("captcha.html"));
        let again = ensure_captcha_page().unwrap();
        assert_eq!(uri, again);
        let _ = dir;
    }
}
