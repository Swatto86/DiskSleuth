# DiskSleuth — Project Progress

**Author:** Swatto  
**Repository:** <https://github.com/Swatto/disksleuth>  
**License:** MIT  
**Last updated:** 2026-02-10

---

## Current State: Phase 1 MVP Complete, Phase 3 In Progress

The project builds with **zero errors, zero warnings**.

---

## Architecture

Multi-crate Rust workspace.

```
disksleuth/
├── Cargo.toml                          # Workspace root + binary crate
├── build.rs                            # Windows manifest + icon embedding
├── src/main.rs                         # Thin binary entry point
├── crates/
│   ├── disksleuth-core/                # Pure logic, zero UI deps
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── scanner/
│   │       │   ├── mod.rs              # start_scan() + ScanHandle
│   │       │   ├── parallel.rs         # jwalk/rayon parallel walker (Tier 2)
│   │       │   ├── mft.rs             # MFT stub (Phase 3)
│   │       │   └── progress.rs         # ScanProgress/ScanCommand enums
│   │       ├── model/
│   │       │   ├── mod.rs
│   │       │   ├── file_node.rs        # NodeIndex(u32) + FileNode struct
│   │       │   ├── file_tree.rs        # Arena-backed FileTree + aggregation
│   │       │   └── size.rs             # format_size(), format_count()
│   │       ├── analysis/
│   │       │   ├── mod.rs
│   │       │   ├── top_files.rs        # Top N largest files
│   │       │   ├── file_types.rs       # Extension categorisation + stats
│   │       │   ├── duplicates.rs       # Stub (Phase 2)
│   │       │   └── age.rs             # Stale file finder
│   │       └── platform/
│   │           ├── mod.rs
│   │           ├── drives.rs           # Windows drive enumeration
│   │           └── permissions.rs      # UAC elevation check
│   │
│   └── disksleuth-gui/                 # egui desktop frontend
│       └── src/
│           ├── lib.rs
│           ├── app.rs                  # eframe::App implementation
│           ├── state.rs                # AppState + VisibleRow + toggle_expand
│           ├── theme.rs                # Dark/light theme, colour palette
│           ├── widgets/
│           │   ├── mod.rs
│           │   ├── tree_view.rs        # Virtualised TreeView (painter-based)
│           │   ├── drive_picker.rs     # Drive selection with usage bars
│           │   ├── size_bar.rs         # Proportional size bar widget
│           │   ├── toolbar.rs          # Top action bar
│           │   ├── status_bar.rs       # Bottom progress/stats bar
│           │   └── treemap.rs          # Squarified treemap (replaced sunburst)
│           └── panels/
│               ├── mod.rs
│               ├── scan_panel.rs       # Left sidebar — drives + controls
│               ├── tree_panel.rs       # Centre — column headers + tree
│               ├── details_panel.rs    # Right sidebar — selected item info
│               └── chart_panel.rs      # File type breakdown bars
├── assets/
│   └── fonts/
└── README.md
```

## Key Dependencies

| Crate | Version | Purpose |
|-------|---------|---------|
| eframe/egui | 0.31 | GUI framework |
| jwalk | 0.8 | Parallel directory walking |
| rayon | 1.10 | Thread pool |
| crossbeam-channel | 0.5 | Scan→UI progress channel |
| compact_str | 0.8 | Small-string optimisation for file names |
| windows | 0.58 | Win32 API (drives, permissions) |
| parking_lot | 0.12 | Fast mutexes |
| thiserror | 2 | Typed errors in core |
| anyhow | 1 | Error handling in GUI/binary |
| tracing | 0.1 | Structured logging |

## What's Done (Phase 1)

- [x] Workspace structure with core/GUI separation
- [x] Arena-allocated file tree (`Vec<FileNode>` + `NodeIndex(u32)`)
- [x] O(n) bottom-up size aggregation (single reverse pass, no recursion)
- [x] Parallel directory scanner with jwalk + real-time progress via crossbeam
- [x] Cancellation support via `AtomicBool`
- [x] Windows drive enumeration (type, label, filesystem, space)
- [x] Admin elevation detection
- [x] Virtualised TreeView — only renders visible rows, O(1) render cost
- [x] Expand/collapse with flat VisibleRow list algorithm
- [x] Size bars with green→red gradient by percentage
- [x] Right-click context menu (Open in Explorer, Copy Path)
- [x] Column headers (Name, Size, %, Bar, Files)
- [x] Details panel for selected item
- [x] File type categorisation + breakdown chart
- [x] Top N largest files (pre-computed during aggregation)
- [x] Stale file finder (by age threshold)
- [x] Dark/light theme toggle with full colour palette
- [x] Status bar with live scan stats + duration
- [x] Toolbar with Scan/Stop/Refresh/Export/Theme controls
- [x] build.rs with Windows manifest (DPI awareness, elevation)
- [x] 9 unit tests passing (tree aggregation, path reconstruction, sorting, size formatting)

