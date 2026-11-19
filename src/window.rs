//! Spry's own window: the page in the system's web view (WebView2 on Windows, WebKit on macOS),
//! so Spry looks and behaves like an app instead of a tab in the browser.
//!
//! The window is a process of its own, `spry window`, started by the running Spry (tray.rs). A web
//! view that was loaded once stays in memory: inside Spry itself it took 20 MB more for good, even
//! with the window closed (measured on macOS, 19 Nov 2026). In its own process all of it is given
//! back the moment the window closes, and the Spry that holds the apps back stays as small as before.
//!
//! The two are tied by the window's standard input: a line on it means "come to the front", and its
//! end means Spry has stopped, so the window closes too.

use crate::{config, platform, server};
use std::io::BufRead;
use tao::dpi::LogicalSize;
use tao::event::{Event, StartCause, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tao::window::{Window, WindowBuilder};
use wry::{NewWindowResponse, WebContext, WebView, WebViewBuilder};

/// Exit code of `spry window` when this computer has no web view to show the page in.
pub const NO_WEB_VIEW: i32 = 3;

/// What the running Spry tells its window.
enum Told {
    ComeToFront,
    SpryStopped,
}

/// True for the addresses the window may show itself: Spry's own page and nothing else.
pub fn is_own_page(url: &str, port: u16) -> bool {
    url.starts_with(&format!("http://127.0.0.1:{port}/"))
}

/// A link that leaves Spry's page (GitHub, a new release) belongs in the browser, not in the window.
fn hand_to_browser(url: &str) {
    if url.starts_with("https://") || url.starts_with("http://") {
        platform::open_url(url);
    }
}

/// The menu bar of the window on macOS: without one, Cmd+Q, Cmd+W and copy and paste do nothing.
/// Returns the menu (it must stay alive) and its "Quit Spry" item.
#[cfg(target_os = "macos")]
fn menu_bar() -> Option<(tray_icon::menu::Menu, tray_icon::menu::MenuItem)> {
    use tray_icon::menu::accelerator::{Accelerator, Code, Modifiers};
    use tray_icon::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
    let quit = MenuItem::new("Quit Spry", true, Some(Accelerator::new(Modifiers::META, Code::KeyQ)));
    let app = Submenu::with_items(
        "Spry",
        true,
        &[
            &PredefinedMenuItem::hide(None),
            &PredefinedMenuItem::hide_others(None),
            &PredefinedMenuItem::show_all(None),
            &PredefinedMenuItem::separator(),
            &quit,
        ],
    )
    .ok()?;
    let edit = Submenu::with_items(
        "Edit",
        true,
        &[
            &PredefinedMenuItem::undo(None),
            &PredefinedMenuItem::redo(None),
            &PredefinedMenuItem::separator(),
            &PredefinedMenuItem::cut(None),
            &PredefinedMenuItem::copy(None),
            &PredefinedMenuItem::paste(None),
            &PredefinedMenuItem::select_all(None),
        ],
    )
    .ok()?;
    let window = Submenu::with_items(
        "Window",
        true,
        &[&PredefinedMenuItem::minimize(None), &PredefinedMenuItem::close_window(None)],
    )
    .ok()?;
    let menu = Menu::with_items(&[&app, &edit, &window]).ok()?;
    menu.init_for_nsapp();
    Some((menu, quit))
}

/// Shows the page until the window is closed or Spry stops. Never returns.
pub fn run() -> ! {
    #[allow(unused_mut)]
    let mut event_loop = EventLoopBuilder::<Told>::with_user_event().build();
    #[cfg(target_os = "macos")]
    {
        // Spry.app starts as a menu-bar icon with no place in the Dock. Its window has one.
        use tao::platform::macos::{ActivationPolicy, EventLoopExtMacOS};
        event_loop.set_activation_policy(ActivationPolicy::Regular);
    }

    let tell = event_loop.create_proxy();
    std::thread::spawn(move || {
        let mut line = String::new();
        loop {
            line.clear();
            let told = match std::io::stdin().lock().read_line(&mut line) {
                Ok(n) if n > 0 => Told::ComeToFront,
                _ => Told::SpryStopped,
            };
            let stopped = matches!(told, Told::SpryStopped);
            if tell.send_event(told).is_err() || stopped {
                return;
            }
        }
    });

    let port = config::port();
    let token = config::token();
    // The web view keeps its own files (cache, the light or dark choice) next to Spry's settings.
    let mut context = WebContext::new(Some(config::data_dir().join("webview")));
    let mut page: Option<(WebView, Window)> = None;
    #[cfg(target_os = "macos")]
    let mut bar = None;

    event_loop.run(move |event, target, control_flow| {
        *control_flow = ControlFlow::Wait;
        match event {
            Event::NewEvents(StartCause::Init) => {
                #[cfg(target_os = "macos")]
                {
                    bar = menu_bar();
                }
                #[allow(unused_mut)]
                let mut builder = WindowBuilder::new()
                    .with_title("Spry")
                    .with_inner_size(LogicalSize::new(1120.0, 780.0))
                    .with_min_inner_size(LogicalSize::new(400.0, 480.0));
                #[cfg(windows)]
                {
                    // The icon build.rs put into spry.exe, so the taskbar and the title bar show Spry's own.
                    use tao::platform::windows::{IconExtWindows, WindowBuilderExtWindows};
                    let icon = tao::window::Icon::from_resource(1, None).ok();
                    builder = builder.with_window_icon(icon.clone()).with_taskbar_icon(icon);
                }
                let made = builder.build(target).ok().and_then(|window| {
                    let view = WebViewBuilder::new_with_web_context(&mut context)
                        .with_url(server::url(&token))
                        .with_navigation_handler(move |url| {
                            let own = is_own_page(&url, port);
                            if !own {
                                hand_to_browser(&url);
                            }
                            own
                        })
                        .with_new_window_req_handler(|url, _| {
                            hand_to_browser(&url);
                            NewWindowResponse::Deny
                        })
                        .build(&window)
                        .ok()?;
                    Some((view, window))
                });
                match made {
                    Some((view, window)) => {
                        window.set_focus();
                        page = Some((view, window));
                    }
                    // No web view on this computer: Spry opens the page in the browser instead.
                    None => std::process::exit(NO_WEB_VIEW),
                }
            }
            Event::UserEvent(Told::ComeToFront) => {
                if let Some((_, window)) = &page {
                    window.set_minimized(false);
                    window.set_focus();
                }
            }
            // Closing the window does not stop Spry: it goes on as the tray or menu-bar icon.
            Event::UserEvent(Told::SpryStopped) | Event::WindowEvent { event: WindowEvent::CloseRequested, .. } => {
                page = None;
                *control_flow = ControlFlow::Exit;
            }
            _ => {}
        }

        #[cfg(target_os = "macos")]
        while let Ok(event) = tray_icon::menu::MenuEvent::receiver().try_recv() {
            if bar.as_ref().is_some_and(|(_, quit)| event.id == *quit.id()) {
                // "Quit Spry" means all of Spry: every app is put back, and this window closes with it.
                let _ = server::send("POST", "/api/quit");
                page = None;
                *control_flow = ControlFlow::Exit;
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::is_own_page;

    #[test]
    fn the_window_shows_spry_s_own_page_and_hands_every_other_address_to_the_browser() {
        assert!(is_own_page("http://127.0.0.1:47633/?t=abc#apps", 47633));
        assert!(is_own_page("http://127.0.0.1:47633/fonts/geist.woff2?t=abc", 47633));
        assert!(!is_own_page("https://github.com/juw12/spry", 47633));
        assert!(!is_own_page("http://127.0.0.1:47633.example.com/", 47633));
        assert!(!is_own_page("http://127.0.0.1:8080/", 47633));
        assert!(!is_own_page("http://localhost:47633/", 47633));
        assert!(!is_own_page("file:///etc/passwd", 47633));
    }
}
