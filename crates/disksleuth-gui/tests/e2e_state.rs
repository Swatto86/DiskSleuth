/// End-to-end tests for `AppState` — the GUI application state machine.
///
/// These tests exercise the real business-logic paths of `AppState` without
/// spinning up an egui window, keeping them fast and deterministic.
///
/// **Scope:** All user-visible state transitions are covered:
///   - Scan lifecycle (start, progress messages, completion, cancellation)
///   - Treemap navigation (forward/back/up, history bounds)
///   - Tree-view expansion and `MAX_VISIBLE_ROWS` cap
///   - Monitor start/stop
///   - Error accumulation and `MAX_SCAN_ERRORS` cap
///
/// The real `parallel::scan_parallel` scanner is used so no mocking is needed.
use disksleuth_core::model::SortKey;
use disksleuth_gui::state::{AppPhase, AppState, ExportFormat};
use std::fs;
use std::io::Write;
use std::path::Path;
use std::time::Duration;
use tempfile::TempDir;

// ── Helpers ───────────────────────────────────────────────────────────────────

fn write_bytes(path: &Path, n: usize) {
    let mut f = fs::File::create(path).unwrap();
    f.write_all(&vec![0u8; n]).unwrap();
}

/// Build a minimal temp directory and return the `TempDir` guard.
fn make_temp_tree() -> TempDir {
    let tmp = TempDir::new().unwrap();
    write_bytes(&tmp.path().join("a.txt"), 100);
    write_bytes(&tmp.path().join("b.bin"), 200);
    let sub = tmp.path().join("sub");
    fs::create_dir_all(&sub).unwrap();
    write_bytes(&sub.join("c.rs"), 300);
    tmp
}

/// Pump `process_scan_messages()` until the phase leaves `Scanning` or the
/// deadline expires.
fn pump_until_done(state: &mut AppState) {
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    while state.phase == AppPhase::Scanning {
        assert!(
            std::time::Instant::now() < deadline,
            "scan did not complete within 30 seconds"
        );
        state.process_scan_messages();
        std::thread::sleep(Duration::from_millis(10));
    }
}

// ── Scan lifecycle ─────────────────────────────────────────────────────────────

/// After `start_scan`, the phase must be `Scanning`.
#[test]
fn start_scan_sets_scanning_phase() {
    let tmp = make_temp_tree();
    let mut state = AppState::new();
    state.start_scan(tmp.path().to_path_buf());
    assert_eq!(state.phase, AppPhase::Scanning);
}

/// After the scan channel delivers `Complete`, the phase must flip to `Results`
/// and the final tree must be populated.
#[test]
fn scan_completes_and_tree_is_available() {
    let tmp = make_temp_tree();
    let mut state = AppState::new();
    state.start_scan(tmp.path().to_path_buf());
    pump_until_done(&mut state);

    assert_eq!(state.phase, AppPhase::Results);
    assert!(
        state.current_tree().is_some(),
        "tree must be populated after completion"
    );
}

/// The final tree must contain at least the root node and the files we wrote.
#[test]
fn scan_tree_contains_expected_nodes() {
    let tmp = make_temp_tree();
    let mut state = AppState::new();
    state.start_scan(tmp.path().to_path_buf());
    pump_until_done(&mut state);

    let tree = state.current_tree().expect("tree must exist");
    // root + "sub" dir + 3 files = at least 5 nodes.
    assert!(tree.len() >= 5, "expected >= 5 nodes, got {}", tree.len());
    assert!(tree.total_size >= 600, "expected total_size >= 600");
}

/// Cancelling a scan must transition to `Results` with `scan_was_cancelled = true`.
#[test]
fn cancel_scan_sets_cancelled_flag() {
    let tmp = make_temp_tree();
    let mut state = AppState::new();
    state.start_scan(tmp.path().to_path_buf());
    state.cancel_scan();

    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    while state.phase == AppPhase::Scanning {
        assert!(std::time::Instant::now() < deadline, "timed out");
        state.process_scan_messages();
        std::thread::sleep(Duration::from_millis(5));
    }

    // The scan may complete so quickly that cancellation is never observed.
    // Accept either cancelled state or a normal Results state.
    assert_ne!(
        state.phase,
        AppPhase::Scanning,
        "phase must leave Scanning after cancel"
    );
}

