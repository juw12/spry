//! The tray (Windows) or menu-bar (macOS) icon. It also looks after Spry's window, which is a
//! process of its own (window.rs): it starts it, brings it to the front, and closes it when Spry quits.

use crate::engine::Shared;
use crate::{platform, server, window};
use std::io::Write;
use std::process::{Child, Stdio};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use tao::event::{Event, StartCause};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIconBuilder};

const SIZE: u32 = 64;
const ICON_ON: &[u8] = include_bytes!("../assets/tray-on.rgba");
const ICON_OFF: &[u8] = include_bytes!("../assets/tray-off.rgba");

fn icon(active: bool) -> Option<Icon> {
    Icon::from_rgba(if active { ICON_ON } else { ICON_OFF }.to_vec(), SIZE, SIZE).ok()
}

/// Starts `spry window`. Its standard input stays open as long as Spry runs: see window.rs.
fn start_window() -> Option<Child> {
    let exe = std::env::current_exe().ok()?;
    platform::command(exe).arg("window").stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().ok()
}

/// Brings the open window to the front. False when it is gone.
fn to_front(window: &mut Child) -> bool {
    matches!(window.try_wait(), Ok(None)) && window.stdin.as_mut().is_some_and(|pipe| writeln!(pipe, "front").is_ok())
}

/// Ends the window's standard input, which closes it, and does not wait for it for long.
fn close_window(mut window: Child) {
    drop(window.stdin.take());
    let asked = Instant::now();
    while matches!(window.try_wait(), Ok(None)) {
        if asked.elapsed() > Duration::from_secs(2) {
            let _ = window.kill();
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = window.wait();
}

/// Runs the icon on the main thread until Spry quits. `worker` is the engine thread; it is joined
/// before the process ends so every process gets put back. `show` opens the window at the start
/// (a start by hand); at sign-in Spry starts as the icon alone.
pub fn run(shared: Arc<Shared>, worker: JoinHandle<()>, show: bool) -> ! {
    #[allow(unused_mut)]
    let mut event_loop = EventLoopBuilder::new().build();
    #[cfg(target_os = "macos")]
    {
        use tao::platform::macos::{ActivationPolicy, EventLoopExtMacOS};
        event_loop.set_activation_policy(ActivationPolicy::Accessory);
    }

    let menu = Menu::new();
    let open = MenuItem::new("Open Spry", true, None);
    let pause = MenuItem::new("Pause for 15 minutes", true, None);
    let quit = MenuItem::new("Quit Spry", true, None);
    let _ = menu.append_items(&[&open, &pause, &PredefinedMenuItem::separator(), &quit]);

    let mut tray = None;
    let mut worker = Some(worker);
    let mut shown: Option<(usize, bool)> = None;
    let mut page: Option<Child> = None;
    shared.window.store(true, Ordering::Relaxed);
    shared.show.store(show, Ordering::Relaxed);

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(500));
        match event {
            Event::NewEvents(StartCause::Init) => {
                let mut builder = TrayIconBuilder::new().with_menu(Box::new(menu.clone())).with_tooltip("Spry");
                if let Some(icon) = icon(true) {
                    builder = builder.with_icon(icon);
                }
                tray = builder.build().ok();
            }
            Event::LoopDestroyed => {
                shared.quit.store(true, Ordering::Relaxed);
                if let Some(w) = worker.take() {
                    let _ = w.join();
                }
                crate::update::relaunch(&shared, true);
                return;
            }
            _ => {}
        }

        while let Ok(event) = MenuEvent::receiver().try_recv() {
            if event.id == *open.id() {
                shared.show.store(true, Ordering::Relaxed);
            } else if event.id == *pause.id() {
                let mut until = shared.pause_until.lock().unwrap();
                *until = if until.is_some() { None } else { Some(Instant::now() + Duration::from_secs(900)) };
            } else if event.id == *quit.id() {
                shared.quit.store(true, Ordering::Relaxed);
            }
        }
        #[cfg(windows)]
        while let Ok(event) = tray_icon::TrayIconEvent::receiver().try_recv() {
            use tray_icon::{MouseButton, MouseButtonState, TrayIconEvent};
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                shared.show.store(true, Ordering::Relaxed);
            }
        }

        // A window that has ended: closed by its owner, or it could not be made at all.
        if let Some(code) = page.as_mut().and_then(|window| window.try_wait().ok().flatten()) {
            page = None;
            if code.code() == Some(window::NO_WEB_VIEW) && shared.window.swap(false, Ordering::Relaxed) {
                // No web view on this computer: from now on the page opens in the browser.
                platform::open_url(&server::url(&shared.token));
            }
        }
        // Someone asked for the window: the icon, a second start, `spry open`, or this start itself.
        if shared.show.swap(false, Ordering::Relaxed) && !shared.quit.load(Ordering::Relaxed) {
            if !shared.window.load(Ordering::Relaxed) {
                platform::open_url(&server::url(&shared.token));
            } else if !page.as_mut().is_some_and(to_front) {
                page = start_window();
                if page.is_none() {
                    shared.window.store(false, Ordering::Relaxed);
                    platform::open_url(&server::url(&shared.token));
                }
            }
        }

        let paused = shared.paused_secs() > 0;
        let active = !paused && shared.cfg.lock().unwrap().enabled;
        let held = shared.held.load(Ordering::Relaxed);
        if shown != Some((held, active)) {
            shown = Some((held, active));
            if let Some(tray) = &tray {
                let text = match (active, held) {
                    (false, _) => "Spry is paused".to_string(),
                    (true, 0) => "Spry: nothing needs holding back".to_string(),
                    (true, 1) => "Spry: holding back 1 background process".to_string(),
                    (true, n) => format!("Spry: holding back {n} background processes"),
                };
                let _ = tray.set_tooltip(Some(text));
                let _ = tray.set_icon(icon(active));
            }
            pause.set_text(if paused { "Resume now" } else { "Pause for 15 minutes" });
        }

        let engine_gone = worker.as_ref().is_some_and(|w| w.is_finished());
        if shared.quit.load(Ordering::Relaxed) || engine_gone {
            // The window closes with Spry: its standard input ends here.
            if let Some(window) = page.take() {
                shared.window_open.store(true, Ordering::Relaxed);
                close_window(window);
            }
            tray = None;
            *control_flow = ControlFlow::Exit;
        }
    })
}
