# DiskSleuth — Project Atlas

> **Canonical reference.** Every change affecting structure, APIs, architecture,
> build, or config MUST update this document in the same increment.
> Atlas/code/test contradictions are defects.

---

## 1. System Purpose

DiskSleuth is a production-grade, standalone Windows disk-space analyser.  It
scans a drive or directory using one of two strategies (NTFS MFT direct-read or
parallel directory walk), presents an interactive SpaceSniffer-style treemap and
virtualised tree view, and lets the user explore, navigate, and export findings.

**Target users:** Windows power users, developers, and sysadmins who need to
find where disk space is going — quickly and without installing runtime
dependencies.

**Out of scope (current version):**
- Cross-platform (Linux / macOS) support.
- Network share scanning beyond what Windows enumerates as a local drive letter.
- Hash-based duplicate detection (Phase 2 stub exists, not yet implemented).
- CLI / TUI frontends (architecture is ready; only the GUI frontend is shipped).

---

## 2. Domain Concepts

| Concept | Definition |
|---------|-----------|
| **FileTree** | Arena-allocated (`Vec<FileNode>`) graph of all scanned nodes. Parent-child relationships use `NodeIndex(u32)` handles, never pointers. |
| **NodeIndex** | Thin `u32` wrapper that indexes into `FileTree::nodes`. Safe by construction; always refers to a node that exists at the time of creation. |
| **FileNode** | A single file or directory. Stores: name (`CompactString`), logical size, allocated size, is_dir flag, parent/first_child/next_sibling links, descendant count, modified timestamp, percent_of_parent. |
| **LiveTree** | `Arc<RwLock<FileTree>>` shared between the scan thread and the UI. The scanner holds a write lock for brief batch inserts; the UI holds a read lock per frame. |
| **ScanHandle** | Returned by `start_scan()`. Carries the progress channel receiver, the LiveTree reference, and the cancellation flag. |
| **ScanProgress** | Enum of messages the scan thread sends to the UI: `ScanTier`, `Update`, `Error`, `Complete`, `Cancelled`. |
| **AppState** | All mutable GUI state. Owns the scan handle, live tree reference, final tree, visible-row list, treemap navigation stacks, monitor handle, UI flags, and cached elevation status (`is_elevated`). |
| **VisibleRow** | Flat entry in the virtualised tree-view: `NodeIndex + depth + is_expanded`. |
| **AppPhase** | `Idle | Scanning | Results` — the top-level state machine of the application. |
| **Treemap** | Squarified layout of `FileNode` rectangles. Painter-based (no retained geometry). Click navigates into a directory. |
| **Monitor** | Background `ReadDirectoryChangesW` watcher. Reports live write events as `WriteEvent` records with path, hit count, and last-seen timestamp. |

---

## 3. Architectural Boundaries

```
┌─────────────────────────────────────────────────────────────────┐
│  Binary entry point  (src/main.rs)                              │
│  • Init logging (DISKSLEUTH_LOG env var)                        │
│  • Build AppState before opening the window (Rule 16)           │
│  • eframe::run_native()                                         │
└───────────────────────┬─────────────────────────────────────────┘
                        │
          ┌─────────────▼──────────────────────┐
          │  disksleuth-gui                     │
          │  • eframe::App impl (app.rs)         │
          │  • AppState state machine (state.rs) │
          │  • Panels & widgets (egui)           │
          │  NO Win32 calls                     │
          └──────────────┬──────────────────────┘
                         │  depends on
          ┌──────────────▼──────────────────────┐
          │  disksleuth-core                    │
          │  • scanner/ (parallel + MFT)        │
          │  • model/ (FileTree arena)           │
          │  • analysis/ (top-files, age, etc.) │
          │  • platform/ (drives, elevation)    │
          │  • monitor/ (ReadDirectoryChangesW) │
          │  ZERO egui / eframe dependencies   │
          └─────────────────────────────────────┘
```

**Hard rules:**
1. `disksleuth-core` MUST NOT depend on `egui`, `eframe`, or any UI crate.
2. `disksleuth-gui` MUST NOT contain business logic. It owns only presentation.
3. All Win32 API calls live inside `disksleuth-core` (scanner, monitor, platform).
4. Cross-cutting concerns (logging, config) do not leak into model or analysis modules.