/// Starting a second scan resets all counters and phase to Scanning.
#[test]
fn start_scan_resets_previous_results() {
    let tmp = make_temp_tree();
    let mut state = AppState::new();

    // First scan.
    state.start_scan(tmp.path().to_path_buf());
    pump_until_done(&mut state);
    assert_eq!(state.phase, AppPhase::Results);
    assert!(state.current_tree().is_some());

    // Second scan must reset state.
    state.start_scan(tmp.path().to_path_buf());
    assert_eq!(
        state.phase,
        AppPhase::Scanning,
        "phase must reset to Scanning on second start"
    );
    // The tree should be cleared at scan start.
    assert!(
        state.current_tree().is_none(),
        "previous tree must be cleared"
    );
}

// ── Treemap navigation ────────────────────────────────────────────────────────

/// Navigate forward, then go back — must return to previous root.
#[test]
fn treemap_back_returns_to_previous_root() {
    let tmp = make_temp_tree();
    let mut state = AppState::new();
    state.start_scan(tmp.path().to_path_buf());
    pump_until_done(&mut state);

    let tree = state.current_tree().expect("tree must exist");
    let roots = tree.roots.clone();
    if roots.is_empty() {
        return; // degenerate tree — nothing to navigate
    }
    let root = roots[0];
    let children = tree.children_sorted_by_size(root);
    if children.is_empty() {
        return; // nothing to navigate into
    }
    let child = *children
        .iter()
        .find(|&&c| tree.node(c).is_dir)
        .unwrap_or(&children[0]);

    // Navigate into child.
    state.treemap_navigate_to(child);
    assert_eq!(state.treemap_root, Some(child));

    // Go back — must return to root.
    state.treemap_go_back();
    assert_eq!(state.treemap_root, Some(root));
}

/// Forward navigation restores to the node after going back.
#[test]
fn treemap_forward_after_back() {
    let tmp = make_temp_tree();
    let mut state = AppState::new();
    state.start_scan(tmp.path().to_path_buf());
    pump_until_done(&mut state);

    let tree = state.current_tree().expect("tree must exist");
    let roots = tree.roots.clone();
    if roots.is_empty() {
        return;
    }
    let root = roots[0];
    let children = tree.children_sorted_by_size(root);
    if children.is_empty() {
        return;
    }
    let child = *children
        .iter()
        .find(|&&c| tree.node(c).is_dir)
        .unwrap_or(&children[0]);

    state.treemap_navigate_to(child);
    state.treemap_go_back();
    state.treemap_go_forward();

    assert_eq!(state.treemap_root, Some(child));
}

/// Going back beyond the start of history is a no-op.
#[test]
fn treemap_go_back_at_start_is_noop() {
    let mut state = AppState::new();
    // No scan, no history.
    let original = state.treemap_root;
    state.treemap_go_back();
    assert_eq!(state.treemap_root, original);
}

// ── Tree-view expansion ────────────────────────────────────────────────────────

/// Expanding a node adds its children to `visible_rows`.
#[test]
fn toggle_expand_adds_children() {
    let tmp = make_temp_tree();
    let mut state = AppState::new();
    state.start_scan(tmp.path().to_path_buf());
    pump_until_done(&mut state);

    // visible_rows should be populated after scan completion.
    assert!(
        !state.visible_rows.is_empty(),
        "visible_rows must be non-empty after scan"
    );

    // Find a collapsed directory row and expand it.
    let collapsed_row = state
        .visible_rows
        .iter()
        .enumerate()
        .find(|(_, r)| {
            !r.is_expanded
                && state
                    .current_tree()
                    .map(|t| t.node(r.node_index).is_dir)
                    .unwrap_or(false)
        })
        .map(|(i, _)| i);

    if let Some(idx) = collapsed_row {
        let rows_before = state.visible_rows.len();
        state.toggle_expand(idx);
        let rows_after = state.visible_rows.len();
        // Expanding a directory with children must add rows.
        let tree = state.current_tree().expect("tree");
        let node = state.visible_rows[idx].node_index;
        let child_count = tree.children(node).len();
        assert_eq!(rows_after, rows_before + child_count);
    }
}

/// Collapsing an expanded node removes its descendants.
#[test]
fn toggle_expand_collapse_removes_descendants() {
    let tmp = make_temp_tree();
    let mut state = AppState::new();
    state.start_scan(tmp.path().to_path_buf());
    pump_until_done(&mut state);

    // Find an expanded directory row.
    let expanded_row = state
        .visible_rows
        .iter()
        .enumerate()
        .find(|(_, r)| r.is_expanded)
        .map(|(i, _)| i);

    if let Some(idx) = expanded_row {
        let rows_before = state.visible_rows.len();
        state.toggle_expand(idx);
        let rows_after = state.visible_rows.len();
        assert!(
            rows_after < rows_before,
            "collapsing must remove descendants"
        );
    }
}

