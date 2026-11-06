//! The monospace fonts Raptor ships with, registered with Pango at startup so
//! the terminal looks the same whether or not they are installed.

use gtk::pango;
use gtk::prelude::*;

pub const FAMILIES: &[&str] = &["JetBrains Mono", "Fira Code", "Cascadia Code", "Iosevka Term", "IBM Plex Mono", "Source Code Pro"];

/// Where the bundled .ttf files live: next to an installed binary
/// (`../share/raptor/fonts`), or in the source tree for `cargo run`.
fn dirs() -> Vec<std::path::PathBuf> {
    let mut v = Vec::new();
    if let Ok(d) = std::env::var("RAPTOR_FONTS_DIR") {
        v.push(d.into());
    }
    if let Some(prefix) = std::env::current_exe().ok().and_then(|e| e.parent()?.parent().map(|p| p.to_path_buf())) {
        v.push(prefix.join("share/raptor/fonts"));
    }
    v.push(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/fonts").into());
    v
}

pub fn register() {
    let map = pangocairo::FontMap::default();
    let Some(dir) = dirs().into_iter().find(|d| d.is_dir()) else { return };
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let path = e.path();
        if path.extension().is_some_and(|x| x == "ttf") {
            let _ = map.add_font_file(&path);
        }
    }
}

/// Monospace families installed on the system, besides the bundled ones.
pub fn installed(ctx: &pango::Context) -> Vec<String> {
    let mut v: Vec<String> = ctx
        .list_families()
        .iter()
        .filter(|f| f.is_monospace())
        .map(|f| f.name().to_string())
        .filter(|n| !FAMILIES.contains(&n.as_str()) && n != "Monospace" && !n.contains("Emoji"))
        .collect();
    v.sort();
    v.dedup();
    v
}