## What's Not Done

### Phase 2 — Analysis
- [ ] Export scan results to CSV/JSON (button exists, handler is a stub)
- [ ] Duplicate file detection (`analysis/duplicates.rs` is a stub)
- [ ] File type pie/donut chart (currently flat bar list)
- [ ] Age analysis panel integration (logic exists, no UI wiring)
- [ ] Keyboard navigation in TreeView (arrow keys, Enter to expand)
- [ ] Sort by column header click

### Phase 3 — Power Features
- [x] NTFS MFT direct reader (`scanner/mft.rs` — `FSCTL_ENUM_USN_DATA`, requires admin + NTFS)
- [x] SpaceSniffer-style treemap (`widgets/treemap.rs` — nested squarified layout with directory header bars, recursive up to 6 levels, click-to-navigate, back/forward/up, breadcrumb, labels, tooltips)
- [x] Cancelling a scan preserves partial results (tree + treemap remain visible)
- [x] Real-time live tree view during scanning (`LiveTree = Arc<RwLock<FileTree>>`)
- [x] Selection sync between tree view and treemap (single-click selects in both, double-click navigates/opens, `reveal_node_in_tree()` expands ancestors)
- [x] App launches maximized (fullscreen)
- [x] Tree view name truncation (proper font-metric clipping with ellipsis + hover tooltip for full name)
- [x] Removed duplicate "Scan Selected Drive" button (toolbar Scan button is the single entry point)
- [x] Auto-scan OS drive on startup (reads `%SystemDrive%`, defaults to `C:`)
- [x] Treemap navigation buttons fixed (back/forward/up now correctly track history from initial root state)
- [x] Access-denied handling (`FileNode.is_error` — error nodes shown in tree with ⚠ icon, warning colour, and tooltip; details panel shows "Access denied" badge)
- [x] Hidden & system files included (both scanners enumerate all files regardless of Windows attributes)
- [x] Symlink safety (parallel walker uses `symlink_metadata` + `follow_links(false)` — no loops, no double-counting)
- [x] MFT-to-parallel fallback (if MFT scan produces no results, automatically retries with Tier 2 parallel walk)
- [x] Scan tier indicator in status bar (shows "MFT" or "Walk" during and after scan; admin badge in toolbar)
- [ ] Scan history / comparison
- [ ] Custom folder scan (not just whole drives)
- [ ] File deletion with recycle bin support
- [ ] Vim-style keyboard navigation

## Known Design Decisions

1. **No `unwrap()` in production paths** — all errors are handled or propagated.
2. **Full paths are NOT stored per node** — only the file name. Paths are reconstructed on-demand via `FileTree::full_path()` by walking up through `parent` indices.
3. **Children are a singly-linked list** (`first_child` + `next_sibling`) — no `Vec<NodeIndex>` per node, saving allocations.
4. **Aggregation is a single reverse pass** over the flat arena — children are always after parents in insertion order.
5. **TreeView uses `egui::Painter` directly** rather than egui layout widgets — gives precise pixel control for virtualisation.
6. **Drive type constants** are hardcoded `u32` values (2/3/4/5) rather than `windows` crate enums, because `windows 0.58` doesn't expose `DRIVE_FIXED` etc. as importable items.
8. **Layout**: Tree view lives in the left sidebar (default 500px, below drive picker); treemap fills the central panel; details + file type chart in the right sidebar. App launches maximized.

## How to Build & Run

```powershell
cd C:\Users\Steve Watson\Projects\disksleuth

# Debug build + run
cargo run

# Release build (LTO, stripped, optimised)
cargo build --release
# Output: target\release\disksleuth.exe

# Run tests
cargo test --workspace
```

## How to Resume Work

Reference this file at the start of a new chat:
> "I'm working on DiskSleuth. Read `C:\Users\Steve Watson\Projects\disksleuth\PROGRESS.md` for project state."

Then specify what you want to work on next (e.g. "Implement Phase 2 duplicate detection" or "Add keyboard navigation to the TreeView").
