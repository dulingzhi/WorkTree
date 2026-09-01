# P1: Dissolve view/mod_helpers.rs Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Dissolve `crates/worktree-ui-gpui/src/view/mod_helpers.rs` (5,877 lines, 9 unrelated domains) into domain modules, ending with the file deleted and the `use mod_helpers::*` glob injection root gone.

**Architecture:** Twelve pure-move tasks, leaf domains first (toast, branch-selection, resize, preview-kind, hit geometry, status), then vocabulary re-bucketing (caches/three-way, popover vocab, view-mode/diff-prefs, terminal types), then the two big relocations (ConflictResolverUiState ~3,000 lines, WorkTreeView struct). Each task moves items to their destination module, replaces glob delivery with named `use` lines in `view/mod.rs` (or explicit imports at few consumers), and verifies the full CI matrix. The final task deletes `mod_helpers.rs` itself.

**Tech Stack:** Rust 2024 edition, cargo workspace, GPUI. No new dependencies.

**Spec:** `docs/superpowers/specs/2026-08-31-rust-codebase-refactor-design.md` — P1 section (lines 81-98). The spec's phase goal, 验收度量 row "glob 注入根 2 处 → 0"， and Global Constraints are binding; the per-domain destination table is advisory where consumer evidence (recon 2026-09-01, line numbers verified against current file — unchanged since spec snapshot) contradicts it (see Plan-Level Rulings).

## Global Constraints

- **验收四件套 + benchmarks 腿，每个任务全跑**（feature-gate 教训来自 P0 Task 9；`rows/benchmarks.rs` 本身显式导入 `crate::view::mod_helpers::{PaneResizeHandle, PaneResizeState, StatusMultiSelection, StatusSection}`，任务 3/6/8 移动这些符号时该 use 必须改指）：
  1. `cargo clippy --workspace --no-default-features --features gix -- -D warnings`
  2. `cargo test --workspace --no-default-features --features gix`
  3. `cargo build -p worktree --features ui-gpui,gix`
  4. `cargo test --workspace --features benchmarks --no-run`
  涉及测试搬迁的任务（1、3、7、11）另跑 `cargo test --workspace`（default features）。
  ⚠️ rtk 代理会过滤 cargo 输出：结论必须以退出码 + 完整日志复核（`rtk proxy <cmd>` 或重定向到文件后读日志），不得只看过滤后的摘要。
- **纯搬移优先**：不改函数体、不改项语义、不重命名。可见性换算仅允许等受众换算：`pub(super)`（在 view 直子模块中 = view 子树可见）搬入新位置后取等价值（新模块同为 view 直子模块时保持 `pub(super)`；`pub` 保持 `pub`）。禁止新增 `pub(in super::super::…)`。
- **兼容壳规则**：搬移符号的 glob 投递由 `view/mod.rs` 中的**具名 use** 接替（glob 注入根消灭，消费方文件零改动）。例外：fan-out ≤ 5 且非 pub API 的符号，改为在其少量消费方加显式 import（任务内指明）。具名 use 写在 `view/mod.rs` 现有 `use mod_helpers::*;`（251 行）下方，按任务分组；最终任务删除 glob 行后具名清单即新的 view 命名面。
- **新模块不新增 glob 放大器**：新模块文件首行的 `use super::*;` 仅允许随搬移体原样保留（mod_helpers.rs:1 本就是 `use super::*;`，保留它维持纯搬移）；不得新增其他 glob use。
- **rustfmt 纪律**（P0 教训）：只对触及文件跑 `rustfmt --edition 2024 --config skip_children=true <file>`；禁止 crate 级 `cargo fmt`。
- **测试安置**：`conflict_resolver_ui_state_tests`（mod_helpers.rs:3072-4347，约 1,275 行）随符号迁入既有 `view/conflict_resolver/tests/` 目录（新文件 `ui_state.rs`，由 tests/mod.rs 挂载）；`resize_drag_ghost_tests`（544-566）、`markdown_preview_wrap_cache_tests`（1348-1385）随符号同迁。
- **测试同一性**：搬迁前后 `cargo test -p worktree-ui-gpui <filter> -- --list` 的测试名集合必须逐名一致（剥除模块路径前缀后比对）；总数不得增减。
- **提交纪律**：每任务一提交，祈使句标题，单一目的（P0 风格：`Move the sidebar details row rendering into its own module`）。