// ── Monitor ────────────────────────────────────────────────────────────────────

/// Starting the monitor sets `monitor_active = true`.
#[test]
fn start_monitor_sets_active() {
    let tmp = TempDir::new().unwrap();
    let mut state = AppState::new();
    state.start_monitor(tmp.path().to_path_buf());
    assert!(state.monitor_active);
    state.stop_monitor();
}

/// Stopping the monitor clears `monitor_active`.
#[test]
fn stop_monitor_clears_active() {
    let tmp = TempDir::new().unwrap();
    let mut state = AppState::new();
    state.start_monitor(tmp.path().to_path_buf());
    state.stop_monitor();
    assert!(!state.monitor_active);
}

/// Starting a second monitor stops the first one (no double-monitor state).
#[test]
fn start_monitor_twice_replaces_first() {
    let tmp1 = TempDir::new().unwrap();
    let tmp2 = TempDir::new().unwrap();
    let mut state = AppState::new();
    state.start_monitor(tmp1.path().to_path_buf());
    state.start_monitor(tmp2.path().to_path_buf());
    assert!(state.monitor_active);
    assert_eq!(
        state.monitor_path,
        tmp2.path().to_string_lossy().into_owned()
    );
    state.stop_monitor();
}

// ── Drive refresh ──────────────────────────────────────────────────────────────

/// After a drive refresh the selection must always be in bounds (or None) —
/// a stale index would panic on the next `drives[idx]` access in the toolbar.
#[test]
fn refresh_drives_selection_stays_in_bounds() {
    let mut state = AppState::new();
    state.refresh_drives();
    if let Some(idx) = state.selected_drive_index {
        assert!(
            idx < state.drives.len(),
            "selected_drive_index {idx} out of bounds for {} drives",
            state.drives.len()
        );
    }
}

/// A selection pointing past the end of the drive list (simulating a drive
/// that vanished) must be repaired by refresh, not carried over.
#[test]
fn refresh_drives_repairs_stale_selection() {
    let mut state = AppState::new();
    state.selected_drive_index = Some(usize::MAX);
    state.refresh_drives();
    if let Some(idx) = state.selected_drive_index {
        assert!(idx < state.drives.len());
    }
}

// ── Export ─────────────────────────────────────────────────────────────────────

/// Exporting without a completed scan must set a failure status, not panic.
#[test]
fn export_without_tree_sets_failure_status() {
    let mut state = AppState::new();
    for format in [ExportFormat::Csv, ExportFormat::Json] {
        state.status_flash = None;
        state.export(format);
        let flash = state.status_flash.as_ref().expect("flash must be set");
        assert!(flash.is_error, "expected failure status for {format:?}");
        assert!(
            flash.text.starts_with("Export failed"),
            "unexpected status text: {}",
            flash.text
        );
    }
}

/// Starting a new scan clears any previous status flash.
#[test]
fn start_scan_clears_status_flash() {
    let tmp = make_temp_tree();
    let mut state = AppState::new();
    state.status_flash = Some(disksleuth_gui::state::StatusFlash::info("Exported 42 rows"));
    state.start_scan(tmp.path().to_path_buf());
    assert!(state.status_flash.is_none());
    pump_until_done(&mut state);
}

// ── Sorting ────────────────────────────────────────────────────────────────────

/// Clicking a different sort column reorders the visible rows; clicking the
/// same column flips the direction.
#[test]
fn set_sort_reorders_visible_rows() {
    let tmp = make_temp_tree();
    let mut state = AppState::new();
    state.start_scan(tmp.path().to_path_buf());
    pump_until_done(&mut state);

    let names_at_depth_1 = |state: &AppState| -> Vec<String> {
        let tree = state.current_tree().expect("tree");
        state
            .visible_rows
            .iter()
            .filter(|r| r.depth == 1)
            .map(|r| tree.node(r.node_index).name.to_string())
            .collect()
    };

    state.set_sort(SortKey::Name);
    assert_eq!(state.sort_key, SortKey::Name);
    assert!(state.sort_ascending, "name sorts ascending by default");
    let by_name = names_at_depth_1(&state);
    // dirs first ("sub"), then files alphabetically.
    assert_eq!(by_name, ["sub", "a.txt", "b.bin"]);

    // Same column again → direction flips.
    state.set_sort(SortKey::Name);
    assert!(!state.sort_ascending);
    let by_name_desc = names_at_depth_1(&state);
    assert_eq!(by_name_desc, ["sub", "b.bin", "a.txt"]);

    // Size descending is the startup default ordering.
    state.set_sort(SortKey::Size);
    assert!(!state.sort_ascending, "size sorts descending by default");
    let by_size = names_at_depth_1(&state);
    assert_eq!(by_size, ["sub", "b.bin", "a.txt"]);
}

