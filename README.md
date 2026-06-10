# DiskSleuth

A fast, visual disk space analyser for Windows — built with Rust and [egui](https://github.com/emilk/egui).

DiskSleuth scans your drives in parallel, displays results in an interactive tree view and a SpaceSniffer-style treemap, and helps you find where your disk space is going.

## Features

- **Parallel scanning** — uses [jwalk](https://crates.io/crates/jwalk) + rayon to walk the filesystem across all available cores
- **NTFS MFT fast-scan** — optional direct MFT reader (`FSCTL_ENUM_USN_DATA`) for near-instant enumeration on NTFS volumes (requires admin; falls back to the parallel walker automatically)
- **SpaceSniffer-style treemap** — nested squarified layout with directory headers, click-to-navigate, back/forward/up, and breadcrumb trail
- **Virtualised tree view** — renders only visible rows for smooth scrolling with millions of files; proper font-metric text clipping with ellipsis
- **Real-time progress** — tree view and treemap update live as the scan progresses via `Arc<RwLock<FileTree>>`
- **Selection sync** — clicking an item in the tree highlights it in the treemap and vice versa
- **Live write monitor** — optional bottom panel that watches the selected drive with `ReadDirectoryChangesW` and shows which files are being written to right now, with per-file hit counts
- **CSV / JSON export** — one click writes the full scan (paths, sizes, types, timestamps) to a timestamped CSV or JSON file in your Documents folder
- **Auto-scan on startup** — begins scanning the OS drive (`%SystemDrive%`) immediately on launch
- **Arena-allocated file tree** — `Vec<FileNode>` + `NodeIndex(u32)` for cache-friendly traversal and O(n) bottom-up aggregation
- **Drive picker** — lists local drives (fixed, removable, optical) with usage bars, filesystem type, and capacity
- **File type breakdown** — interactive donut chart plus proportional bars, by extension category
- **Largest files window** — the top 100 biggest files, one click to locate each in the tree and treemap
- **Old files window** — files untouched for 1 month – 2 years, sorted by reclaimable size
- **Duplicate file detection** — size grouping → 4 KB prefix hash → full content hash, parallel and cancellable, grouped by wasted bytes
- **Scan history & comparison** — every completed scan records a snapshot; diff two scans of the same path to see which directories grew or shrank
- **Custom folder scan** — scan any folder via the native folder picker, not just whole drives
- **Sortable columns** — click Name / Size / Files headers to re-order the tree (direction toggles)
- **Keyboard navigation** — arrow keys or vim-style h/j/k/l, PgUp/PgDn, Home/End, Enter to drill in / open, Del to delete
- **Recycle Bin deletion** — delete files or folders straight from the results (with confirmation); totals, charts, and treemap update in place
- **Right-click context menu** — Open in Explorer, Copy Path, Delete (Recycle Bin)
- **Dark / Light theme** toggle
- **Cancellation** — stop a scan at any time; partial results stay visible
- **Single portable executable** — no installer, no runtime dependencies

## Screenshot

<!-- TODO: Add screenshot -->

## Getting Started

### Download

Grab the latest `DiskSleuth.exe` from the
[Releases page](https://github.com/Swatto86/DiskSleuth/releases/latest) — a
single portable executable, no installer needed. Run it as administrator to
enable the MFT fast-scan tier.

### Requirements

- **Windows 10+** (x86_64)
- **Rust 1.87+** (2021 edition) — for building from source

### Build & Run

```powershell
git clone https://github.com/Swatto86/DiskSleuth.git
cd DiskSleuth

# Release build (recommended — LTO, stripped, optimised)
cargo build --release

# Run
.\target\release\DiskSleuth.exe

# Or build + run in one step
cargo run --release
```

The release binary is at `target\release\DiskSleuth.exe`.

### Run Tests

```powershell
cargo test --workspace
```

### CI & Releases

Every push and pull request runs the quality gates on Windows
(`cargo fmt -- --check`, `cargo clippy -- -D warnings`,
`cargo test --workspace`). Releases are built and published by GitHub Actions
— either by pushing a `vX.Y.Z` tag (see `update-application.ps1`, which bumps
the version, tags, and pushes in one step) or by manually dispatching the
**Release** workflow with a version and release notes.

### Debug / Verbose Logging

DiskSleuth logs to **stderr** using
[tracing](https://docs.rs/tracing/latest/tracing/).  The default log level is
`info`.  To enable verbose diagnostic output without recompiling, set the
`DISKSLEUTH_LOG` environment variable before running the executable:

```powershell
# Verbose debug output (function flow, state transitions, OS interactions)
$env:DISKSLEUTH_LOG = "debug"
.\target\release\DiskSleuth.exe

# Full trace output (very noisy — includes every event)
$env:DISKSLEUTH_LOG = "trace"
.\target\release\DiskSleuth.exe

# Reset to default (info) for the current session
Remove-Item Env:\DISKSLEUTH_LOG
```

**Valid values** (least → most verbose): `error`, `warn`, `info` *(default)*,
`debug`, `trace`.

All output goes to **stderr** so it does not interfere with stdout.  Secrets,
tokens, and PII are never logged at any level.

## Architecture

```
DiskSleuth/
├── src/main.rs                     # Thin binary entry point
├── crates/
│   ├── disksleuth-core/            # Pure logic — scanning, model, analysis (zero UI deps)
│   │   ├── src/
│   │   │   ├── scanner/            # Parallel walker, MFT reader, progress channel
│   │   │   ├── model/              # Arena file tree, node types, size formatting
│   │   │   ├── analysis/           # Top files, file types, age, duplicates, history, CSV/JSON export
│   │   │   ├── platform/           # Windows drives, admin detection, Recycle Bin ops
│   │   │   └── monitor/            # ReadDirectoryChangesW live write-event watcher
│   │   └── tests/                  # End-to-end scanner tests (real tempdir scans)
│   └── disksleuth-gui/             # egui desktop frontend
│       ├── src/
│       │   ├── app.rs              # eframe::App + font setup (Segoe UI + Segoe UI Emoji)
│       │   ├── state.rs            # UI state, navigation history, tree expansion, export
│       │   ├── icon.rs             # Application icon generation
│       │   ├── widgets/            # TreeView, Treemap, DrivePicker, Toolbar, StatusBar
│       │   └── panels/             # Scan, Tree, Details, Chart, Monitor panels
│       └── tests/                  # End-to-end AppState tests
├── .github/workflows/              # CI quality gates + tag-triggered release build
├── update-application.ps1          # One-step release script (bump, tag, push)
├── build.rs                        # Windows manifest + icon embedding
└── assets/                         # icon.ico (generated by build.rs)
```

### Key Design Decisions

| Decision | Rationale |
|----------|-----------|
| Arena tree (`Vec<FileNode>`) | Cache-friendly, zero-allocation traversal, O(n) aggregation |
| Singly-linked children | `first_child` + `next_sibling` — no `Vec` per node |
| Names only, no full paths | Paths reconstructed on-demand via parent chain |
| Painter-based tree view | Pixel-precise virtualised rendering, O(1) per frame |
| `parking_lot::RwLock` for live tree | Lock-free reads during rendering, writer only from scanner thread |
| Crossbeam channels for progress | Decouples scanner from UI — UI never blocks |

## Dependencies

| Crate | Purpose |
|-------|---------|
| `eframe` / `egui` 0.31 | Immediate-mode GUI framework |
| `jwalk` 0.8 | Parallel directory walking |
| `rayon` 1.10 | Work-stealing thread pool |
| `crossbeam-channel` 0.5 | Scan → UI progress messaging |
| `compact_str` 0.8 | Small-string optimisation for file names |
| `windows` 0.58 | Win32 API (drives, filesystem, MFT) |
| `parking_lot` 0.12 | Fast reader-writer locks |
| `chrono` 0.4 | Date/time for file age analysis, export timestamps |
| `csv` 1.3 | CSV export writer |
| `serde` / `serde_json` 1 | JSON export serialisation |
| `rfd` 0.15 | Native folder picker for custom folder scans |
| `tracing` 0.1 | Structured stderr logging (`DISKSLEUTH_LOG`) |

## Roadmap

All planned features have shipped:

- [x] Export scan results to CSV (toolbar → Documents folder)
- [x] Live file-write monitor
- [x] Export scan results to JSON
- [x] Duplicate file detection (size → prefix hash → full hash)
- [x] Stale / old file panel
- [x] File type pie / donut chart
- [x] Keyboard navigation (arrow keys, vim-style)
- [x] Sort by column header click
- [x] Custom folder scan (not just whole drives)
- [x] Scan history & comparison
- [x] File deletion with recycle bin support

Ideas for the future are tracked in [issues](https://github.com/Swatto86/DiskSleuth/issues).

## License

[MIT](LICENSE)
