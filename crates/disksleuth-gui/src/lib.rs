/// DiskSleuth GUI — egui-based desktop frontend.
///
/// This crate contains all UI code. Business logic lives in `disksleuth-core`.
pub mod app;
pub mod icon;
pub mod panels;
pub mod state;
pub mod widgets;

pub use app::{DiskSleuthApp, DiskSleuthState};