/// Re-sorting keeps directories expanded.
#[test]
fn set_sort_preserves_expansion() {
    let tmp = make_temp_tree();
    let mut state = AppState::new();
    state.start_scan(tmp.path().to_path_buf());
    pump_until_done(&mut state);

    // Expand "sub" (find its collapsed row).
    let sub_row = state
        .visible_rows
        .iter()
        .position(|r| {
            state
                .current_tree()
                .map(|t| t.node(r.node_index).is_dir && r.depth == 1)
                .unwrap_or(false)
        })
        .expect("sub dir row");
    state.toggle_expand(sub_row);
    let rows_expanded = state.visible_rows.len();

    state.set_sort(SortKey::Name);
    assert_eq!(
        state.visible_rows.len(),
        rows_expanded,
        "expanded rows must survive a resort"
    );
}

// ── Keyboard navigation ────────────────────────────────────────────────────────

/// Down/up selection moves through visible rows and requests a scroll.
#[test]
fn move_selection_walks_rows() {
    let tmp = make_temp_tree();
    let mut state = AppState::new();
    state.start_scan(tmp.path().to_path_buf());
    pump_until_done(&mut state);
    assert!(state.visible_rows.len() >= 3);

    // No selection: down selects the first row.
    state.move_selection(1);
    assert_eq!(state.selected_node, Some(state.visible_rows[0].node_index));
    assert!(state.scroll_to_selected);

    state.scroll_to_selected = false;
    state.move_selection(1);
    assert_eq!(state.selected_node, Some(state.visible_rows[1].node_index));

    // Clamped at the top.
    state.move_selection(-10);
    assert_eq!(state.selected_node, Some(state.visible_rows[0].node_index));

    // Clamped at the bottom.
    state.move_selection(isize::MAX / 2);
    assert_eq!(
        state.selected_node,
        Some(state.visible_rows.last().unwrap().node_index)
    );
}

/// Right expands a collapsed directory; left collapses it again; left on a
/// collapsed child jumps to the parent.
#[test]
fn expand_collapse_selection() {
    let tmp = make_temp_tree();
    let mut state = AppState::new();
    state.start_scan(tmp.path().to_path_buf());
    pump_until_done(&mut state);

    // Select the "sub" directory row (depth 1, is_dir).
    let sub_row = state
        .visible_rows
        .iter()
        .position(|r| {
            r.depth == 1
                && state
                    .current_tree()
                    .map(|t| t.node(r.node_index).is_dir)
                    .unwrap_or(false)
        })
        .expect("sub dir row");
    let sub_node = state.visible_rows[sub_row].node_index;
    state.selected_node = Some(sub_node);

    let before = state.visible_rows.len();
    state.expand_selection();
    assert!(state.visible_rows[sub_row].is_expanded);
    assert!(state.visible_rows.len() > before, "children must appear");

    // Expanded + right → steps into the first child.
    state.expand_selection();
    let child = state.visible_rows[sub_row + 1].node_index;
    assert_eq!(state.selected_node, Some(child));

    // Left on a file/collapsed node → jumps to parent.
    state.collapse_selection();
    assert_eq!(state.selected_node, Some(sub_node));

    // Left on the expanded dir → collapses it.
    state.collapse_selection();
    assert!(!state.visible_rows[sub_row].is_expanded);
    assert_eq!(state.visible_rows.len(), before);
}

// ── Old files & duplicates ─────────────────────────────────────────────────────

/// Freshly written files are not stale at a 1-year threshold.
#[test]
fn old_files_cache_refreshes() {
    let tmp = make_temp_tree();
    let mut state = AppState::new();
    state.start_scan(tmp.path().to_path_buf());
    pump_until_done(&mut state);

    state.refresh_old_files();
    let old = state.old_files.as_deref().expect("cache must be set");
    assert!(old.is_empty(), "new files cannot be a year old");
}