---

## 4. Repository Structure

```
DiskSleuth/
├── Cargo.toml                     Workspace manifest; shared dependency versions
├── Cargo.lock                     Locked dependency tree (committed)
├── build.rs                       Windows manifest + icon embedding
├── src/
│   └── main.rs                    Thin binary entry point
├── crates/
│   ├── disksleuth-core/
│   │   ├── Cargo.toml
│   │   ├── src/
│   │   │   ├── lib.rs             Crate root; re-exports modules
│   │   │   ├── scanner/
│   │   │   │   ├── mod.rs         start_scan(), ScanHandle, LiveTree type alias
│   │   │   │   ├── parallel.rs    Tier 2: jwalk/rayon parallel walker
│   │   │   │   ├── mft.rs         Tier 1: NTFS MFT direct reader (admin only)
│   │   │   │   └── progress.rs    ScanProgress / ScanCommand enums
│   │   │   ├── model/
│   │   │   │   ├── mod.rs         Re-exports FileTree, FileNode, NodeIndex
│   │   │   │   ├── file_node.rs   NodeIndex(u32) + FileNode struct
│   │   │   │   ├── file_tree.rs   Arena, aggregation, path reconstruction
│   │   │   │   └── size.rs        format_size(), format_count()
│   │   │   ├── analysis/
│   │   │   │   ├── mod.rs         Re-exports public analysis functions
│   │   │   │   ├── top_files.rs   Top-N largest files
│   │   │   │   ├── file_types.rs  Extension categorisation + CategoryStats
│   │   │   │   ├── age.rs         Stale-file finder
│   │   │   │   └── duplicates.rs  Phase 2 stub (size+hash duplicate detection)
│   │   │   ├── platform/
│   │   │   │   ├── mod.rs         Re-exports enumerate_drives, is_elevated
│   │   │   │   ├── drives.rs      GetLogicalDriveStringsW + DriveInfo
│   │   │   │   └── permissions.rs GetTokenInformation elevation check
│   │   │   └── monitor/
│   │   │       └── mod.rs         ReadDirectoryChangesW overlapped monitor
│   │   └── tests/
│   │       └── e2e_scanner.rs     E2E integration tests (real tempdir scans)
│   └── disksleuth-gui/
│       ├── Cargo.toml
│       ├── src/
│       │   ├── lib.rs             Re-exports DiskSleuthApp, DiskSleuthState
│       │   ├── app.rs             eframe::App + DiskSleuthState::build()
│       │   ├── state.rs           AppState, AppPhase, VisibleRow
│       │   ├── icon.rs            Application icon generation
│       │   ├── panels/
│       │   │   ├── mod.rs
│       │   │   ├── scan_panel.rs  Left sidebar: drives, scan controls
│       │   │   ├── tree_panel.rs  Centre: virtualised tree view
│       │   │   ├── details_panel.rs  Right sidebar: selected item info
│       │   │   ├── chart_panel.rs File type breakdown bars
│       │   │   └── monitor_panel.rs  Live write-event table
│       │   └── widgets/
│       │       ├── mod.rs
│       │       ├── tree_view.rs   Painter-based virtualised TreeView
│       │       ├── treemap.rs     Squarified treemap widget
│       │       ├── drive_picker.rs  Drive selection with usage bars
│       │       ├── size_bar.rs    Proportional size-bar widget
│       │       ├── toolbar.rs     Top action bar
│       │       └── status_bar.rs  Bottom progress/stats bar
│       └── tests/
│           └── e2e_state.rs       E2E tests for AppState (no window needed)
├── assets/                        Application icons and supplementary assets
├── .github/
│   └── workflows/
│       ├── ci.yml                 PR/push quality gates (fmt + clippy + test)
│       └── release.yml            Tag-triggered release build + publish
├── update-application.ps1         Automated release lifecycle script
├── ATLAS.md                       This document (Project Atlas)
├── README.md                      User-facing documentation
└── PROGRESS.md                    Development progress tracker
```

---

## 5. Entry Points, APIs, and Extension Points

