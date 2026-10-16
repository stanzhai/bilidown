//! URL recognition layer for bilidown: dispatch bilibili links to their
//! Parser Type, lift raw ids, handle search keywords, expand short links.
//!
//! - [`classify`]: order-sensitive dispatch table + raw-id extraction.
//! - [`search`]: server-side search keywords (space/favlist/history/watch_later).
//! - [`expand`]: b23.tv short links and festival pages (needs a reqwest
//!   client; the real network crate lands with ticket 04).
pub mod classify;
pub mod expand;
pub mod search;