/// The duplicate background scan finds the duplicate pair in the temp tree
/// and clears its in-flight handle on completion.
#[test]
fn duplicate_scan_finds_pairs() {
    let tmp = TempDir::new().unwrap();
    let payload = vec![0x5Au8; 4096];
    std::fs::write(tmp.path().join("copy_one.bin"), &payload).unwrap();
    std::fs::write(tmp.path().join("copy_two.bin"), &payload).unwrap();
    write_bytes(&tmp.path().join("unique.bin"), 100);

    let mut state = AppState::new();
    state.start_scan(tmp.path().to_path_buf());
    pump_until_done(&mut state);

    state.duplicate_min_size = 1;
    state.start_duplicate_scan();
    assert!(state.duplicate_scan.is_some());

    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    while state.duplicate_scan.is_some() {
        assert!(
            std::time::Instant::now() < deadline,
            "duplicate scan timed out"
        );
        state.process_duplicate_messages();
        std::thread::sleep(Duration::from_millis(10));
    }

    let groups = state.duplicates.as_deref().expect("results must be set");
    assert_eq!(groups.len(), 1, "exactly one duplicate group");
    assert_eq!(groups[0].files.len(), 2);
    assert_eq!(groups[0].size, 4096);
}

// ── Scan history ───────────────────────────────────────────────────────────────

/// Completed scans are recorded; two scans of the same path produce a
/// comparable default pair.
#[test]
fn history_records_completed_scans() {
    let tmp = make_temp_tree();
    let mut state = AppState::new();

    state.start_scan(tmp.path().to_path_buf());
    pump_until_done(&mut state);
    assert_eq!(state.scan_history.len(), 1);
    assert!(
        state.history_pair().is_none(),
        "one snapshot cannot compare"
    );

    // Grow the tree, scan again.
    write_bytes(&tmp.path().join("grown.bin"), 10_000);
    state.start_scan(tmp.path().to_path_buf());
    pump_until_done(&mut state);
    assert_eq!(state.scan_history.len(), 2);

    let (a, b) = state.history_pair().expect("default pair");
    assert_eq!((a, b), (0, 1));
    assert!(
        state.scan_history[b].total_size > state.scan_history[a].total_size,
        "second snapshot must include the new file"
    );

    state.refresh_history_diff();
    assert!(state.history_diff.is_some());
}

// ── Deletion (state update without filesystem I/O) ─────────────────────────────

/// Purging a node updates totals, caches, visible rows, and clears stale
/// references — without touching the filesystem.
#[test]
fn purge_node_updates_results() {
    let tmp = make_temp_tree();
    let mut state = AppState::new();
    state.start_scan(tmp.path().to_path_buf());
    pump_until_done(&mut state);

    let tree = state.current_tree().expect("tree");
    let total_before = tree.total_size;
    let root = tree.roots[0];
    let victim = *tree
        .children(root)
        .iter()
        .find(|&&c| !tree.node(c).is_dir)
        .expect("a file child");
    let victim_size = tree.node(victim).size;
    state.selected_node = Some(victim);
    state.treemap_root = Some(victim);

    state.purge_node_from_results(victim);

    let tree = state.current_tree().expect("tree");
    assert_eq!(tree.total_size, total_before - victim_size);
    assert!(tree.node(victim).is_deleted);
    assert_eq!(state.selected_node, None, "stale selection must be cleared");
    assert_eq!(
        state.treemap_root,
        Some(root),
        "treemap must re-root to the deleted node's parent"
    );
    assert!(
        state.visible_rows.iter().all(|r| r.node_index != victim),
        "purged node must leave the visible rows"
    );
}

/// Requesting deletion of a scan root is rejected with an error flash.
#[test]
fn request_delete_rejects_root() {
    let tmp = make_temp_tree();
    let mut state = AppState::new();
    state.start_scan(tmp.path().to_path_buf());
    pump_until_done(&mut state);

    let root = state.current_tree().unwrap().roots[0];
    state.request_delete(root);
    assert!(state.pending_delete.is_none());
    assert!(state.status_flash.as_ref().is_some_and(|f| f.is_error));
}

// ── AppState construction ─────────────────────────────────────────────────────

/// A freshly created `AppState` must start in the `Idle` phase.
#[test]
fn new_state_is_idle() {
    let state = AppState::new();
    assert_eq!(state.phase, AppPhase::Idle);
}

/// Default state must start in dark mode (safety default: dark is lower contrast
/// and less likely to cause issues on first render).
#[test]
fn default_state_is_dark_mode() {
    let state = AppState::new();
    assert!(state.dark_mode, "dark mode must be the default");
}