### Binary entry point
- `src/main.rs` — resolves `DISKSLEUTH_LOG` env var, builds `DiskSleuthState`,
  then calls `eframe::run_native`.

### Public API surface of `disksleuth-core`

| Module | Key exports |
|--------|------------|
| `scanner` | `start_scan(PathBuf) -> ScanHandle`, `ScanHandle`, `LiveTree`, `PROGRESS_CHANNEL_CAPACITY` |
| `scanner::mft` | `is_mft_available(&Path) -> bool`, `scan_mft(...)` |
| `scanner::parallel` | `scan_parallel(...)` |
| `scanner::progress` | `ScanProgress`, `ScanCommand` |
| `model` | `FileTree`, `FileNode`, `NodeIndex` |
| `model::size` | `format_size(u64) -> String`, `format_count(u64) -> String` |
| `analysis` | `top_files`, `analyse_file_types`, `find_stale_files`, `find_duplicates` |
| `platform` | `enumerate_drives() -> Vec<DriveInfo>`, `is_elevated() -> bool`, `DriveInfo`, `DriveType` |
| `monitor` | `start_monitor(PathBuf) -> MonitorHandle`, `MonitorHandle`, `WriteEvent`, `MonitorMessage`, `MAX_MONITOR_ENTRIES` |

### Extension points
- **New scanner tier:** Add a new module under `scanner/`, implement the same
  `(PathBuf, Sender<ScanProgress>, Arc<AtomicBool>, LiveTree)` signature, and
  select it in `scanner/mod.rs::start_scan`.
- **New analysis:** Add a module under `analysis/`, export from `analysis/mod.rs`.
- **New UI panel:** Add a file under `panels/`, register in `app.rs::update`.
- **New widget:** Add a file under `widgets/`, call from the appropriate panel.

---

## 6. Build, Test, CI, and Release

### Build

```powershell
# Debug build
cargo build --workspace

# Release build (LTO, stripped, optimised)
cargo build --release

# Run debug build
cargo run

# Single portable exe at:
target\release\disksleuth.exe
```

### Testing

```powershell
# All tests (unit + integration)
cargo test --workspace

# Specific integration test file
cargo test --test e2e_scanner
cargo test --test e2e_state
```

### Linting & Formatting

```powershell
cargo fmt --all             # format all crates
cargo fmt --all -- --check  # check without writing (used in CI)
cargo clippy --workspace --all-targets -- -D warnings
```

### CI/CD

| Workflow | Trigger | Jobs |
|----------|---------|------|
| `.github/workflows/ci.yml` | Push/PR to `main` | Format check → Clippy → Tests → Debug build |
| `.github/workflows/release.yml` | Push of `v*` tag | Format check → Clippy → Tests → Release build → Publish |

### Release script

```powershell
# Interactive release (prompts for version and notes)
.\update-application.ps1

# Non-interactive
.\update-application.ps1 -Version 1.2.0 -Notes "What changed"

# Dry run (no file changes)
.\update-application.ps1 -Version 1.2.0 -Notes "Preview" -DryRun
```

The script performs: version bump → lockfile refresh → confirmation → release
build → fmt/clippy/test gates → commit → tag → push → prune old tags.

---

## 7. Configuration

DiskSleuth deliberately has no persistent configuration file for simplicity.
All user preferences (theme, monitor panel visibility) are in-memory only and
reset to defaults on each launch.

### Runtime configuration

| Variable | Type | Default | Purpose |
|----------|------|---------|---------|
| `DISKSLEUTH_LOG` | env var | `info` | Log level: `error`, `warn`, `info`, `debug`, `trace`. Activates runtime debug output without recompilation. |
| `SystemDrive` | env var (Windows) | `C:` | Drive auto-scanned on startup. |
| `SystemRoot` | env var (Windows) | `C:\Windows` | Search path for Segoe UI font. |

### Workspace version

Authoritative location: `[workspace.package] version` in `Cargo.toml`.
All crate versions inherit from this single source via `version.workspace = true`.
Never edit individual crate Cargo.toml version fields.

---

## 8. Critical Invariants

These invariants MUST hold at all times. Violations are bugs.

