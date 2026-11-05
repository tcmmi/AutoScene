//! macOS-only native menu bar, compiled everywhere so its words stay checked.
//!
//! Windows/Linux keep the custom frameless chrome (`TitleBar.tsx`) and no
//! menu. Items that need app state emit [`MenuAction`] with their id; the
//! webview dispatches it (see `useGlobalBindings` in `app/src/lib/actions.ts`,
//! which has a handler for every id below). Predefined items
//! (copy/paste/quit/…) are handled by AppKit and never produce an event.
//!
//! Every entry takes its words from [`LABELS`], in the language AutoScene
//! speaks: a change of language builds the menu again ([`rebuild`]), and the
//! keys the person gave it go back on. What AppKit adds by itself (Start
//! Dictation, Emoji & Symbols, Enter Full Screen) follows the macOS language,
//! through `CFBundleLocalizations` in `Info.plist`.

use std::collections::HashMap;
use std::sync::LazyLock;

use parking_lot::Mutex;
use tauri::menu::{
    AboutMetadataBuilder, HELP_SUBMENU_ID, Menu, MenuBuilder, MenuEvent, MenuItemBuilder, SubmenuBuilder,
    WINDOW_SUBMENU_ID,
};
use tauri::{AppHandle, Runtime};
use tauri_specta::Event;

use crate::events::MenuAction;
use crate::native_text::Words;

/// Every entry of the menu and the key of its words: submenus, the items of
/// the system (`system:`) and AutoScene's own, by the ids the webview knows.
pub const LABELS: &[(&str, &str)] = &[
    ("menu:app", "menu.app.title"),
    ("system:about", "menu.app.about"),
    ("app:check-updates", "menu.app.checkUpdates"),
    ("app:settings", "menu.app.settings"),
    ("system:services", "menu.app.services"),
    ("system:hide", "menu.app.hide"),
    ("system:hide-others", "menu.app.hideOthers"),
    ("system:show-all", "menu.app.showAll"),
    ("system:quit", "menu.app.quit"),
    ("menu:file", "menu.file.title"),
    ("file:new-project", "menu.file.newProject"),
    ("file:add-media", "menu.file.addMedia"),
    ("file:export", "menu.file.export"),
    ("file:reveal", "menu.file.reveal"),
    ("menu:edit", "menu.edit.title"),
    ("system:undo", "menu.edit.undo"),
    ("system:redo", "menu.edit.redo"),
    ("system:cut", "menu.edit.cut"),
    ("system:copy", "menu.edit.copy"),
    ("system:paste", "menu.edit.paste"),
    ("system:select-all", "menu.edit.selectAll"),
    ("edit:copy-timecodes", "menu.edit.copyTimecodes"),
    ("menu:view", "menu.view.title"),
    ("view:command-palette", "menu.view.commandPalette"),
    ("view:library", "menu.view.library"),
    ("view:episodes", "menu.view.episodes"),
    ("view:exports", "menu.view.exports"),
    ("view:activity", "menu.view.activity"),
    ("view:plugins", "menu.view.plugins"),
    ("view:toggle-sidebar", "menu.view.toggleSidebar"),
    ("system:full-screen", "menu.view.fullScreen"),
    ("menu:window", "menu.window.title"),
    ("system:minimize", "menu.window.minimize"),
    ("system:zoom", "menu.window.zoom"),
    ("system:close-window", "menu.window.close"),
    ("menu:help", "menu.help.title"),
    ("help:logs", "menu.help.logs"),
];

/// What the entry `id` of [`LABELS`] is called in `words`.
fn label(words: &Words, id: &str) -> String {
    match LABELS.iter().find(|(entry, _)| *entry == id) {
        Some((_, key)) => words.say(key, &[]),
        None => id.to_owned(),
    }
}

fn item<R: Runtime>(
    app: &AppHandle<R>,
    words: &Words,
    id: &str,
    accel: Option<&str>,
) -> tauri::Result<tauri::menu::MenuItem<R>> {
    let mut b = MenuItemBuilder::with_id(id, label(words, id));
    if let Some(a) = accel {
        b = b.accelerator(a);
    }
    b.build(app)
}