## Plan-Level Rulings（spec P1 表的偏差，均有消费方证据）

| # | 偏差 | 证据 | 裁决 |
|---|---|---|---|
| R1 | spec 表未覆盖 mod_helpers.rs:22-67（SelectedBranch 族） | 消费方 panes/history、panes/sidebar、rows/history、rows/sidebar | 新增任务 2：`view/branch_selection.rs`（BranchSection 本体在 view/branch_sidebar.rs:30，不动） |
| R2 | spec 把 4381-4438 归"终端类型族"，实为 1 终端 + 4 popover 词汇 | TerminalMenuContext 仅 terminal_panel.rs:2963 用；BranchPickerPurpose 11 文件（全 popover 系）、Remote/StashPickerPurpose、AutosquashMode 消费方为 popover/context_menu/interactive_rebase | TerminalMenuContext → 任务 10 terminal_types.rs；其余随任务 8 popover 词汇表 |
| R3 | RemoteRow/DiffClickKind/PatchSplitRow 位于 PopoverKind 行段内但非 popover 语义 | DiffClickKind 12 文件（panes/main 系）、PatchSplitRow 10 文件（panes/main + patch_split.rs）、RemoteRow 2 文件 | 任务 8 内分拣：RemoteRow 随 popover；DiffClickKind → `view/diff_utils.rs`；PatchSplitRow → `view/patch_split.rs`（均留 view 根具名壳） |
| R4 | spec 把 1076-1385 整段归 panes/main；实际是三个不相干簇 | ThreeWayColumn 15 文件（冲突渲染系）、ThreeWaySides 5 文件；MarkdownPreviewWrapCache/DeferredLineStarts 消费方 panes/main 仅 2-3 文件 | 任务 7 分三桶：ThreeWay 簇 → `view/conflict_resolver/three_way.rs`；缓存簇 → 既有 `view/caches.rs`；DiffTextLayoutCacheEntry（5874-5877，spec 未列）→ `view/caches.rs` |
| R5 | spec "滚动/命中几何 → rows 层"对 103-124 不成立 | absolute_scroll_y 仅 settings_window.rs 用；scroll_is_near_bottom 仅 panes/history/history_panel.rs 用；should_hide_unified_diff_header_line 3 文件（diff_cache×2 + rows/diff） | 任务 1：前两者 → 新 `view/scroll_geometry.rs`（相互调用须同处）；后者 → `rows/diff.rs` |
| R6 | 567-685 命中几何消费方在 view/panels 而非 rows | DiffTextHitbox 5 文件、ConflictTextHitbox 3 文件、DiffTextOffsetMap 仅 rows/diff_canvas.rs | 任务 5：新 `rows/text_geometry.rs`（渲染层词汇，pub(in crate::view) 受众不变） |
| R7 | spec 收尾"编译器逐文件暴露真实依赖，显式补 import（一次性付清）"与 spec 自身"兼容壳策略/保留存量 use super::* 文化"张力 | 验收度量的可测项是"glob 注入根 2→0" | 采用根级具名 use 方案：消灭注入根、view 命名面显式化、消费方零改动；全量 per-file 显式化不在 P1（留待自然更新），与 spec 兼容壳策略一致 |

## File Map（全部去向）