1. **Arena immutability after completion:** Once `ScanProgress::Complete` is
   sent, the `FileTree` inside `LiveTree` is never written again. Readers may
   hold read locks indefinitely without risking stale data.

2. **NodeIndex validity:** A `NodeIndex` is only ever created by `FileTree::add_node`
   or `FileTree::add_root`. Passing an index from one tree to another tree is
   undefined behaviour and MUST NOT occur.

3. **Aggregation idempotence:** `aggregate_sizes()` resets all dir-node accumulated
   fields to zero before each pass, so calling it multiple times on the same tree
   produces the same result. It is safe to call during scanning (live) and again
   after completion (final).

4. **Parent-child ordering:** Nodes are inserted into the arena parent-first.
   The reverse-iteration aggregation pass (`i in (0..n).rev()`) depends on this
   invariant. Inserting a child before its parent will produce wrong aggregated
   sizes.

5. **Bounded collections:** The following constants cap all growing collections.
   Raising them increases memory usage; do not delete them without a replacement:
   - `MAX_VISIBLE_ROWS = 500_000` (tree-view rows)
   - `MAX_SCAN_ERRORS = 1_000` (per-scan error list in GUI state)
   - `MAX_MONITOR_ENTRIES = 500` (live-monitor event table)
   - `MAX_MESSAGES_PER_FRAME = 300` (scan progress messages drained per frame)
   - `MAX_MONITOR_MESSAGES_PER_FRAME = 200` (monitor messages drained per frame)
   - `MAX_NAV_HISTORY = 50` (treemap nav back/forward stacks)
   - `MAX_MFT_RECORDS = 15_000_000` (MFT record cap per scan)
   - `PROGRESS_CHANNEL_CAPACITY = 4_096` (scan progress channel depth)

6. **No UI work before window creation:** All expensive startup I/O (drive
   enumeration, initial scan kick-off) happens inside `DiskSleuthState::build()`
   before `eframe::run_native()` is called. The `run_native` creator closure
   performs only font setup and state struct construction.

7. **Cancellation is cooperative:** The scanner checks the cancel flag at most
   every 1 000 entries. The monitor checks it at most every 200 ms. Maximum
   latency to actual stop is bounded by these intervals.

11. **`start_scan` cancels any in-progress scan:** `AppState::start_scan`
    always calls `cancel_scan()` before starting a new scan, ensuring the old
    scan thread sets its stop flag and the orphaned-thread scenario is
    impossible even if future code paths call `start_scan` while scanning.

8. **Log level default is INFO:** `DISKSLEUTH_LOG` absent = INFO. Debug/trace
   level must never be active in release builds by default. Secrets and PII
   must never be logged at any level.

9. **Single source of version truth:** The workspace version in `Cargo.toml` is
   the canonical version. The release script (`update-application.ps1`) is the
   only sanctioned way to change it. There are no separate per-crate version
   declarations.

10. **Quality gates block release:** All three gates — `cargo fmt -- --check`,
    `cargo clippy -- -D warnings`, `cargo test --workspace` — must pass before
    a release tag is created, both in the CI workflow and in the release script.

---

## 9. Required Runtime Dependencies

| Dependency | Minimum version | Why |
|------------|-----------------|-----|
| Windows 10 (x86_64) | Build 19041 | Win32 `ReadDirectoryChangesW` overlapped I/O, `FSCTL_ENUM_USN_DATA` |
| Rust toolchain (build only) | 1.75 (2021 edition) | `is_multiple_of` on integers, workspace deps |

No runtime-installable dependencies (no .NET, no VC++ redistributables). The
release binary is a fully standalone `.exe`.

---

## 10. Known Technical Debt

| Area | Issue | Tracking |
|------|-------|---------|
| Duplicate detection | `find_duplicates` is a Phase 2 stub — returns empty vec | Phase 2 |
| Error persistence | Scan errors are in-memory only; not written to disk | Phase 3 |
| Export | No CSV/JSON export from the UI (core has `csv` dep) | Phase 3 |
| Theme persistence | Dark/light preference resets on restart | Phase 3 |

---

## 11. Performance Architecture

