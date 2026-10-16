//! Login dialog (ticket 23): QR + SMS/GeeTest + cookie paste — port of
//! `src/gui/dialog/login.py` on the `crates/auth` stack.
//!
//! - `model` — gpui-free state machine (headless tests)
//! - `cid` — the 215-entry calling-code table (`cid_list.py`)
//! - `qrcode_png` — QR pixmap painter (`util/auth/qrcode.py`)
//! - `server` — loopback GeeTest server (`util/auth/server.py`)
//! - `captcha_page` — the embedded `captcha.html` + browser open
//!   (`util/misc/web.py`)
//! - `controller` — network flows with injectable endpoints (wiremock
//!   seam; `auth::login`'s consts are the defaults)
//! - `screen` — the gpui render layer

pub mod captcha_page;
pub mod cid;
pub mod controller;
pub mod model;
pub mod qrcode_png;
pub mod screen;
pub mod server;

pub use screen::LoginState;