| 新文件 | 内容（来源行段） | 任务 |
|---|---|---|
| `view/scroll_geometry.rs` | absolute_scroll_y、scroll_is_near_bottom（110-124） | 1 |
| `view/branch_selection.rs` | SelectedBranch 族（22-67） | 2 |
| `view/resize_state.rs` | History 列宽 + 6 组分隔条状态机 + ResizeDragGhost + 其 tests（70-102、360-566） | 3 |
| `view/preview_kind.rs` | 预览类型判定族（125-358） | 4 |
| `rows/text_geometry.rs` | DiffTextRegion/Pos/Hitbox、ConflictTextHitbox、DiffTextWrappedHit、DiffTextOffsetMap（567-685） | 5 |
| `view/status_section.rs` | StatusSection 多选状态机（735-1075） | 6 |
| `view/conflict_resolver/three_way.rs` | ThreeWayColumn、ThreeWaySides（1076-1126） | 7 |
| `view/caches.rs`（扩） | DeferredLineStarts、Loadable 三别名、MarkdownPreviewWrapSlot/Cache + tests（1127-1385）；DiffTextLayoutCacheEntry（5874-5877） | 7 |
| `panels/popover.rs`（扩） | PopoverKind 族 + picker purposes + AutosquashMode + RemoteRow（4388-4955 内分拣） | 8 |
| `view/diff_utils.rs`（扩） | DiffClickKind | 8 |
| `view/patch_split.rs`（扩） | PatchSplitRow | 8 |
| `view/view_mode.rs` | 视图模式/启动/bootstrap 族 + 谓词 + ThemeMode（4956-5165、5373-5562） | 9 |
| `view/diff_prefs.rs` | ChangeTrackingView、DiffScrollSync、DiffContentMode、DiffWhitespaceMode（5563-5727） | 9 |
| `view/terminal_types.rs` | 终端 14 类型 + AlacrittyTermLock 别名 + TerminalMenuContext（7、4381-4387、5166-5372） | 10 |
| `view/conflict_resolver/ui_state.rs` | ConflictResolver 全状态族（1386-4380）；tests → `conflict_resolver/tests/ui_state.rs` | 11 |
| `view/worktree_view.rs` | `struct WorkTreeView`（5728-5872） | 12 |
| 折叠进既有文件 | Toast 族 → `view/toast_host.rs`（9-21、686-727）；CommitDetailsDelayState → `panes/details.rs`（728-734）；should_hide_unified_diff_header_line → `rows/diff.rs`（103-108） | 1 |
| 删除 | `view/mod_helpers.rs` + `view/mod.rs:174` mod 声明 + `:251` glob 行 | 12 |

---

### Task 1: Micro-moves — toast, commit-details delay, diff header predicate, scroll helpers

**Files:**
- Modify: `crates/worktree-ui-gpui/src/view/mod_helpers.rs`（删除 9-21、103-124、686-727、728-734）
- Modify: `crates/worktree-ui-gpui/src/view/toast_host.rs`（并入 toast_fade_in_duration、toast_fade_out_duration、toast_total_lifetime、ToastState、ToastAction、ToastDismissBehavior）
- Create: `crates/worktree-ui-gpui/src/view/scroll_geometry.rs`（absolute_scroll_y、scroll_is_near_bottom）
- Modify: `crates/worktree-ui-gpui/src/view/mod.rs`（挂 `mod scroll_geometry;` + 具名 use）
- Modify: `crates/worktree-ui-gpui/src/view/panes/details.rs`（并入 CommitDetailsDelayState）
- Modify: `crates/worktree-ui-gpui/src/view/rows/diff.rs`（并入 should_hide_unified_diff_header_line）
- Modify: `crates/worktree-ui-gpui/src/view/panes/main/diff_cache.rs`、`.../diff_cache/patch_diff.rs`（如仍引用，加显式 import——fan-out ≤5 规则）