The following design decisions and invariants govern performance-critical paths.
Violating them will regress measurable scan speed or render-thread responsiveness.

### 11.1 Scanner lock contention (parallel.rs)

The scan loop accumulates nodes in a local `Vec<PendingEntry>` and flushes them
to the shared `LiveTree` under a **single write lock per batch** (`BATCH_SIZE =
2_000`).  Every `PendingEntry` carries a pre-computed `NodeIndex` equal to
`arena_base + position_in_vec`.  Directory entries are registered in `dir_map`
immediately with this pre-computed index so children within the same batch can
resolve their parent without an extra lock or flush.

**Invariant:** `arena_base` must be incremented by exactly the number of nodes
inserted into `FileTree::nodes` between flushes (including nodes inserted by
`ensure_ancestors` or error-handling paths that bypass the batch).  A mismatch
causes `debug_assert_eq!` failures in debug builds.

**Impact:** ~2 000× fewer write-lock acquisitions on a 2M-node drive (≈1 000 vs
≈2 000 000).

### 11.2 MFT Phase C — parallel metadata reads (mft.rs)

USN records do not carry file sizes.  After building the tree from MFT records,
`build_tree_from_mft` stats all file nodes with `fs::metadata`.  This is done in
parallel with `rayon::par_iter` (read-only `full_path` traversal) followed by a
sequential write-back pass.  `FileTree` is `Sync`, so the shared reference is
safe across rayon threads.

**Impact:** Phase C wall-clock time scales with `1 / CPU_count` on SSDs/NVMe
instead of serial execution.

### 11.3 Top-N file sort — partial select (file_tree.rs)

`compute_largest_files(n)` uses `select_nth_unstable_by` (O(n) average) to
bring the top-n indices to the front, then `sort_unstable_by` on only those n
elements (O(k log k)).  For n = 100 on a 2M-file tree this is O(2M) instead of
O(2M log 2M) — roughly a 20× reduction in comparisons.

### 11.4 Extension categorisation — zero-heap allocation (file_types.rs)

`categorise_extension` lowercases the input into a fixed `[u8; 16]` stack
buffer.  Extensions longer than 16 bytes are short-circuited to `Other`.
This eliminates one `String` heap allocation per file node during
`analyse_file_types`, which processes every non-directory node in the tree.

### 11.5 File-type stats caching (state.rs / chart_panel.rs)

`analyse_file_types` iterates all nodes (O(n)).  It is called **once** after
scan completion and the result is stored in `AppState::file_type_stats`.  The
chart panel reads this cache rather than recomputing on every render frame.
During an active scan the panel shows a placeholder message.

### 11.6 Treemap navigation — no FileTree clone (state.rs / app.rs)

`reveal_node_in_tree` and `treemap_go_up` previously cloned the entire
`FileTree` to satisfy the borrow checker.  Both now use disjoint field borrows
or internal state lookups, eliminating multi-million-node heap allocations on
every navigation event.

### 11.7 Navigation history — O(1) eviction (state.rs)

`treemap_back` and `treemap_forward` are `VecDeque<NodeIndex>`.  Evicting the
oldest entry when at capacity uses `pop_front()` (O(1)) instead of
`Vec::remove(0)` (O(n)).

### 11.8 Theme application — change-gated (app.rs)

`ctx.set_visuals()` is called only when `AppState::dark_mode` changes (tracked
via `DiskSleuthApp::last_dark_mode`), not on every frame.  The initial call at
startup still ensures correct visual state.

### 11.9 Elevation check — cached in AppState (state.rs / toolbar.rs)

`is_elevated()` (`OpenProcessToken` + `GetTokenInformation` + `CloseHandle`)
is called **once** inside `AppState::new()` and stored in `AppState::is_elevated`.
The toolbar reads this cached field rather than issuing three Win32 syscalls on
every render frame.  Elevation status is immutable for the lifetime of a process,
so caching is always correct.

### 11.10 MFT enumeration — no per-record heap allocation (mft.rs)

The hot `FSCTL_ENUM_USN_DATA` parsing loop previously allocated two heap objects
per USN record: a `Vec<u16>` for the UTF-16 name bytes and a `String` for the
decoded text.  For a 2M-file volume this was 4M allocations in a tight loop.

