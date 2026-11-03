//! The main window. On Windows it is created here rather than from the
//! config (`create: false` in `tauri.windows.conf.json`): Windows 11 gets that
//! config's transparent window with Mica behind the rail; Windows 10 has no
//! Mica, so its window stays solid — a transparent one would show the desktop
//! wherever the page hasn't painted yet (start-up, live resize).
//!
//! Release builds also turn off WebView2's own keys and menu there.

use tauri::utils::config::Color;
use tauri::{App, Manager, WebviewWindow, WebviewWindowBuilder};

/// Create the main window unless the config already did (macOS, Linux).
pub fn create_main(app: &App) -> tauri::Result<()> {
    if app.get_webview_window("main").is_some() {
        return Ok(());
    }
    let Some(mut config) = app.config().app.windows.iter().find(|w| w.label == "main").cloned() else {
        return Ok(());
    };
    if !has_mica() {
        config.transparent = false;
        config.no_redirection_bitmap = false;
        config.window_effects = None;
        config.background_color = Some(Color(9, 9, 11, 255));
    }
    let window = WebviewWindowBuilder::from_config(app.handle(), &config)?.build()?;
    // While developing, F5 reloads and F12 opens the developer tools.
    if !cfg!(debug_assertions) {
        no_browser_keys(&window);
    }
    Ok(())
}

/// Windows 11 (build 22000 and up) draws Mica.
#[cfg(windows)]
fn has_mica() -> bool {
    use windows_sys::Wdk::System::SystemServices::RtlGetVersion;
    use windows_sys::Win32::System::SystemInformation::OSVERSIONINFOW;
    let mut v = OSVERSIONINFOW { dwOSVersionInfoSize: size_of::<OSVERSIONINFOW>() as u32, ..Default::default() };
    // SAFETY: `v` is a valid OSVERSIONINFOW whose size field is set, as the call requires.
    let status = unsafe { RtlGetVersion(&mut v) };
    status == 0 && v.dwBuildNumber >= 22000
}

/// Elsewhere the config decides.
#[cfg(not(windows))]
fn has_mica() -> bool {
    true
}

/// WebView2's own keys and menu: F5 and Ctrl+R reload the app, Ctrl+P prints
/// it, Ctrl+F finds in it, and a right-click shows the browser's menu. The
/// app's page cancels them itself (`browser-guards.ts`, `AppMenu`), but a
/// plugin page is a frame with a document of its own, whose keys and clicks
/// never pass through the app's page. So they go for the whole webview.
/// Copy, paste, undo and select all are editing keys and stay.
///
/// This runs right after `build`, before the first page starts to load:
/// WebView2 applies a later change only from the next top-level navigation,
/// which the app never makes. Even so, the very first key the webview gets
/// in a run still acts as a browser key; from the second one on, none does.
/// That one is WebView2's own: the setting made again before a new
/// navigation changes nothing for it.
#[cfg(windows)]
fn no_browser_keys(window: &WebviewWindow) {
    use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Settings3;
    use windows_core::Interface;
    let set = window.with_webview(|webview| {
        let Some(webview) = webview.downcast_ref::<tauri_runtime_wry::Webview>() else {
            log::warn!("turning off WebView2's browser keys failed: the webview isn't WebView2");
            return;
        };
        // SAFETY: COM calls on the webview's own thread, where `with_webview` runs this.
        let off = unsafe {
            webview.controller().CoreWebView2().and_then(|core| core.Settings()).and_then(|settings| {
                settings.SetAreDefaultContextMenusEnabled(false)?;
                settings.cast::<ICoreWebView2Settings3>()?.SetAreBrowserAcceleratorKeysEnabled(false)
            })
        };
        if let Err(e) = off {
            log::warn!("turning off WebView2's browser keys failed: {e}");
        }
    });
    if let Err(e) = set {
        log::warn!("turning off WebView2's browser keys failed: {e}");
    }
}

/// Elsewhere there is no WebView2.
#[cfg(not(windows))]
fn no_browser_keys(_: &WebviewWindow) {}

/// WebKitGTK's own menu, which a right-click the page didn't cancel shows:
/// its Reload, Back and Forward act on the whole app, also from a plugin
/// page's frame, whose page needn't load the SDK that cancels it. So it
/// never shows in release builds; the app's page draws its own menus.
#[cfg(target_os = "linux")]
pub fn no_browser_menu(window: &WebviewWindow) {
    use webkit2gtk::WebViewExt;
    let set = window.with_webview(|webview| {
        let Some(webview) = webview.downcast_ref::<tauri_runtime_wry::Webview>() else {
            log::warn!("turning off WebKit's menu failed: the webview isn't WebKitGTK");
            return;
        };
        // True: handled, so WebKit shows nothing.
        webview.inner().connect_context_menu(|_, _, _, _| true);
    });
    if let Err(e) = set {
        log::warn!("turning off WebKit's menu failed: {e}");
    }
}

/// WebView2's goes with its keys ([`no_browser_keys`]); WKWebView's stays.
#[cfg(not(target_os = "linux"))]
pub fn no_browser_menu(_: &WebviewWindow) {}