**Interfaces:**
- Produces: `crate::view::scroll_geometry::{absolute_scroll_y, scroll_is_near_bottom}`（pub(in crate::view)，受众与原 pub(super) 等价）；toast 三函数与 ToastState/Action/DismissBehavior 成为 toast_host 模块项（原 pub(super) 受众不变，唯一消费方即 toast_host 自身，无需 view 根壳）；`rows::diff::should_hide_unified_diff_header_line`（pub(in crate::view)）；`panes::details::CommitDetailsDelayState`。

- [ ] **Step 1: 基线** — `cargo test -p worktree-ui-gpui -- --list > before.txt`（记录总数）
- [ ] **Step 2: 搬移** — 按 Files 移动四个符号簇；新模块 `scroll_geometry.rs` 首行 `use super::*;`（继承原文件头的等价导入面）；toast/CommitDetails/diff 谓词并入目标文件时放在相邻语义区
- [ ] **Step 3: view/mod.rs 接线** — `mod scroll_geometry;` + `use scroll_geometry::{absolute_scroll_y, scroll_is_near_bottom};`（settings_window 与 history_panel 经 glob 链零改动获得）
- [ ] **Step 4: rustfmt** — 仅触及文件：`rustfmt --edition 2024 --config skip_children=true <each file>`
- [ ] **Step 5: 验证** — 四件套 + default 全测；`-- --list` 与 before.txt 逐名一致（含 8 项 ignored 计数）
- [ ] **Step 6: Commit** — `Fold toast, scroll, and single-consumer helpers out of mod_helpers`

### Task 2: view/branch_selection.rs — SelectedBranch family

**Files:**
- Modify: `view/mod_helpers.rs`（删除 22-67）
- Create: `view/branch_selection.rs`（SelectedBranch、selected_branch_label_color、selected_branch_row_bg、SelectedHistoryBranch、selected_branch_for_history_row）
- Modify: `view/mod.rs`（`mod branch_selection;` + 具名 use 五符号）

**Interfaces:**
- Consumes: `BranchSection`（view/branch_sidebar.rs:30，不动）、`RepoId`、`AppTheme`、`with_alpha`（经 `use super::*;` 获得，与现状同源）
- Produces: `crate::view::branch_selection::{SelectedBranch, SelectedHistoryBranch, selected_branch_for_history_row, selected_branch_label_color, selected_branch_row_bg}`；消费方（panes/history、panes/sidebar、rows/history、rows/sidebar）经 glob 链零改动

- [ ] **Step 1-6**: 同 Task 1 模式（基线 → 搬移 → mod.rs 具名 use → rustfmt skip_children → 四件套验证 + list 同一性 → commit `Move the branch selection helpers into their own module`）

### Task 3: view/resize_state.rs — column/split resize state machines

**Files:**
- Modify: `view/mod_helpers.rs`（删除 70-102、360-566，含 `mod resize_drag_ghost_tests`）
- Create: `view/resize_state.rs`（HistoryColResizeHandle/State、ResizeDragGhost、PaneResizeHandle/State、DiffSplitResizeHandle/State、ConflictVSplitResizeHandle/State、StatusSectionResizeHandle/State、ConflictHSplitResizeHandle/State、ConflictDiffSplitResizeHandle/State、resize_drag_ghost_tests）
- Modify: `view/mod.rs`（`mod resize_state;` + 具名 use 全部符号——fan-out >5（mod.rs、rows/benchmarks、tests、panes/history））
- Modify: `view/rows/benchmarks.rs:16-18`（`use crate::view::mod_helpers::{PaneResizeHandle, PaneResizeState, StatusMultiSelection, StatusSection};` → StatusMultiSelection/StatusSection 暂留 mod_helpers（Task 6 移），本任务只把 PaneResizeHandle/PaneResizeState 改指 `crate::view::resize_state`）

**Interfaces:**
- Produces: `crate::view::resize_state::{…上述全部类型}`（pub(super) 等受众）；`rows/benchmarks.rs` 的显式路径改指
- 注意：本任务触及 benchmarks 消费方，四件套之 benchmarks 腿必跑