pub fn build<R: Runtime>(app: &AppHandle<R>, words: &Words) -> tauri::Result<Menu<R>> {
    let w = words;
    let about = AboutMetadataBuilder::new()
        .name(Some("AutoScene"))
        .version(Some(app.package_info().version.to_string()))
        .build();

    let app_menu = SubmenuBuilder::new(app, label(w, "menu:app"))
        .about_with_text(label(w, "system:about"), Some(about))
        .separator()
        .item(&item(app, w, "app:check-updates", None)?)
        .separator()
        .item(&item(app, w, "app:settings", Some("CmdOrCtrl+,"))?)
        .separator()
        .services_with_text(label(w, "system:services"))
        .separator()
        .hide_with_text(label(w, "system:hide"))
        .hide_others_with_text(label(w, "system:hide-others"))
        .show_all_with_text(label(w, "system:show-all"))
        .separator()
        .quit_with_text(label(w, "system:quit"))
        .build()?;

    let file_menu = SubmenuBuilder::new(app, label(w, "menu:file"))
        .item(&item(app, w, "file:new-project", Some("CmdOrCtrl+N"))?)
        .item(&item(app, w, "file:add-media", Some("CmdOrCtrl+O"))?)
        .separator()
        .item(&item(app, w, "file:export", Some("CmdOrCtrl+E"))?)
        .separator()
        .item(&item(app, w, "file:reveal", Some("Shift+CmdOrCtrl+R"))?)
        .build()?;

    let edit_menu = SubmenuBuilder::new(app, label(w, "menu:edit"))
        .undo_with_text(label(w, "system:undo"))
        .redo_with_text(label(w, "system:redo"))
        .separator()
        .cut_with_text(label(w, "system:cut"))
        .copy_with_text(label(w, "system:copy"))
        .paste_with_text(label(w, "system:paste"))
        .select_all_with_text(label(w, "system:select-all"))
        .separator()
        .item(&item(app, w, "edit:copy-timecodes", Some("Shift+CmdOrCtrl+C"))?)
        .build()?;

    let view_menu = SubmenuBuilder::new(app, label(w, "menu:view"))
        .item(&item(app, w, "view:command-palette", Some("CmdOrCtrl+K"))?)
        .separator()
        .item(&item(app, w, "view:library", Some("CmdOrCtrl+1"))?)
        .item(&item(app, w, "view:episodes", Some("CmdOrCtrl+2"))?)
        .item(&item(app, w, "view:exports", Some("CmdOrCtrl+3"))?)
        .separator()
        .item(&item(app, w, "view:activity", Some("CmdOrCtrl+J"))?)
        .item(&item(app, w, "view:plugins", None)?)
        .item(&item(app, w, "view:toggle-sidebar", Some("CmdOrCtrl+B"))?)
        .separator()
        .fullscreen_with_text(label(w, "system:full-screen"))
        .build()?;

    // These ids make Tauri register the window list and the Help search field.
    let window_menu = SubmenuBuilder::with_id(app, WINDOW_SUBMENU_ID, label(w, "menu:window"))
        .minimize_with_text(label(w, "system:minimize"))
        .maximize_with_text(label(w, "system:zoom"))
        .separator()
        .close_window_with_text(label(w, "system:close-window"))
        .build()?;

    let help_menu = SubmenuBuilder::with_id(app, HELP_SUBMENU_ID, label(w, "menu:help"))
        .item(&item(app, w, "help:logs", None)?)
        .build()?;

    MenuBuilder::new(app).items(&[&app_menu, &file_menu, &edit_menu, &view_menu, &window_menu, &help_menu]).build()
}

/// The keys the person gave the menu so far, each item's last: a menu built
/// again knows only its own.
#[derive(Default)]
struct Given(Mutex<HashMap<String, String>>);

impl Given {
    fn add(&self, keys: &HashMap<String, String>) {
        self.0.lock().extend(keys.iter().map(|(id, accel)| (id.clone(), accel.clone())));
    }

