# P2 结构归位（Structural Relocation）Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把 P1 解散后的游离结构归位：MainPaneView 定义与 impl 同树、panels/main 渲染半边并入 panes/main 行为半边、DetailsPaneView/ContextMenuModel 各归其位、`pub(crate) use helpers::*` 匿名泄漏降级为显式 re-export——panels/ 回归窗口级 chrome 本位。

**Architecture:** 五个搬迁全部沿用 P1 验证过的"纯搬移 + 具名接线"模式（Ruling W2：具名 re-export 块为默认机制；禁一切新 `use xxx::*` 根 glob）。每个任务独立验证、独立提交。任务顺序按依赖排定：先搬 struct（P2.1），再并树（P2.2——树合并后 helpers glob 的消费者集合才稳定），再 DetailsPaneView/ContextMenuModel（P2.3，独立于前两项），命名归位（P2.4，独立），最后拆 helpers glob（P2.5——必须在 P2.1/P2.2 之后，因为 struct 搬走后 helpers.rs 的可见面才收窄到稳定集合）。

**Tech Stack:** Rust 2024 edition，cargo workspace（worktree-core / worktree-git-gix / worktree-state / worktree-ui-gpui），rustfmt `--config skip_children=true` 单文件格式化纪律。

**Spec:** docs/superpowers/specs/2026-08-31-rust-codebase-refactor-design.md（P2 节为权威——100-112 行）

## Global Constraints

（继承 P1 全部纪律，以下为逐条重申）

1. `cargo clippy --workspace --no-default-features --features gix -- -D warnings` — **基线豁免制**：门禁 = 错误集 ⊆ 仓库根 `clippy-baseline.txt`（31 行 file:line 清单）。基线内条目随任务搬移同文件时可等位更新，不得新增。非 ui-gpui crate 必须零错误。
2. `cargo test --workspace --no-default-features --features gix` — EXIT=0（已知环境抖动两枚：`standalone_tool_mode_integration`（worktree crate）与 `view::panels::popover::tests::picker::rebase_onto_picker_excludes_current_branch_and_opens_confirm`（gpui 视觉测试）；失败时单目标重跑复核一次，重跑过即记录两份 EXIT 继续）。
3. `cargo test --workspace`（default features）— EXIT=0。
4. `cargo test -p worktree-ui-gpui -- --list` — 测试名集前后同一（随迁测试模块的路径改名除外，需逐一列出并证明去路径名集相等）。
5. **纯搬移**：函数体逐字保持（受众保持的可见性改写除外：`pub(super)`/`pub(crate)` ↔ `pub(in crate::view)` 等价升/降级）。非 ASCII 字符（省略号、em-dash）必须逐字节存活。
6. **W2 接线规则**：具名 re-export 块为默认机制；单消费方场景用消费方显式 import；**禁止新增任何 `use xxx::*` 根 glob**（本计划的存在意义之一就是消灭 P1 遗留的这一个）。
7. **G1 门禁顺序**：最后一次全量验证之后不得有任何源码改动；若必须改（如 clippy 触发删 unused re-export），四腿全部重跑。lib 视角 unused ≠ test 视角 unused——测试专用导入用 `#[cfg(test)]` 门控，不要删。
8. rustfmt 只对触及文件执行：`rustfmt --edition 2024 --config skip_children=true <files>`，禁止 crate 级 cargo fmt。
9. cargo 验证腿必须串行执行（并发 cargo 会死锁 build-dir 锁——P1 Task 3 教训）。
10. 简报/文档中的行号仅供参考（先期任务删行会漂移）——**以符号名定位为准**。

---

## File Map