Both are eliminated: `char::decode_utf16()` iterates over the raw byte slice
directly and `collect::<CompactString>()` decodes in a single pass.  For the
vast majority of filenames (≤ 15 UTF-8 bytes) `CompactString` stores the result
inline without any heap allocation at all.  Longer names allocate a single
object instead of two.

**Impact:** eliminates 4M heap allocs per scan; reduces peak resident set size
on MFT-path scans.

### 11.11 MFT IOCTL buffer — 64 KB → 256 KB (mft.rs)

The `DeviceIoControl` output buffer for `FSCTL_ENUM_USN_DATA` was 64 KB.
Increasing it to 256 KB delivers four times as many MFT records per syscall,
reducing the number of kernel transitions by up to 75% on high-inode volumes.

**Impact:** measurable reduction in kernel-mode time for NTFS volumes with
millions of small files.

### 11.12 `FileTree::file_count` — O(1) render-thread stat (file_tree.rs / status_bar.rs)

The status bar previously computed file count via
`tree.nodes.iter().filter(|n| !n.is_dir).count()` on every egui frame (60 fps).
On a 2M-node tree this is 120M node reads per second on the render thread.

A `pub file_count: u64` field is now maintained inside `aggregate_sizes_inner`,
which already iterates all nodes to reset directory fields.  The counter is set
during that O(n) pass at no additional asymptotic cost.  Render frames read the
cached scalar.

**Invariant:** `file_count` is only authoritative after `aggregate_sizes()` or
`aggregate_sizes_live()` has been called.  It is initialised to zero and
represents all non-directory nodes (including error file nodes) as per the
pre-existing status bar semantics.

**Impact:** O(n) per frame → O(1) per frame; eliminates ~120M node loads/sec
in the Results phase.

### 11.13 `truncate_name` — zero-copy for non-truncated names (treemap.rs)

The treemap header and file label renderer calls `truncate_name` on every
visible rect per frame (up to 75 000 rects).  The function previously returned
`String::to_owned()` even when no truncation was needed — allocating for every
name that already fit.

Return type changed to `Cow<'_, str>`: the `Borrowed` variant returns a slice
into the existing `CompactString` with no allocation; the `Owned` variant
allocates only when the name is actually truncated.  The vast majority of
visible names fit without truncation so almost all calls now take the zero-copy
path.

### 11.14 `TreemapRect::name` — `CompactString` instead of `String` (treemap.rs)

Each `TreemapRect` previously held `name: String` set via `.to_string()` on the
node's `CompactString` name.  Changed to `name: CompactString` and `clone()`.
For the common short-name case the clone is an inline copy with no heap
allocation.

### 11.15 `squarify_nested` scratch buffers — hoisted out of inner loop (treemap.rs)

Two `Vec` allocations occurred inside the greedy-row-building inner loop:
`row: Vec<usize>` and `trial: Vec<f32>`.  They were re-created on every row
iteration via `vec![idx]` and `.collect()` respectively.

Both are now declared once before the outer `while idx < items.len()` loop and
reused with `.clear()` + `.push()` / `.extend()`.  The allocator sees at most
two allocations per `squarify_nested` call; capacity is preserved across
iterations.  Because `squarify_nested` is called recursively (one call per
visible directory, with `capacity(32)` warmup), this eliminates O(rects) heap
allocs per frame entirely from the greedy step.

### 11.16 `rebuild_live_visible_rows` — pre-sized `HashSet` (state.rs)

The `HashSet<NodeIndex>` used to track expanded nodes was constructed from a
filtered iterator whose lower-bound size hint is 0, causing `HashSet::default()`
to start with a small backing table and rehash up to O(log n) times.

It is now created with `HashSet::with_capacity(approx_expanded + roots + 8)`
where `approx_expanded` is the count of currently expanded rows.  This
eliminates all rehashes for typical trees and reduces allocator pressure during
live-scan updates (which fire every few seconds on an active scan).

---

*Last updated: 2026-03-01 — performance review: 7 measurable improvements across MFT
scanner, FileTree aggregation, treemap rendering, and live-scan state
management (see §11.10 – §11.16).*