    /// Swaps in a menu built again with `swap`, then hands `put` the keys
    /// given until then. Nothing is held meanwhile: `menu_set_keys` runs on
    /// the main thread, which building the menu waits for. Keys it gives
    /// during the swap reach `put`, and so do keys it gives during `put`,
    /// which that older `put` may have overwritten on the new menu.
    fn across<E>(
        &self,
        swap: impl FnOnce() -> Result<(), E>,
        mut put: impl FnMut(&HashMap<String, String>) -> Result<(), E>,
    ) -> Result<(), E> {
        swap()?;
        let mut given = self.0.lock().clone();
        loop {
            put(&given)?;
            let now = self.0.lock().clone();
            if now == given {
                return Ok(());
            }
            given = now;
        }
    }
}

static GIVEN: LazyLock<Given> = LazyLock::new(Given::default);

/// Keeps `keys` for the next menu, then has `put` give them to this one.
fn give<E>(
    keys: &HashMap<String, String>,
    put: impl FnOnce(&HashMap<String, String>) -> Result<(), E>,
) -> Result<(), E> {
    GIVEN.add(keys);
    put(keys)
}

/// Give the items named in `keys` the person's accelerators (`""`: none).
/// Settings → Shortcuts sends those of the items a shortcut shares a key with,
/// and none while it records a key (or the menu would take it first).
pub fn set_keys<R: Runtime>(app: &AppHandle<R>, keys: &HashMap<String, String>) -> tauri::Result<()> {
    give(keys, |keys| put_keys(app, keys))
}

fn put_keys<R: Runtime>(app: &AppHandle<R>, keys: &HashMap<String, String>) -> tauri::Result<()> {
    let Some(menu) = app.menu() else { return Ok(()) };
    for top in menu.items()? {
        let Some(sub) = top.as_submenu() else { continue };
        for item in sub.items()? {
            let Some(item) = item.as_menuitem() else { continue };
            let id: &str = item.id().as_ref();
            if let Some(accel) = keys.get(id) {
                item.set_accelerator((!accel.is_empty()).then_some(accel))?;
            }
        }
    }
    Ok(())
}

/// Build the menu again in `words`, with the person's keys on it.
pub fn rebuild<R: Runtime>(app: &AppHandle<R>, words: &Words) -> tauri::Result<()> {
    GIVEN.across(|| app.set_menu(build(app, words)?).map(|_| ()), |keys| put_keys(app, keys))
}