- [ ] **Step 1-6**: 同 Task 1 模式；commit `Move the resize drag state machines into their own module`

### Task 4: view/preview_kind.rs — preview kind determination

**Files:**
- Modify: `view/mod_helpers.rs`（删除 125-358）
- Create: `view/preview_kind.rs`（is_svg_path、should_bypass_text_file_preview_for_path、RenderableConflictFile、conflict_file_is_binary、renderable_conflict_file、DiffViewMode、RenderedPreviewKind、RenderedPreviewMode、RenderedPreviewModes、ConflictResolverPreviewMode、is_markdown_path、preview_path_rendered_kind、diff_target_rendered_preview_kind、main_diff_rendered_preview_toggle_kind）
- Modify: `view/mod.rs`（`mod preview_kind;` + 具名 use 全部符号——RenderedPreviewKind 8+ 文件 fan-out）

**Interfaces:**
- Produces: `crate::view::preview_kind::{…上述全部}`；is_svg_path 外部零消费（内部依赖，随迁）

- [ ] **Step 1-6**: 同 Task 1 模式；commit `Move preview kind determination into its own module`

### Task 5: rows/text_geometry.rs — diff/conflict text hit geometry

**Files:**
- Modify: `view/mod_helpers.rs`（删除 567-685）
- Create: `crates/worktree-ui-gpui/src/view/rows/text_geometry.rs`（DiffTextRegion、DiffTextPos、DiffTextHitbox、ConflictTextHitbox、DiffTextWrappedHit、DiffTextOffsetMap 及各自 impl）
- Modify: `view/rows/mod.rs`（`mod text_geometry;`）
- Modify: `view/mod.rs`（具名 use 六类型——fan-out 10+ 文件（diff_text_selection、panels/main/diff_view、panels/mod.rs、popover、panels/tests×3、rows/diff_canvas、rows/markdown_flow_text、rows/conflict_canvas））

**Interfaces:**
- Produces: `crate::view::rows::text_geometry::{DiffTextRegion, DiffTextPos, DiffTextHitbox, ConflictTextHitbox, DiffTextWrappedHit, DiffTextOffsetMap}`（可见性升为 `pub(in crate::view)`——消费方本就遍布 view 子树，受众等价）

- [ ] **Step 1-6**: 同 Task 1 模式；commit `Move diff text hit geometry into the rows layer`

### Task 6: view/status_section.rs — status section multi-selection

**Files:**
- Modify: `view/mod_helpers.rs`（删除 735-1075）
- Create: `view/status_section.rs`（StatusSection、StatusSectionFilter、StatusSectionEntries、StatusSectionIndexes、StatusSectionIter、StatusSectionIterInner、status_section_filter_matches、status_section_rev、status_section_is_loading、StatusMultiSelection、reconcile_status_multi_selection、reconcile_status_multi_selection_with_repo）
- Modify: `view/mod.rs`（`mod status_section;` + 具名 use pub 符号）
- Modify: `view/rows/benchmarks.rs:16-18`（剩余 `StatusMultiSelection, StatusSection` 改指 `crate::view::status_section`，本任务后 mod_helpers use 行清空删除）

**Interfaces:**
- Produces: `crate::view::status_section::{…上述全部}`；约 10 消费文件经 glob 链零改动
- benchmarks 腿必跑（rows/benchmarks.rs 显式路径）

- [ ] **Step 1-6**: 同 Task 1 模式；commit `Move the status section selection state into its own module`

### Task 7: Re-bucket the markdown/three-way cluster + tail cache entry