| 动作 | 文件 | 职责 |
|---|---|---|
| Create | `panes/main/state.rs` | `struct MainPaneView`（515 行，2999-3514）及其字段类型别名——定义与构造 |
| Modify | `panes/main/helpers.rs` | 删除 struct 区段（2999-3514），保留全部自由函数/类型/测试 |
| Modify | `panes/main.rs` | `mod state;` + glob 拆解（P2.5 在此落地） |
| Move | `panels/main/*.rs`（9 文件 + main.rs，10,696 行）→ `panes/main/` | 渲染半边并入行为半边，单一 MainPaneView 树 |
| Modify | `panels/mod.rs` | 删 `mod main;`；ContextMenuModel 族（540-731）外迁后删对应段 |
| Create | `components/context_menu_model.rs` | ContextMenuModel/Item/Segment/Rows 族（约 190 行） |
| Modify | `components/mod.rs` | `mod context_menu_model;` + 具名 re-export |
| Create | `panes/details/`（目录）+ `panes/details/{mod,render_impl}.rs` | `impl DetailsPaneView`（layout.rs:447-3512，约 3,065 行）迁入 |
| Modify | `panels/layout.rs` | 删 447-3512 段 |
| Move | `view/reflog_panel.rs` → `view/panels/reflog_host.rs`（归位 + 改名） | WorkTreeView 的 reflog 面板宿主 impl（402 行） |
| Rename | `panels/main/conflict_resolver_view.rs` 搬家时改名 `panes/main/conflict_resolver_render.rs` | 消除与 `view/conflict_resolver.rs`（逻辑层）的命名歧义 |

---

### Task 1: MainPaneView struct → panes/main/state.rs

**Files:**
- Create: `crates/worktree-ui-gpui/src/view/panes/main/state.rs`
- Modify: `crates/worktree-ui-gpui/src/view/panes/main/helpers.rs`（删除 struct 区段——以 `pub(crate) struct MainPaneView {` 定位，至其闭括号；P1 后基线约 2999-3514 行）
- Modify: `crates/worktree-ui-gpui/src/view/panes/main.rs`（`mod state;` + `pub(crate) use state::MainPaneView;`）

**Interfaces:**
- Consumes: 无新依赖——struct 字段类型全部经 `use super::*;`（panes/main 命名空间）解析，与原helpers.rs 同源
- Produces: `crate::view::panes::main::MainPaneView`（路径与可见性 `pub(crate)` 不变）；helpers.rs 的 `impl` 块与其余 11 个 panes/main 文件的 `impl MainPaneView` 块经 `use super::*` 链零改动

