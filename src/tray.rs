//! The tray (Windows) or menu-bar (macOS) icon.

use crate::engine::Shared;
use crate::{platform, server};
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

/// Runs the icon on the main thread until Spry quits. `worker` is the engine thread;
/// it is joined before the process ends so every process gets put back.
pub fn run(shared: Arc<Shared>, worker: JoinHandle<()>) -> ! {
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
                return;
            }
            _ => {}
        }

        while let Ok(event) = MenuEvent::receiver().try_recv() {
            if event.id == *open.id() {
                platform::open_url(&server::url(&shared.token));
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
                platform::open_url(&server::url(&shared.token));
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
            tray = None;
            *control_flow = ControlFlow::Exit;
        }
    })
}