**Files:**
- Modify: `view/mod_helpers.rs`（删除 1076-1385、5874-5877）
- Create: `view/conflict_resolver/three_way.rs`（ThreeWayColumn、ThreeWaySides 及 impl）
- Modify: `view/conflict_resolver.rs` 或其 mod 挂载点（`mod three_way;`，遵循该树现有挂载方式）
- Modify: `view/caches.rs`（并入 deferred_line_starts_for_text、DeferredLineStarts、LoadableMarkdownDoc、LoadableMarkdownDiff、LoadableImagePreview、MarkdownPreviewWrapSlot、MarkdownPreviewWrapCache 及 impl、`mod markdown_preview_wrap_cache_tests`、DiffTextLayoutCacheEntry）
- Modify: `view/mod.rs`（three_way 具名 use——ThreeWayColumn 15 文件 fan-out；caches 增补具名 use：DeferredLineStarts、Loadable 三别名、MarkdownPreviewWrapCache、DiffTextLayoutCacheEntry）

**Interfaces:**
- Produces: `crate::view::conflict_resolver::three_way::{ThreeWayColumn, ThreeWaySides}`；`view::caches` 新增上述缓存项（DeferredLineStarts 的唯一外部消费方 panes/main/conflict_actions.rs 经 glob 链零改动）
- 纯搬移：caches.rs 并入时保留原 `use super::*;` 依赖面

- [ ] **Step 1-6**: 同 Task 1 模式（含测试模块随迁，default 全测 + list 同一性）；commit `Split the three-way and markdown cache vocabulary out of mod_helpers`

### Task 8: Popover vocabulary → panels/popover.rs (+ diff vocab re-homing)

**Files:**
- Modify: `view/mod_helpers.rs`（删除 4388-4955）
- Modify: `crates/worktree-ui-gpui/src/view/panels/popover.rs`（并入 BranchPickerPurpose、RemotePickerPurpose、StashPickerPurpose、AutosquashMode、PopoverKind、RepoPopoverKind、RemotePopoverKind、WorktreePopoverKind、SubmodulePopoverKind、impl PopoverKind、RemoteRow）
- Modify: `view/diff_utils.rs`（并入 DiffClickKind）
- Modify: `view/patch_split.rs`（并入 PatchSplitRow）
- Modify: `view/mod.rs`（具名 use 全部上述符号——PopoverKind 99 文件、DiffClickKind 12、PatchSplitRow 10，全走 view 根壳；再导出用 `pub(crate) use` 级别与原受众等价即可，原为 pub(super)/pub(in crate::view) 则用私有 use）

**Interfaces:**
- Produces: popover 词汇在 `panels::popover`；`view::diff_utils::DiffClickKind`、`view::patch_split::PatchSplitRow`；view 根具名壳承载全部（99 文件零改动）
- 注意：panels/popover.rs 已有子模块树（popover/{geometry,dialog,…} 将在 P3 拆）——并入项放文件本体，不得预先拆分

- [ ] **Step 1-6**: 同 Task 1 模式（benchmarks 腿必跑）；commit `Move the popover vocabulary next to its host panel`

### Task 9: view/view_mode.rs + view/diff_prefs.rs

**Files:**
- Modify: `view/mod_helpers.rs`（删除 4956-5165、5373-5727）
- Create: `view/view_mode.rs`（WorkTreeViewMode、InitialRepositoryLaunchMode、WorkTreeViewConfig、StartupCrashReport、FocusedMergetoolLabels、FocusedMergetoolViewConfig、FocusedMergetoolBootstrap、FocusedMergetoolBootstrapAction、DeferredRepoBootstrap、SubmoduleDiffBootstrap、SubmoduleDiffBootstrapAction、normalize_bootstrap_repo_path、normalize_bootstrap_target_path、normalize_bootstrap_diff_target、focused_mergetool_target_path、canonicalize_path、focused_mergetool_bootstrap_action、submodule_diff_bootstrap_action、renders_full_chrome、show_diff_file_navigation、show_titlebar_repo_tabs、command_palette_available、should_seed_initial_repository_from_session、repository_entry_interstitial_active、should_show_startup_repository_loading_screen、should_show_splash_screen、titlebar_workspace_actions_enabled、ThemeMode 及 impl）
- Create: `view/diff_prefs.rs`（ChangeTrackingView、DiffScrollSync、DiffContentMode、DiffWhitespaceMode 及 impl）
- Modify: `view/mod.rs`（两 mod 声明 + `:252-255` 的 `pub use mod_helpers::{…}` 改指：WorkTreeViewMode、WorkTreeViewConfig、InitialRepositoryLaunchMode、StartupCrashReport、FocusedMergetoolLabels、FocusedMergetoolViewConfig → `view_mode`；其余符号私有具名 use）

