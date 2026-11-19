//! Spry keeps an old computer quick by holding back background apps,
//! so the app you are using gets the processor, the disk and the memory first.

pub mod bench;
pub mod clean;
pub mod config;
pub mod engine;
pub mod install;
pub mod platform;
pub mod policy;
pub mod reference;
pub mod server;
#[cfg(any(windows, target_os = "macos"))]
pub mod tray;
#[cfg(any(windows, target_os = "macos"))]
pub mod window;
