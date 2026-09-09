//! Headless UI tests.
//!
//! egui is a pure immediate-mode library, so a panel can be run against a
//! bare `egui::Context` with no window and the text it paints inspected. That
//! is enough to prove a control exists and that state reaches the screen.

use disksleuth_gui::state::AppState;

/// Run two frames (the first lays windows out, the second paints them at
/// their final size) and return every string painted on the last one.
fn painted_text(mut draw: impl FnMut(&egui::Context)) -> String {
    let ctx = egui::Context::default();
    let _ = ctx.run(egui::RawInput::default(), &mut draw);
    let output = ctx.run(egui::RawInput::default(), &mut draw);

    let mut out = String::new();
    for cp in output.shapes {
        collect(&cp.shape, &mut out);
    }
    out
}

fn collect(shape: &egui::epaint::Shape, out: &mut String) {
    match shape {
        egui::epaint::Shape::Text(t) => {
            out.push_str(t.galley.text());
            out.push('\n');
        }
        egui::epaint::Shape::Vec(v) => v.iter().for_each(|s| collect(s, out)),
        _ => {}
    }
}

/// An `AppState` that looks like a finished scan which skipped two paths.
fn state_with_errors() -> AppState {
    let mut state = AppState::new();
    state.scan_error_count = 2;
    state.scan_errors = vec![
        (
            r"C:\System Volume Information".into(),
            "access denied".into(),
        ),
        (r"C:\pagefile.sys".into(), "in use".into()),
    ];
    state
}

/// The per-path scan errors must actually reach the screen — the status bar
/// only shows an aggregate count, which tells the user nothing about *what*
/// was skipped.
#[test]
fn errors_window_lists_the_skipped_paths() {
    let mut state = state_with_errors();
    state.show_errors = true;

    let text = painted_text(|ctx| {
        disksleuth_gui::panels::errors_window::errors_window(ctx, &mut state);
    });

    assert!(
        text.contains(r"C:\System Volume Information") && text.contains("access denied"),
        "the window must list each skipped path and its reason; painted:\n{text}"
    );
    assert!(text.contains(r"C:\pagefile.sys"));
}

/// The window stays closed until asked for.
#[test]
fn errors_window_is_hidden_when_not_requested() {
    let mut state = state_with_errors();
    state.show_errors = false;

    let text = painted_text(|ctx| {
        disksleuth_gui::panels::errors_window::errors_window(ctx, &mut state);
    });

    assert!(
        !text.contains(r"C:\pagefile.sys"),
        "nothing should be painted while show_errors is false"
    );
}

/// The sidebar must offer a way to open that window — otherwise the paths are
/// collected but unreachable, which is the defect this guards.
#[test]
fn scan_panel_offers_a_control_for_skipped_items() {
    let mut state = state_with_errors();

    let text = painted_text(|ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            disksleuth_gui::panels::scan_panel::scan_panel(ui, &mut state);
        });
    });

    assert!(
        text.contains("skipped items"),
        "sidebar must expose a control opening the skipped-items list; painted:\n{text}"
    );
}

/// With nothing skipped, no such control is shown.
#[test]
fn scan_panel_hides_the_control_when_nothing_was_skipped() {
    let mut state = AppState::new();

    let text = painted_text(|ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            disksleuth_gui::panels::scan_panel::scan_panel(ui, &mut state);
        });
    });

    assert!(!text.contains("skipped items"));
}