**Interfaces:**
- Produces: crate 外部 API 面 `view::{WorkTreeViewMode, WorkTreeViewConfig, InitialRepositoryLaunchMode, StartupCrashReport, FocusedMergetoolLabels, FocusedMergetoolViewConfig}` 不变（re-export 改源，消费方零改动）；`view::diff_prefs::{ChangeTrackingView, DiffScrollSync, DiffContentMode, DiffWhitespaceMode}`（DiffWhitespaceMode 原为 pub(crate)，保持）

- [ ] **Step 1-6**: 同 Task 1 模式；commit `Move view mode, bootstrap, and diff preference vocabulary into modules`

### Task 10: view/terminal_types.rs

**Files:**
- Modify: `view/mod_helpers.rs`（删除 7、4381-4387、5166-5372）
- Create: `view/terminal_types.rs`（`type AlacrittyTermLock` 别名、TerminalMenuContext、TerminalTextMetrics、TerminalGridSize、TerminalLayoutKey、TerminalLayoutCache、TerminalCachedRow、TerminalViewportCacheKey、TerminalRenderCache、TerminalViewportView、TerminalInstance、RepoTerminalSession、TerminalShutdownSummary、TerminalPanelResizeState、BottomPanelTab、TerminalGridPoint 及 impl）
- Modify: `view/mod.rs`（`mod terminal_types;`；`:250` `pub(crate) use mod_helpers::TerminalPanelResizeState;` 改指 `terminal_types`；其余符号具名 use）

**Interfaces:**
- Produces: `crate::view::terminal_types::{…上述全部}`；消费方 terminal_panel.rs、terminal_alacritty.rs、reflog_panel.rs（BottomPanelTab）经 glob 链零改动；mod.rs:250 的 pub(crate) 再导出保持路径 `crate::view::TerminalPanelResizeState` 不变

- [ ] **Step 1-6**: 同 Task 1 模式；commit `Move the terminal type family beside the terminal panels`

### Task 11: view/conflict_resolver/ui_state.rs — the big relocation

**Files:**
- Modify: `view/mod_helpers.rs`（删除 1386-4380）
- Create: `view/conflict_resolver/ui_state.rs`（ConflictResolverMarkdownPreviewState、ConflictResolverImagePreviewState、ResolvedOutputConflictMarker、ResolvedOutlineData、StreamedConflictState、ConflictModeState、ConflictRowSelection、AlignmentLineSelection、ConflictResolverUiState、indexed_line_text、append_conflict_row_without_whitespace、impl ConflictResolverUiState、ResolverPickTarget、ConflictResolverJoinTarget）
- Create: `view/conflict_resolver/tests/ui_state.rs`（原 `mod conflict_resolver_ui_state_tests` 体，改名挂载于既有 `view/conflict_resolver/tests/mod.rs`）
- Modify: `view/mod.rs`（具名 use 上述符号——fan-out 大（panels/main 冲突系 + tests））