- [ ] **Step 1: 基线** — `git rev-parse HEAD` 记录 BASE；`rtk proxy cargo test -p worktree-ui-gpui -- --list > /tmp/p2t1-list-before.txt 2>&1` 确认 3564。
- [ ] **Step 2: 搬移** — 新建 state.rs：顶部 `use super::*;`，struct MainPaneView 连同其 derive 行逐字迁入。helpers.rs 删除该区段。**注意**：struct 区段内若有紧邻的 `impl Default for MainPaneView`/构造函数，一并随迁（以编译器指认为准——struct 的关联构造逻辑应与定义同住；helpers.rs 只留自由函数与无关类型）。
- [ ] **Step 3: 接线** — panes/main.rs 加 `mod state;`（按现有 mod 声明排序惯例）+ `pub(crate) use state::MainPaneView;`。helpers.rs 及全部 `impl MainPaneView` 文件零改动（经 `use super::*` 链解析）。
- [ ] **Step 4: rustfmt** — `rustfmt --edition 2024 --config skip_children=true state.rs helpers.rs main.rs`（panes/main/ 路径下）。
- [ ] **Step 5: 四腿验证** — gix 全测（EXIT 纪律 + could-not-compile/error[ 扫描）；default 全测；clippy 基线比对（消息+位置对；触及文件内基线站点行号漂移须报告映射，如 settings_window 类似情况）；list 同一性（无随迁测试模块则零差异）。
- [ ] **Step 6: Commit** — `git add <files> && git commit -m "Move the MainPaneView struct next to its impls"`。

### Task 2: panels/main 渲染半边并入 panes/main

**Files:**
- Move: `panels/main/{binary_conflict,conflict_resolver_view,decision_conflict,diff,diff_view,diff_view_helpers,keep_delete_conflict,lfs_pointer,status_nav}.rs` → `panes/main/`（conflict_resolver_view.rs 同时改名 conflict_resolver_render.rs）
- Move+Delete: `panels/main.rs`（48 行：4 个 `pub(super)` 自由函数）→ 并入 `panes/main.rs` 或新文件 `panes/main/conflict_chrome.rs`（见 Step 2 裁决）
- Modify: `panels/mod.rs`（删 `mod main;` 行）
- Modify: `panels/tests/mod.rs`（`pub(super) use super::main::{...4 函数}` 改指新路径）
- Modify: `panes/main.rs`（9 个新 `mod` 声明）

**Interfaces:**
- Consumes: Task 1 的 `state::MainPaneView`（并入后的文件仍经 `use super::*` 解析）
- Produces: `panes/main/` 单树承载 MainPaneView 全部 impl（行为 + 渲染）；`panels/` 不再有 main 子树

- [ ] **Step 1: 基线** — 同 Task 1 模式（/tmp/p2t2-*）。
- [ ] **Step 2: 文件搬迁** — `git mv` 9 文件至 panes/main/（conflict_resolver_view.rs → conflict_resolver_render.rs）。panels/main.rs 的 4 个函数（show_external_mergetool_actions、show_conflict_save_stage_action、conflict_side_output_bytes、next_conflict_diff_split_ratio）裁决去向：它们是冲突渲染 chrome 的判定函数，消费方全在 main 树内部与 panels/tests——并入新文件 `panes/main/conflict_chrome.rs`（顶部 `use super::*;`），panels/main.rs 删除。
- [ ] **Step 3: 接线** — panes/main.rs 加 10 个 mod 声明（binary_conflict、conflict_chrome、conflict_resolver_render、decision_conflict、diff、diff_view、diff_view_helpers、keep_delete_conflict、lfs_pointer、status_nav；与既有 mod 排序惯例一致）。panels/mod.rs 删 `mod main;`。panels/tests/mod.rs 的 `pub(super) use super::main::{...}` 改为 `pub(super) use crate::view::panes::main::{conflict_side_output_bytes, next_conflict_diff_split_ratio, show_conflict_save_stage_action, show_external_mergetool_actions};`（具名，禁 glob）。搬迁文件内部的 `use super::*;` 语义自动翻转（super 从 panels/main.rs 变为 panes/main.rs——两者都 `use super::*` 进 view 根，命名空间面等价；但 panels/main.rs 原 48 行内的 4 函数不再经旧 super 可达，已由 Step 2 并入解决）。**编译器是接线净**：逐文件修 E0433/E0425，全部以具名 import 修，禁 glob。
- [ ] **Step 4: rustfmt** — 触及文件（含 git mv 后的 9+2 文件与两个 mod.rs、tests/mod.rs）。
- [ ] **Step 5: 四腿验证** — 同 Task 1；list 腿预期有随迁测试模块路径改名（若 panels/main 内有 #[cfg(test)] mod）——逐一列出并证明去路径名集相等。
- [ ] **Step 6: Commit** — `git add -A <files> && git commit -m "Fold the panels/main render tree into panes/main"`。

### Task 3: impl DetailsPaneView → panes/details/

**Files:**
- Create: `crates/worktree-ui-gpui/src/view/panes/details/mod.rs`（模块壳：`use super::*;` + 子模块声明）
- Create: `crates/worktree-ui-gpui/src/view/panes/details/render_impl.rs`（`impl DetailsPaneView` 区段——layout.rs 447-3512，约 3,065 行，含其前的 StatusSectionActionSelection 辅助 377-445 一并随迁）
- Modify: `crates/worktree-ui-gpui/src/view/panels/layout.rs`（删除 377-3512 段）
- Modify: `crates/worktree-ui-gpui/src/view/panes/mod.rs`（`mod details;`——注意 panes/details.rs 单文件升级为目录；若 Rust 的 mod 解析冲突，将原 details.rs 的内容并入 details/mod.rs 头部）
- Delete 或并入: `crates/worktree-ui-gpui/src/view/panes/details.rs`（1,901 行——struct DetailsPaneView 定义所在；裁决：其内容并入 details/mod.rs，则定义与 impl 同树归位，与 Task 1 的 MainPaneView 模式呼应）

**Interfaces:**
- Consumes: `DetailsPaneView`（定义在 panes/details.rs:61）、`StatusSection`/`StatusSectionEntries`（P1 已入 view 命名空间）
- Produces: `panes/details/` 目录树（定义 + 行为 impl 同树）；layout.rs 回归纯布局骨架（约 470 行）

- [ ] **Step 1: 基线** — 同前（/tmp/p2t3-*）。
- [ ] **Step 2: 目录化** — panes/details.rs → panes/details/mod.rs（git mv，内容不动）。panes/mod.rs 的 `mod details;` 声明零改动（目录 mod.rs 自动解析）。
- [ ] **Step 3: 搬迁** — layout.rs 377-445（StatusSectionActionSelection 族）+ 447-3512（impl DetailsPaneView）逐字迁入新文件 panes/details/render_impl.rs（顶部 `use super::*;`）。layout.rs 删除该段。panes/details/mod.rs 加 `mod render_impl;`。**可见性裁决**：迁入项原 `pub(super)` 指 panels 作用域，现指 panes/details——编译器指认需调整者按"受众保持"原则改写（`pub(in crate::view)` 等）。
- [ ] **Step 4: rustfmt + 四腿验证** — 同前（/tmp/p2t3-*；layout.rs 内基线站点若因删段行号漂移，报告映射）。
- [ ] **Step 5: Commit** — `git add -A <files> && git commit -m "Relocate DetailsPaneView rendering next to its definition"`。

### Task 4: ContextMenuModel 族 → components/

**Files:**
- Create: `crates/worktree-ui-gpui/src/view/components/context_menu_model.rs`（panels/mod.rs 540-731 段：ContextMenuItem、ContextMenuSegment、ContextMenuModel、ContextMenuRows + impls，约 190 行）
- Modify: `crates/worktree-ui-gpui/src/view/components/mod.rs`（`mod context_menu_model;` + 具名 re-export）
- Modify: `crates/worktree-ui-gpui/src/view/panels/mod.rs`（删除该段；消费方 popover/context_menu.rs 与 popover/context_menu/mergetool_settings.rs 经链解析）

**Interfaces:**
- Consumes: `ContextMenuAction`（panels/mod.rs:39——留在 panels，窗口 chrome 词汇）
- Produces: `components::context_menu_model::{ContextMenuModel, ContextMenuItem, ContextMenuSegment, ContextMenuRows}`；消费方 popover/context_menu.rs（ContextMenuRows ×3 处）与 mergetool_settings.rs（ContextMenuSegment ×2 + ContextMenuModel 引用）经 `use super::*` 链零改动或具名 import（编译器指认）

- [ ] **Step 1: 基线** — 同前（/tmp/p2t4-*）。
- [ ] **Step 2: 搬迁** — 540-731 段逐字迁入新文件（顶部 `use super::*;`）。panels/mod.rs 删段。components/mod.rs 接线（具名 re-export：ContextMenuModel 族全部 4 名——消费方在 components 外）。
- [ ] **Step 3: 可见性裁决** — 原段私有项（struct ContextMenuModel 无 pub 前缀）迁入 components 后对 popover 消费方不可达——按受众保持升级（`pub(in crate::view)` 或 `pub(super)` 链，编译器指认后逐一处理并记录）。
- [ ] **Step 4: rustfmt + 四腿验证** — 同前。
- [ ] **Step 5: Commit** — `git add <files> && git commit -m "Move the context menu model into components"`。

### Task 5: reflog 命名归位 + conflict 命名去歧义

**Files:**
- Move: `crates/worktree-ui-gpui/src/view/reflog_panel.rs` → `crates/worktree-ui-gpui/src/view/panels/reflog_host.rs`（它实现的是 WorkTreeView 的 reflog 面板宿主逻辑——panels/ 的窗口 chrome 本位）
- Modify: `crates/worktree-ui-gpui/src/view/mod.rs`（`mod reflog_panel;` → `mod panels::reflog_host` 声明迁移；实现者按 panels/mod.rs 挂载惯例处理）
- Rename（Task 2 已随迁改名的确认项）: conflict_resolver_render.rs 名称确认——与 `view/conflict_resolver.rs`（逻辑层）、`panes/main/conflict_actions.rs`（行为层）三者命名已各表其义

**Interfaces:**
- Consumes: `ReflogPaneView`（panes/reflog.rs——不动的真正面板）
- Produces: `panels::reflog_host`（WorkTreeView 的 reflog 宿主 impl）；spec 第 4 条"reflog_panel.rs 与 panes/reflog.rs 归位"闭合

- [ ] **Step 1: 基线** — 同前（/tmp/p2t5-*）。
- [ ] **Step 2: 搬迁** — git mv + panels/mod.rs 挂载（`mod reflog_host;`）。文件内 `impl WorkTreeView` 逻辑零改动；可见性按受众保持（原 `pub(super)` 指 view 根，现指 panels——若消费方在 view 根则升 `pub(in crate::view)` 并具名 re-export）。
- [ ] **Step 3: rustfmt + 四腿验证** — 同前。
- [ ] **Step 4: Commit** — `git add -A <files> && git commit -m "Rename the reflog panel host into the panels tree"`。

### Task 6: 撤销 helpers glob——降级为显式 re-export 清单

**Files:**
- Modify: `crates/worktree-ui-gpui/src/view/panes/main.rs:27`（`pub(crate) use helpers::*;` → `pub(crate) use helpers::{...};` 具名清单）

**Interfaces:**
- Consumes: Task 1 后 helpers.rs 的稳定可见面——110 个 `pub(in crate::view)` 项（struct 搬走后）中有树外消费者的那部分
- Produces: panes/main 的显式再导出清单（消费方零改动——它们已通过 `crate::view::panes::main::X` 或 glob 链消费这些名字）；spec P2 第 5 条闭合（"先降级为显式 re-export 清单"，与 P3 的 helpers 拆分联动）

**注意**：本任务只做 glob → 具名清单的机械降级，**不做** helpers.rs 的进一步拆分（那是 P3 的 `panes/main/helpers.rs` 行）。清单生成法：删 glob 行 → `cargo check` 收 E0433/E0425 全集 → 按错误指认逐名补进具名 use 块 → 直至编译净。清单内名字必须逐一与 helpers.rs 的 `pub(in crate::view)` 定义核对（`grep "pub(in crate::view)" helpers.rs` 的 110 项中取树外消费子集）。

- [ ] **Step 1: 基线** — 同前（/tmp/p2t6-*）。
- [ ] **Step 2: 降级** — 删 glob 行，编译器指认法生成具名清单（见上）。清单按字母排序。
- [ ] **Step 3: rustfmt + 四腿验证** — 同前。**附加终证**：`grep -c "use helpers::\*" panes/main.rs` = 0；`grep -rn "pub(crate) use .*::\*" crates/worktree-ui-gpui/src --include="*.rs"` 的输出中 panes/main.rs 不再出现（其余命中为 P1 前既有且非 view 注入根者——如实记录，不在本任务范围内扩大）。
- [ ] **Step 4: Commit** — `git add <files> && git commit -m "Replace the helpers glob export with a named re-export list"`。

---

## Self-Review 结论

1. **Spec 覆盖**：P2 节 5 条全部映射——第 1 条=Task 1；第 2 条=Task 2（含 panels 回归 chrome 本位：Task 2/3/4/5 各自迁走非 chrome 内容）；第 3 条=Task 3+4；第 4 条=Task 5（conflict 命名三义在 Task 2 改名 + Task 5 确认中闭合；spec 注明"借 P3 拆分改名"的部分——conflict_resolver.rs 逻辑层的最终改名留给 P3，本阶段完成 view/panels/panes 三方名实对应）；第 5 条=Task 6。
2. **占位符扫描**：无 TBD/TODO；每个 Step 均有具体操作与验证命令。
3. **类型一致性**：MainPaneView 路径在 Task 1 产出后 Task 2-6 均沿用；ContextMenuModel 族 4 名在 Task 4 产出块与消费方一致。

## 执行顺序依赖

Task 1 → Task 2（struct 先归位，树合并的接线面才最小）→ Task 6（glob 拆解必须在 1+2 后——helpers.rs 可见面稳定）。Task 3、4、5 相互独立，可在 1-2-6 主线前后任意插入；建议串行 1→2→6→3→4→5（glob 拆解越早，后续任务的接线面越显式）。