pub fn on_event<R: Runtime>(app: &AppHandle<R>, event: MenuEvent) {
    let id: &str = event.id().as_ref();
    if id == "help:logs" {
        // Handled in Rust — no IPC round-trip needed. The system's `open`,
        // never one found on PATH.
        let _ = std::process::Command::new("/usr/bin/open").arg(as_core::AppPaths::resolve().logs()).spawn();
        return;
    }
    if let Err(e) = MenuAction(id.to_string()).emit(app) {
        log::warn!("menu emit failed for {id}: {e}");
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::HashSet;

    use super::*;
    use crate::native_text::tests::{english, fixture};

    /// The function `name` of this file, as written.
    fn source_of(name: &str) -> &'static str {
        let source = include_str!("mac_menu.rs");
        let start = source.find(&format!("pub fn {name}<R")).unwrap();
        &source[start..start + source[start..].find("\n}\n").unwrap()]
    }

    /// `build`, as written.
    fn build_source() -> &'static str {
        source_of("build")
    }

    /// The ids `build` names, in `"prefix:name"` strings.
    fn built_ids() -> HashSet<&'static str> {
        let quoted = build_source().split('"').skip(1).step_by(2);
        quoted.filter(|s| s.contains(':') && !s.contains('+')).collect()
    }

    #[test]
    fn every_entry_of_the_menu_has_a_label_and_every_label_an_entry() {
        let labelled: HashSet<&str> = LABELS.iter().map(|(id, _)| *id).collect();
        assert_eq!(labelled.len(), LABELS.len(), "an id twice");
        assert_eq!(built_ids(), labelled);
        for (id, key) in LABELS {
            assert_ne!(label(english(), id), *key, "{id} has no words");
        }
    }

    #[test]
    fn the_menu_says_what_it_said_in_english() {
        let said: HashMap<&str, String> = LABELS.iter().map(|(id, _)| (*id, label(english(), id))).collect();
        for (id, words) in [
            ("menu:app", "AutoScene"),
            ("system:about", "About AutoScene"),
            ("app:check-updates", "Check for Updates…"),
            ("app:settings", "Settings…"),
            ("system:services", "Services"),
            ("system:hide", "Hide AutoScene"),
            ("system:hide-others", "Hide Others"),
            ("system:show-all", "Show All"),
            ("system:quit", "Quit AutoScene"),
            ("menu:file", "File"),
            ("file:new-project", "New Project…"),
            ("file:add-media", "Add Episodes…"),
            ("file:export", "Export Clip…"),
            ("file:reveal", "Reveal in Finder"),
            ("menu:edit", "Edit"),
            ("system:undo", "Undo"),
            ("system:redo", "Redo"),
            ("system:cut", "Cut"),
            ("system:copy", "Copy"),
            ("system:paste", "Paste"),
            ("system:select-all", "Select All"),
            ("edit:copy-timecodes", "Copy Timecodes"),
            ("menu:view", "View"),
            ("view:command-palette", "Command Palette…"),
            ("view:library", "Library"),
            ("view:episodes", "Episodes"),
            ("view:exports", "Exports"),
            ("view:activity", "Activity"),
            ("view:plugins", "Plugins"),
            ("view:toggle-sidebar", "Toggle Sidebar"),
            ("system:full-screen", "Toggle Full Screen"),
            ("menu:window", "Window"),
            ("system:minimize", "Minimize"),
            ("system:zoom", "Zoom"),
            ("system:close-window", "Close Window"),
            ("menu:help", "Help"),
            ("help:logs", "Show Logs in Finder"),
        ] {
            assert_eq!(said[id], words, "{id}");
        }
    }

    #[test]
    fn no_item_of_the_system_keeps_its_english() {
        for bare in [
            ".about(",
            ".services(",
            ".hide(",
            ".hide_others(",
            ".show_all(",
            ".quit(",
            ".undo(",
            ".redo(",
            ".cut(",
            ".copy(",
            ".paste(",
            ".select_all(",
            ".fullscreen(",
            ".minimize(",
            ".maximize(",
            ".close_window(",
        ] {
            assert!(!build_source().contains(bare), "{bare}");
        }
    }

    #[test]
    fn a_label_follows_the_language() {
        let romanian = fixture(r#"{"menu": {"file": {"title": "Fișier"}, "edit": {"undo": "Anulează"}}}"#);
        assert_eq!(label(romanian, "menu:file"), "Fișier");
        assert_eq!(label(romanian, "system:undo"), "Anulează");
        assert_eq!(label(romanian, "system:redo"), "Redo");
    }

    #[test]
    fn the_menu_speaks_romanian() {
        let romanian = crate::native_text::words(as_api::Lang::Ro);
        let said: HashMap<&str, String> = LABELS.iter().map(|(id, _)| (*id, label(romanian, id))).collect();
        for (id, words) in [
            ("menu:app", "AutoScene"),
            ("system:about", "Despre AutoScene"),
            ("app:check-updates", "Caută actualizări…"),
            ("app:settings", "Setări…"),
            ("system:services", "Servicii"),
            ("system:hide", "Ascunde AutoScene"),
            ("system:hide-others", "Ascunde celelalte"),
            ("system:show-all", "Arată tot"),
            ("system:quit", "Ieși din AutoScene"),
            ("menu:file", "Fișier"),
            ("file:new-project", "Proiect nou…"),
            ("file:add-media", "Adaugă episoade…"),
            ("file:export", "Exportă clipul…"),
            ("file:reveal", "Arată în Finder"),
            ("menu:edit", "Editare"),
            ("system:undo", "Anulează"),
            ("system:redo", "Refă"),
            ("system:cut", "Taie"),
            ("system:copy", "Copiază"),
            ("system:paste", "Lipește"),
            ("system:select-all", "Selectează tot"),
            ("edit:copy-timecodes", "Copiază timecode-urile"),
            ("menu:view", "Afișare"),
            ("view:command-palette", "Paleta de comenzi…"),
            ("view:library", "Bibliotecă"),
            ("view:episodes", "Episoade"),
            ("view:exports", "Exporturi"),
            ("view:activity", "Activitate"),
            ("view:plugins", "Pluginuri"),
            ("view:toggle-sidebar", "Arată/ascunde bara laterală"),
            ("system:full-screen", "Ecran complet"),
            ("menu:window", "Fereastră"),
            ("system:minimize", "Minimizează"),
            ("system:zoom", "Zoom"),
            ("system:close-window", "Închide fereastra"),
            ("menu:help", "Ajutor"),
            ("help:logs", "Arată jurnalele în Finder"),
        ] {
            assert_eq!(said[id], words, "{id}");
        }
    }

    fn keys(given: &[(&str, &str)]) -> HashMap<String, String> {
        given.iter().map(|(id, accel)| ((*id).to_owned(), (*accel).to_owned())).collect()
    }

    #[test]
    fn the_persons_keys_survive_a_rebuild() {
        let given = Given::default();
        given.add(&keys(&[("view:library", "CmdOrCtrl+L"), ("file:export", "")]));
        given.add(&keys(&[("view:library", "CmdOrCtrl+Y"), ("view:activity", "")]));

        let steps = RefCell::new(Vec::new());
        let swap = || {
            steps.borrow_mut().push("swapped");
            Ok::<_, ()>(())
        };
        let mut put = None;
        given
            .across(swap, |keys| {
                steps.borrow_mut().push("put back");
                put = Some(keys.clone());
                Ok(())
            })
            .unwrap();
        assert_eq!(*steps.borrow(), ["swapped", "put back"]);
        // Each item's last, and an item that was given no key keeps none.
        assert_eq!(put.unwrap(), keys(&[("view:library", "CmdOrCtrl+Y"), ("file:export", ""), ("view:activity", "")]));

        // A menu that couldn't be built leaves the keys for the next time.
        let failed = given.across(|| Err("no menu"), |_| panic!("nothing to put them on"));
        assert_eq!(failed, Err("no menu"));
        assert_eq!(given.0.lock().len(), 3);
    }

    #[test]
    fn keys_given_during_a_rebuild_stay_on_the_new_menu() {
        let given = Given::default();
        given.add(&keys(&[("view:library", "CmdOrCtrl+L")]));

        // The person's key lands while the older ones go on: they overwrite
        // it on the new menu, so it goes on again after them.
        let mut put = Vec::new();
        let result: Result<(), ()> = given.across(
            || Ok(()),
            |held| {
                put.push(held["view:library"].clone());
                given.add(&keys(&[("view:library", "CmdOrCtrl+Y")]));
                Ok(())
            },
        );
        result.unwrap();
        assert_eq!(put, ["CmdOrCtrl+L", "CmdOrCtrl+Y"]);
    }

    /// `set_keys` and `rebuild` need a Mac's menu, so what they hand over is
    /// read from their source; `give` and `across` do the rest, through the
    /// store they share.
    #[test]
    fn keys_given_through_set_keys_reach_the_next_menu() {
        assert!(source_of("set_keys").contains("give(keys, |keys| put_keys(app, keys))"));
        assert!(source_of("rebuild").contains("GIVEN.across("));
        assert!(source_of("rebuild").contains("|keys| put_keys(app, keys)"));

        give(&keys(&[("help:logs", "CmdOrCtrl+Shift+L")]), |_| Ok::<_, ()>(())).unwrap();

        let mut put = None;
        let result: Result<(), ()> = GIVEN.across(
            || Ok(()),
            |held| {
                put = Some(held.clone());
                Ok(())
            },
        );
        result.unwrap();
        assert_eq!(put.unwrap()["help:logs"], "CmdOrCtrl+Shift+L");
    }
}