**Interfaces:**
- Consumes: Task 7 的 `conflict_resolver::three_way`（若 ui_state 体引用 ThreeWayColumn——编译器指认则显式 use）
- Produces: `crate::view::conflict_resolver::ui_state::{…上述全部}`；869 处 `conflict_resolver::` 前缀引用不受影响（该前缀本就指向 view/conflict_resolver.rs 树）
- 搬迁规模 ~3,000 行：沿用 P0 Task 8 纪律——分块搬运（状态类型 / 自由函数 / 巨型 impl / tests 四块），每块后 `cargo check -p worktree-ui-gpui` 增量验证；测试名同一性按 `conflict_resolver` 过滤集核对（245 冲突域测试基线）

- [ ] **Step 1: 基线** — `cargo test -p worktree-ui-gpui conflict_resolver -- --list > before.txt`
- [ ] **Step 2: 分块搬移** — 四块顺序迁移 + 每块 cargo check
- [ ] **Step 3: 接线** — conflict_resolver 树挂载 + view/mod.rs 具名 use
- [ ] **Step 4: rustfmt** — 触及文件 skip_children=true
- [ ] **Step 5: 验证** — 四件套 + default 全测；conflict_resolver 过滤集逐名一致；行数守恒（mod_helpers -2,995 / 新文件 +等量）
- [ ] **Step 6: Commit** — `Move ConflictResolverUiState into the conflict resolver tree`

### Task 12: Final — WorkTreeView move + delete mod_helpers.rs + kill the glob root

**Files:**
- Modify: `view/mod_helpers.rs` → **删除文件**
- Create: `view/worktree_view.rs`（`struct WorkTreeView`，145 字段，5728-5872）
- Modify: `view/mod.rs`（`mod worktree_view;`；删除 `:174 mod mod_helpers;`、`:251 use mod_helpers::*;`；`:252` pub use 中 `WorkTreeView` 改指 worktree_view）

**Interfaces:**
- Produces: `crate::view::worktree_view::WorkTreeView`；`view::WorkTreeView` 公共路径不变
- 前置：任务 1-11 全部完成（mod_helpers.rs 此时应仅剩 `use` 头 + `struct WorkTreeView`）

- [ ] **Step 1: 残留清单** — `grep -n "^pub\|^struct\|^impl\|^enum\|^fn\|^const\|^type\|^mod" view/mod_helpers.rs` 输出应为空（除 WorkTreeView 块）——若非空，枚举残留项按 File Map 补迁（controller 裁决归处）
- [ ] **Step 2: 搬移 + 删除** — WorkTreeView → worktree_view.rs；`git rm view/mod_helpers.rs`；清理 mod.rs 三处引用
- [ ] **Step 3: 验证（全矩阵）** — 四件套 + `cargo test --workspace`（default）；gix 腿总数与 P0 合并后基线一致量级（5934 passed / 8 ignored）；`cargo build -p worktree --features ui-gpui,gix`
- [ ] **Step 4: 度量** — `wc -l` 确认 mod_helpers.rs 不存在；`grep -rn "use mod_helpers" crates/` 为 0；`grep -c "pub(crate) use\|^use" view/mod.rs` 记录新具名面规模
- [ ] **Step 5: Commit** — `Delete mod_helpers.rs and move WorkTreeView to its own module`

---

## Self-Review 结论（写计划时已核）

1. **Spec 覆盖**：spec P1 表 11 行全覆盖（PopoverKind→任务 8、ConflictResolverUiState→11、终端族→10（TerminalMenuContext 归位修正 R2）、拖拽分隔条→3、StatusSection→6、Toast→1、预览判定→4、滚动/命中→1+5、WrapCache/Markdown→7、view_mode/bootstrap/主题/diff 偏好→9、WorkTreeView→12）；收尾 glob 根消灭→12。偏差 7 项均载入 Plan-Level Rulings。
2. **占位符扫描**：各任务的符号清单、行段、命令均为实值，无 TBD。
3. **类型一致性**：任务 3/6 共享 `rows/benchmarks.rs:16-18` 的改指（任务 3 改一半、任务 6 清尾），已显式分工；任务 11 可能消费任务 7 的 three_way（编译器指认）已注明。
