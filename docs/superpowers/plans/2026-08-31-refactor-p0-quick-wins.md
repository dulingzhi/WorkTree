# P0 快赢阶段实施计划（Quick Wins Implementation Plan）

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 执行重构方案 P0 阶段的 8 项零/低风险动作：测试外迁、两处视图拆分、一处测试拷贝删除、两对镜像函数合并、语法注册表宏化、空壳文件删除——全部行为不变。

**Architecture:** 纯搬移 + 局部等价改写。每个 Task 独立提交、独立可回滚；公开符号名全部保留（内部实现收敛），调用方零改动。验证依赖既有测试套件的"数量与结果前后一致"。

**Tech Stack:** Rust 2024 edition，workspace 见根 `Cargo.toml`（UI crate = `crates/worktree-ui-gpui`）。

**Spec:** `docs/superpowers/specs/2026-08-31-rust-codebase-refactor-design.md`（§5 P0 表格）

## Global Constraints

- 行为不变：任何 Task 都不得改变测试结果或用户可见行为。
- 本地验证命令（每个 Task 的提交门槛）：
  - `cargo test -p worktree-ui-gpui <过滤器>`（见各 Task，结果与基线一致）
  - `cargo clippy -p worktree-ui-gpui -- -D warnings`（零警告）
- 合并/推送前的完整 CI 三件套（CONTRIBUTING.md 定义）：
  - `cargo clippy --workspace --no-default-features --features gix -- -D warnings`
  - `cargo test --workspace --no-default-features --features gix`
  - `cargo build -p worktree --features ui-gpui,gix`
- 提交信息风格：祈使句、无 conventional-commit 前缀（与 `git log` 现有风格一致，如 "Stop truncating staged reads to the open-time CWD prefix"）。
- 禁止新增 glob 再导出（`pub use xxx::*`）；新代码用显式导入。
- 可见性规范：跨模块 `pub(crate)` / `pub(in crate::view)`，模块内 `pub(super)`；禁止 `pub(in super::super::super)`。
- 平台：本地 Windows 执行，CI Linux 复验；本计划不触碰任何平台 `#[cfg]` 分叉。
- 基线：dev 分支，spec 已提交于 `1b9e698`。开始前 `git status` 必须干净；在 `dev` 上逐 Task 提交（或按执行者偏好每个 Task 一个分支）。

## Spec 偏差说明（经第一手代码核验）

Spec P0 行"conflict_jump 六件套 → 方向枚举参数"高估了收益：`panes/main/conflict_actions.rs:633-713` 的六个方法已是 8-10 行薄封装，参数化它们省不了行数还伤可读性。真实镜像在 `view/conflict_resolver.rs:361-432` 的两对自由函数（仅 `.rev()` 与比较方向不同）。本计划将去重落到自由函数对上（Task 4），薄封装保持原样。该偏差不改变 P0 的目标与验收。

---

### Task 1: 删除 panels/main/history.rs 空壳

**Files:**
- Delete: `crates/worktree-ui-gpui/src/view/panels/main/history.rs`（全文 2 行注释，无任何符号）
- Modify: `crates/worktree-ui-gpui/src/view/panels/main.rs:9`（删 `mod history;`）

**Interfaces:**
- Consumes: 无。
- Produces: 无（`panels::main::history` 模块本就无符号；grep 确认全库无 `main::history` 引用）。

- [ ] **Step 1: 确认无引用**

Run: `grep -rn "main::history" crates/`
Expected: 无输出。

- [ ] **Step 2: 删除声明与文件**

`crates/worktree-ui-gpui/src/view/panels/main.rs` 第 9 行 `mod history;` 删除；删除文件 `crates/worktree-ui-gpui/src/view/panels/main/history.rs`。

- [ ] **Step 3: 验证**

Run: `cargo check -p worktree-ui-gpui 2>&1 | tail -3`
Expected: `Finished`，零 error。
Run: `cargo clippy -p worktree-ui-gpui -- -D warnings 2>&1 | tail -3`
Expected: 零警告。

- [ ] **Step 4: 提交**

```bash
git add -A crates/worktree-ui-gpui/src/view/panels
git commit -m "Delete the empty panels/main history stub"
```

---

### Task 2: 删除 whitespace 渲染的测试专用拷贝

**Files:**
- Modify: `crates/worktree-ui-gpui/src/view/rows/diff_text.rs:430`（`whitespace_visible_text_and_highlights_impl` 提升 `pub(super)`）
- Modify: `crates/worktree-ui-gpui/src/view/rows/conflict_resolver.rs:2967-3007`（两个 `#[cfg(test)]` 函数改为委托）

**Interfaces:**
- Consumes: 无。
- Produces: `rows::diff_text::whitespace_visible_text_and_highlights_impl` 变为 `pub(super)`（rows 子树可见），签名不变：
  `fn(text: &str, highlights: &[(Range<usize>, gpui::HighlightStyle)], append_eol_marker: bool) -> (SharedString, Vec<(Range<usize>, gpui::HighlightStyle)>)`

背景：`rows/conflict_resolver.rs:2973` 的 `whitespace_visible_text_and_highlights` 与 `rows/diff_text.rs:430` 的 `_impl` 版本逐行相同（唯一差异：`_impl` 多 `append_eol_marker` 参数与对应 4 行逻辑）。拷贝传 `false` 即完全等价。

- [ ] **Step 1: 捕获基线**

Run: `cargo test -p worktree-ui-gpui rows::conflict_resolver 2>&1 | tail -2`
Expected: 全绿，记录通过数 N。

- [ ] **Step 2: 提升可见性**

`rows/diff_text.rs:430`：
```rust
pub(super) fn whitespace_visible_text_and_highlights_impl(
```
（仅添加 `pub(super)`，其余不动。）

- [ ] **Step 3: 替换拷贝为委托**

`rows/conflict_resolver.rs` 用下面内容替换 2967-3007 的两个函数体（保留函数名与签名）：
```rust
#[cfg(test)]
fn whitespace_visible_text(text: &str) -> SharedString {
    whitespace_visible_text_and_highlights(text, &[]).0
}

#[cfg(test)]
fn whitespace_visible_text_and_highlights(
    text: &str,
    highlights: &[(Range<usize>, gpui::HighlightStyle)],
) -> (SharedString, Vec<(Range<usize>, gpui::HighlightStyle)>) {
    super::diff_text::whitespace_visible_text_and_highlights_impl(text, highlights, false)
}
```

- [ ] **Step 4: 验证等价**

Run: `cargo test -p worktree-ui-gpui rows::conflict_resolver 2>&1 | tail -2`
Expected: 全绿，通过数 == N。
Run: `cargo clippy -p worktree-ui-gpui -- -D warnings 2>&1 | tail -3`
Expected: 零警告（若报 `unused import`，按提示删除 `rows/conflict_resolver.rs` 头部因此不再使用的导入）。

- [ ] **Step 5: 提交**

```bash
git add crates/worktree-ui-gpui/src/view/rows/diff_text.rs crates/worktree-ui-gpui/src/view/rows/conflict_resolver.rs
git commit -m "Reuse diff_text whitespace rendering in conflict resolver tests"
```

---

### Task 3: 合并 diff_jump_prev / diff_jump_next 镜像体

**Files:**
- Modify: `crates/worktree-ui-gpui/src/view/panes/main/actions_impl.rs:483-527`

**Interfaces:**
- Consumes: `self.diff_nav_entries()`、`self.diff_focus_visible_range() -> Option<(usize, usize)>`、`diff_navigation::diff_nav_prev_target / diff_nav_next_target`、`self.scroll_diff_to_item_strict`、`self.clear_diff_navigation_selection`、字段 `diff_selection_anchor` / `diff_selection_range`（全部已存在，签名不变）。
- Produces: `diff_jump_prev` / `diff_jump_next` 公开签名不变（`pub(in crate::view) fn(&mut self)`）；新增私有 `diff_jump_to_adjacent_change(&mut self, forward: bool)`。

两函数仅 2 处差异：焦点区间取 `start` 还是 `end`、调 prev 还是 next 目标函数。其余 20 行逐字相同。

- [ ] **Step 1: 捕获基线**

Run: `cargo test -p worktree-ui-gpui 2>&1 | tail -2`
Expected: 全绿，记录总数 T（diff 导航行为由 `panels/tests/shortcuts.rs` 断言覆盖）。

- [ ] **Step 2: 替换实现**

`actions_impl.rs` 用以下内容整体替换 483-527 两个函数：
```rust
    pub(in crate::view) fn diff_jump_prev(&mut self) {
        self.diff_jump_to_adjacent_change(false);
    }

    pub(in crate::view) fn diff_jump_next(&mut self) {
        self.diff_jump_to_adjacent_change(true);
    }

    fn diff_jump_to_adjacent_change(&mut self, forward: bool) {
        let entries = self.diff_nav_entries();
        let focus_range = self.diff_focus_visible_range();
        let current = focus_range
            .map(|(start, end)| if forward { end } else { start })
            .unwrap_or(0);
        if entries.is_empty() {
            return;
        }

        let target = if forward {
            diff_navigation::diff_nav_next_target(&entries, current)
        } else {
            diff_navigation::diff_nav_prev_target(&entries, current)
        };
        let Some(target) = target else {
            if focus_range.is_some() {
                self.clear_diff_navigation_selection();
                self.diff_selection_range = Some((current, current));
            }
            self.diff_selection_anchor = Some(current);
            return;
        };

        self.scroll_diff_to_item_strict(target, gpui::ScrollStrategy::Center);
        self.clear_diff_navigation_selection();
        self.diff_selection_anchor = Some(target);
        self.diff_selection_range = Some((target, target));
    }
```

- [ ] **Step 3: 验证等价**

Run: `cargo test -p worktree-ui-gpui 2>&1 | tail -2`
Expected: 全绿，总数 == T。
Run: `cargo clippy -p worktree-ui-gpui -- -D warnings 2>&1 | tail -3`
Expected: 零警告。

- [ ] **Step 4: 提交**

```bash
git add crates/worktree-ui-gpui/src/view/panes/main/actions_impl.rs
git commit -m "Share one body between diff_jump_prev and diff_jump_next"
```

---

### Task 4: 合并 conflict 导航两对镜像自由函数

**Files:**
- Modify: `crates/worktree-ui-gpui/src/view/conflict_resolver.rs:361-432`

**Interfaces:**
- Consumes: `conflict_nav_anchor_order`（:353）、`sole_matching_anchor_index`（:400）、类型 `ConflictNavTarget` / `ConflictNavAnchor` / `ConflictNavTargetFilter`（均在本文件）。
- Produces: 四个 `pub(in crate::view)` 函数签名不变（`conflict_actions.rs` 的六个薄封装方法继续调用它们）；新增模块私有 `enum ConflictNavDirection` 与两个私有核心函数。

`previous_/next_conflict_nav_target_index`（:361-388）互为镜像：`.rev()` 与 `<`/`>` 之差；`previous_/next_..._or_sole_anchor`（:414-432）互为镜像：仅委托目标不同。

- [ ] **Step 1: 捕获基线**

Run: `cargo test -p worktree-ui-gpui conflict 2>&1 | tail -2`
Expected: 全绿，记录通过数 C（conflict_resolver 245 个测试 + panels/tests/conflict.rs）。

- [ ] **Step 2: 替换实现**

`conflict_resolver.rs` 用以下内容整体替换 361-388 与 414-432 的四个函数（:390-412 的 `sole_matching_anchor_index` 及其文档注释保持原位不动）：
```rust
/// Direction for anchored conflict-navigation searches.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ConflictNavDirection {
    Previous,
    Next,
}

fn conflict_nav_target_index_in_direction(
    direction: ConflictNavDirection,
    targets: &[ConflictNavTarget],
    anchor: Option<ConflictNavAnchor>,
    filter: ConflictNavTargetFilter,
) -> Option<usize> {
    let anchor = anchor?;
    let current_order = conflict_nav_anchor_order(targets, anchor);
    let hit = match direction {
        ConflictNavDirection::Previous => targets
            .iter()
            .enumerate()
            .rev()
            .find(|(_, target)| target.order < current_order && filter.matches(target)),
        ConflictNavDirection::Next => targets
            .iter()
            .enumerate()
            .find(|(_, target)| target.order > current_order && filter.matches(target)),
    };
    hit.map(|(index, _)| index)
}

pub(in crate::view) fn previous_conflict_nav_target_index(
    targets: &[ConflictNavTarget],
    anchor: Option<ConflictNavAnchor>,
    filter: ConflictNavTargetFilter,
) -> Option<usize> {
    conflict_nav_target_index_in_direction(
        ConflictNavDirection::Previous,
        targets,
        anchor,
        filter,
    )
}

pub(in crate::view) fn next_conflict_nav_target_index(
    targets: &[ConflictNavTarget],
    anchor: Option<ConflictNavAnchor>,
    filter: ConflictNavTargetFilter,
) -> Option<usize> {
    conflict_nav_target_index_in_direction(ConflictNavDirection::Next, targets, anchor, filter)
}
```
以及（放在原 :414 位置）：
```rust
fn conflict_nav_target_index_or_sole_anchor_in_direction(
    direction: ConflictNavDirection,
    targets: &[ConflictNavTarget],
    anchor: Option<ConflictNavAnchor>,
    filter: ConflictNavTargetFilter,
) -> Option<usize> {
    let anchor = anchor?;
    conflict_nav_target_index_in_direction(direction, targets, Some(anchor), filter)
        .or_else(|| sole_matching_anchor_index(targets, anchor, filter))
}

pub(in crate::view) fn previous_conflict_nav_target_index_or_sole_anchor(
    targets: &[ConflictNavTarget],
    anchor: Option<ConflictNavAnchor>,
    filter: ConflictNavTargetFilter,
) -> Option<usize> {
    conflict_nav_target_index_or_sole_anchor_in_direction(
        ConflictNavDirection::Previous,
        targets,
        anchor,
        filter,
    )
}

pub(in crate::view) fn next_conflict_nav_target_index_or_sole_anchor(
    targets: &[ConflictNavTarget],
    anchor: Option<ConflictNavAnchor>,
    filter: ConflictNavTargetFilter,
) -> Option<usize> {
    conflict_nav_target_index_or_sole_anchor_in_direction(
        ConflictNavDirection::Next,
        targets,
        anchor,
        filter,
    )
}
```

- [ ] **Step 3: 验证等价**

Run: `cargo test -p worktree-ui-gpui conflict 2>&1 | tail -2`
Expected: 全绿，通过数 == C。
Run: `cargo clippy -p worktree-ui-gpui -- -D warnings 2>&1 | tail -3`
Expected: 零警告。

- [ ] **Step 4: 提交**

```bash
git add crates/worktree-ui-gpui/src/view/conflict_resolver.rs
git commit -m "Collapse conflict nav direction pairs onto shared cores"
```

---

### Task 5: tree_sitter_grammar 注册表宏化

**Files:**
- Modify: `crates/worktree-ui-gpui/src/view/rows/diff_text/syntax/language.rs:246-513`

**Interfaces:**
- Consumes: `DiffSyntaxLanguage`（`syntax.rs:208`，已 derive `PartialEq` ✓）、`TreesitterQueryAsset::{highlights, with_injections}`、各语言 crate 的 `LANGUAGE*` 常量与查询常量（均已在文件内使用）。
- Produces: `tree_sitter_grammar` 签名与语义不变（`pub(super) fn(DiffSyntaxLanguage) -> Option<(tree_sitter::Language, TreesitterQueryAsset)>`）；惰性加载保持（`.into()` 仅在命中臂执行，宏不改变这一点）。

> **设计修正（执行期裁决）**：宏不能展开成 match 臂（rustc 直接拒绝，报 "macros cannot expand to match arms"，已用独立最小用例编译验证）。因此臂的 `DiffSyntaxLanguage::X =>` 模式保持显式，宏只展开为臂体里的**元组表达式**——`arm!` 调用出现在 `Some(...)` 的实参位置（表达式位置，最普通的宏用法）。

转换规则（对现有 match 的每个臂 1:1 适用，臂的完整清单即 language.rs:249-511 现存内容，顺序保持、注释原位保留）：

- `DiffSyntaxLanguage::X => Some(($ts.into(), TreesitterQueryAsset::highlights($HL)))`
  → `DiffSyntaxLanguage::X => Some(arm!($ts, $HL)),`
- `DiffSyntaxLanguage::X => Some(($ts.into(), TreesitterQueryAsset::with_injections($HL, $INJ)))`
  → `DiffSyntaxLanguage::X => Some(arm!($ts, $HL, $INJ)),`
- 尾臂 `_ => None` 保持原样（连同 509-510 的注释）。

- [ ] **Step 1: 捕获基线**

Run: `cargo test -p worktree-ui-gpui rows::diff_text::syntax 2>&1 | tail -2`
Expected: 全绿（约 297 个测试），记录通过数 S。

- [ ] **Step 2: 在函数体内定义臂宏并逐臂转换**

`language.rs` 中 `tree_sitter_grammar` 函数体替换为如下骨架，然后按上述规则把 249-511 的每个现存臂转成单行臂（示例给出了前 4 个臂的转换结果；其余依规则类推，Elixir 臂上方的注释块原位保留）：
```rust
pub(super) fn tree_sitter_grammar(
    language: DiffSyntaxLanguage,
) -> Option<(tree_sitter::Language, TreesitterQueryAsset)> {
    macro_rules! arm {
        ($ts:expr, $hl:expr) => {
            ($ts.into(), TreesitterQueryAsset::highlights($hl))
        };
        ($ts:expr, $hl:expr, $inj:expr) => {
            ($ts.into(), TreesitterQueryAsset::with_injections($hl, $inj))
        };
    }

    match language {
        DiffSyntaxLanguage::Markdown => Some(arm!(tree_sitter_md::LANGUAGE, MARKDOWN_HIGHLIGHTS_QUERY, MARKDOWN_INJECTIONS_QUERY)),
        DiffSyntaxLanguage::MarkdownInline => Some(arm!(tree_sitter_md::INLINE_LANGUAGE, MARKDOWN_INLINE_HIGHLIGHTS_QUERY)),
        DiffSyntaxLanguage::Html => Some(arm!(tree_sitter_html::LANGUAGE, HTML_HIGHLIGHTS_QUERY, HTML_INJECTIONS_QUERY)),
        DiffSyntaxLanguage::Jinja => Some(arm!(tree_sitter_jinja_dialects::LANGUAGE, JINJA_HIGHLIGHTS_QUERY, JINJA_INJECTIONS_QUERY)),
        // … 按同一规则转换其余每个现存臂 …
        // Languages without a wired tree-sitter grammar, or grammars gated off
        // by the current feature set, fall back to heuristic-only highlighting.
        _ => None,
    }
}
```

- [ ] **Step 3: 格式化并验证**

Run: `cargo fmt -p worktree-ui-gpui && git status --short crates/worktree-ui-gpui/src/view/rows/diff_text/syntax/language.rs`
Expected: 仅本文件被格式化（若 fmt 波及无关文件，改为 `rustfmt --edition 2024 crates/worktree-ui-gpui/src/view/rows/diff_text/syntax/language.rs` 单文件格式化并检查 `git status`）。
Run: `cargo test -p worktree-ui-gpui rows::diff_text::syntax 2>&1 | tail -2`
Expected: 全绿，通过数 == S。
Run: `cargo clippy -p worktree-ui-gpui -- -D warnings 2>&1 | tail -3`
Expected: 零警告。

- [ ] **Step 4: 提交**

```bash
git add crates/worktree-ui-gpui/src/view/rows/diff_text/syntax/language.rs
git commit -m "Table-drive the tree-sitter grammar lookup"
```

---

### Task 6: rows/history.rs 拆出 markdown 行渲染器

**Files:**
- Create: `crates/worktree-ui-gpui/src/view/rows/markdown_preview.rs`（承接 `rows/history.rs:667-2888` 的全部内容）
- Modify: `crates/worktree-ui-gpui/src/view/rows/history.rs`（删除 667-2888）
- Modify: `crates/worktree-ui-gpui/src/view/rows/mod.rs`（新增 `mod markdown_preview;`，与既有 `mod history;`（:403）并排）
- Modify: `crates/worktree-ui-gpui/src/view/rows/markdown_document.rs:17-22`（`use super::history::{…8 个常量…}` → `use super::markdown_preview::{…同样 8 个…}`）

**Interfaces:**
- Consumes: 待搬区域是自洽的 markdown 渲染族（常量 667-682、`markdown_preview_scaled_*` 等自由函数、`MarkdownPreviewRowTypography` 等类型、3 个 Element impl（821/887/2025 一带）、若干 `markdown_preview_*` 渲染函数）。
- Produces: `rows::markdown_preview` 模块。可见性语义：`pub(super)` 项在两个位置都等价于"rows 子树可见"（两文件同为 rows 的子模块）✓；`pub(in crate::view)` 深度无关 ✓。唯一外部消费方是 `markdown_document.rs:17` 的 8 常量 import（已 grep 确认无其它 qualified 引用）。

- [ ] **Step 1: 捕获基线**

Run: `cargo test -p worktree-ui-gpui rows 2>&1 | tail -2`
Expected: 全绿，记录通过数 R。
Run: `wc -l crates/worktree-ui-gpui/src/view/rows/history.rs`
Expected: 5134。

- [ ] **Step 2: 机械搬移**

1. 新建 `rows/markdown_preview.rs`，把 `rows/history.rs` 667-2888 行（从 `const MARKDOWN_PREVIEW_ROW_HEIGHT_PX` 起到 2888 行的 `}` 止，即 `impl HistoryView`（2890 起）之前的整段）原样剪切进去，文件头加一行模块注释：`//! Uniform-row markdown renderer for the diff/list preview surfaces.`
2. `rows/history.rs` 删除该区间。
3. `rows/mod.rs` 在 `mod history;`（:403）旁新增一行 `mod markdown_preview;`（按现有字母序插入）。
4. `rows/markdown_document.rs:17` 的 `use super::history::{` 改为 `use super::markdown_preview::{`（导入项不变）。

- [ ] **Step 3: 修编译面（机械）**

Run: `cargo check -p worktree-ui-gpui 2>&1 | grep -E "^error" | sort | uniq -c | head`
Expected 仅为两类可机械修复的错误：
1. `markdown_preview.rs` 缺导入 → 把 `rows/history.rs` 头部的 `use` 块复制过来，再按警告删除未用项；
2. E0624（private）→ 若**留在 history.rs 的代码**（`impl MainPaneView` 80-665、`impl HistoryView` 2890 起、测试）调用了搬走的私有项，把被调用项改成 `pub(super)`；反之若搬走的代码调用了留在 history.rs 的私有项，同样按需提 `pub(super)`（两文件同属 rows，语义不变）。
若出现其它类别错误，停下来核对搬移区间是否完整（边界：667 行常量起始 / 2888 行 `}` 结束），不要改动任何逻辑。

- [ ] **Step 4: 验证等价**

Run: `cargo test -p worktree-ui-gpui rows 2>&1 | tail -2`
Expected: 全绿，通过数 == R。
Run: `cargo clippy -p worktree-ui-gpui -- -D warnings 2>&1 | tail -3`
Expected: 零警告。
Run: `wc -l crates/worktree-ui-gpui/src/view/rows/history.rs crates/worktree-ui-gpui/src/view/rows/markdown_preview.rs`
Expected: history.rs ≈ 2,912（5134 − 2222），markdown_preview.rs ≈ 2,223。

- [ ] **Step 5: 提交**

```bash
git add crates/worktree-ui-gpui/src/view/rows
git commit -m "Move the markdown row renderer out of rows/history"
```

---

### Task 7: rows/sidebar.rs 拆出 DetailsPaneView 渲染

**Files:**
- Create: `crates/worktree-ui-gpui/src/view/rows/sidebar/details.rs`（承接 `rows/sidebar.rs:2646-3123` 的 `impl DetailsPaneView` 块）
- Modify: `crates/worktree-ui-gpui/src/view/rows/sidebar.rs`（删除 2646-3123；文件头部 imports 之后新增 `mod details;`）

**Interfaces:**
- Consumes: `DetailsPaneView`（定义在 `panes/details.rs:54`）、`rows/sidebar.rs` 内的既有项（经 `use super::*` 可见——`details.rs` 作为 sidebar 的子模块，其 `use super::*` 指向 sidebar 模块）。
- Produces: `impl DetailsPaneView` 方法集不变。**可见性必须换算**（模块深度 +1）：块内 `pub(in super::super)` → `pub(in crate::view)`；块内 `pub(super)` → `pub(in crate::view::rows)`。调用方已核实无需改动：`panels/layout.rs:1790/1944` 经 `Self::render_commit_file_rows` 方法解析；`panels/tests/file_status.rs:3201` 经完整路径 `crate::view::panes::DetailsPaneView::render_commit_file_rows`。

- [ ] **Step 1: 捕获基线**

Run: `cargo test -p worktree-ui-gpui rows::sidebar 2>&1 | tail -2`
Expected: 全绿，记录通过数 D。
Run: `cargo test -p worktree-ui-gpui file_status 2>&1 | tail -2`
Expected: 全绿（该文件走 `render_commit_file_rows` 路径）。

- [ ] **Step 2: 机械搬移**

1. 新建 `rows/sidebar/details.rs`：首行 `use super::*;`，然后原样剪切 `rows/sidebar.rs` 2646-3123（`impl DetailsPaneView {` 起至 3123 行的 `}` 止，即 `#[cfg(test)] mod tests`（3125 起）之前）。
2. `rows/sidebar.rs` 删除该区间，并在头部 `use` 块之后加一行 `mod details;`。
3. 在 `details.rs` 内做可见性换算（全局替换两轮）：`pub(in super::super)` → `pub(in crate::view)`；`pub(super)` → `pub(in crate::view::rows)`。

- [ ] **Step 3: 修编译面（机械）**

Run: `cargo check -p worktree-ui-gpui 2>&1 | grep -E "^error" | sort | uniq -c | head`
Expected：缺导入 → 从 `rows/sidebar.rs` 头部复制 `use` 块再按警告删减；E0624 → 留在 sidebar.rs 的代码调用了搬走的方法/项时按换算表提升可见性。出现其它错误先核对区间边界（2646 `impl DetailsPaneView {` / 3123 `}`），不改逻辑。

- [ ] **Step 4: 验证等价**

Run: `cargo test -p worktree-ui-gpui rows::sidebar 2>&1 | tail -2 && cargo test -p worktree-ui-gpui file_status 2>&1 | tail -2`
Expected: 全绿，通过数 == D（第二个命令全绿）。
Run: `cargo clippy -p worktree-ui-gpui -- -D warnings 2>&1 | tail -3`
Expected: 零警告。

- [ ] **Step 5: 提交**

```bash
git add crates/worktree-ui-gpui/src/view/rows
git commit -m "Move the sidebar details row rendering into its own module"
```

---

### Task 8: syntax.rs 内联测试外迁（9,450 行 → tests/ 目录）

**Files:**
- Create: `crates/worktree-ui-gpui/src/view/rows/diff_text/syntax/tests/mod.rs`
- Create: `crates/worktree-ui-gpui/src/view/rows/diff_text/syntax/tests/{prepared.rs, vendored.rs, languages.rs, heuristic.rs, injections.rs, engine.rs}`
- Modify: `crates/worktree-ui-gpui/src/view/rows/diff_text/syntax.rs:448-9899`（`#[cfg(test)] mod tests { … }` 整块替换为 `#[cfg(test)]\nmod tests;`）

**Interfaces:**
- Consumes: `syntax.rs` 根模块符号（子文件经两层 glob 可见：`tests/mod.rs` 首行 `use super::*;`，各子文件首行 `use super::*;`——子文件的 super 是 tests 模块，glob 会带入 tests/mod.rs 的私有导入与共享 helper，与 `conflict_resolver/tests/` 既有模式一致）。
- Produces: 测试路径从 `rows::diff_text::syntax::tests::x` 变为 `rows::diff_text::syntax::tests::<子模块>::x`；**测试数量与结果必须不变**。

分桶规则（按测试函数名第一个 `_` 前的分段，实测分布：prepared 43+prepare 9、vendored 26、heuristic 17、语言名合计约 110、其余机制类）：

| 子模块文件 | 收纳（函数名前缀） |
|---|---|
| `prepared.rs` | `prepared*`、`prepare*`、`batch*`、`streamed*`、`large*`、`small*`、`dense*`、`single*`、`pathological*`、`oversized*`、`timed*`、`perf*`、`warm*`、`cold*`、`background*`、`incremental*`、`wait*`、`visual*`、`unchanged*`、`document*`、`shared*`、`cached*`、`reset*`、`clipping*`、`subtracting*`、`non_*`、`extra*`、`extended*`、`every*`、`collected*`、`all_*`、`merge*`、`recent*`、`repo*` |
| `vendored.rs` | `vendored*` |
| `heuristic.rs` | `heuristic*` |
| `languages.rs` | 语言名前缀：`vue*`、`rust*`、`nix*`、`javascript*`、`js*`、`typescript*`、`ts*`、`tsx*`、`svelte*`、`yaml*`、`jinja*`、`xml*`、`markdown*`、`go*`、`assembly*`、`solidity*`、`fsharp*`、`clojure*`、`ocaml*`、`sql*`、`ruby*`、`lua*`、`julia*`、`json*`、`html*`、`haskell*`、`css*`、`cpp*`、`c*`、`gitcommit*`、`shell*` |
| `injections.rs` | `combined*`、`injection*`、`injected*`、`fenced*`、`markup*` |
| `engine.rs` | 其余全部（`parser*`、`treesitter*`、`grammar*`、`query*`、`capture*`、`normalize*`、`highlight*`、`lock*`、`token*`、`text*`、`assert*`、`has_*`、`with_*`、`parse*`、`query*`、以及任何未列入上述桶者） |

- [ ] **Step 1: 捕获基线**

Run: `cargo test -p worktree-ui-gpui rows::diff_text::syntax 2>&1 | tail -2`
Expected: 全绿，记录通过数 S8（约 297）。
Run: `cargo test -p worktree-ui-gpui rows::diff_text::syntax -- --list 2>/dev/null | grep -c ": test$"`
Expected: 数字 == S8（留作对照）。
Run: `awk 'NR>449 && /^    (fn|async fn) /' crates/worktree-ui-gpui/src/view/rows/diff_text/syntax.rs | sed -E 's/^    (async )?fn ([a-z0-9]+).*/\2/' | sort`
Expected: 完整测试/helper 函数名清单（用于按上表分桶；helper 函数名若非测试，保留在 tests/mod.rs）。

- [ ] **Step 2: 整体搬移（先不改分桶）**

1. 新建 `syntax/tests/mod.rs`：把 `syntax.rs` 449-9899 的 `mod tests { … }` **函数体**原样置入（即去掉 `mod tests {` 与结尾 `}` 两层，内容缩进减一级），首部保持 `use super::*;` 与 `use std::time::{Duration, Instant};`。
2. `syntax.rs` 把 448-9899 替换为：
```rust
#[cfg(test)]
mod tests;
```
3. Run: `cargo test -p worktree-ui-gpui rows::diff_text::syntax 2>&1 | tail -2`
Expected: 全绿，通过数 == S8（若个别测试因路径变化导致 `#[path]`/字符串断言失败，修测试内的字符串，不改产品代码）。

- [ ] **Step 3: 按桶拆分**

1. 在 `tests/mod.rs` 中：共享设施原位保留——`GLOBAL_COUNTER_TEST_LOCK`、`lock_global_counter_tests`、`assert_token_ranges_are_utf8_safe`、`has_token_kind_and_text` 及其余被多个桶使用的 helper/类型。
2. 按分桶表把测试函数剪切进 6 个子文件，每个子文件首行 `use super::*;`；`tests/mod.rs` 追加：
```rust
mod engine;
mod heuristic;
mod injections;
mod languages;
mod prepared;
mod vendored;
```
3. 仅被单一桶使用的 helper 随该桶迁移；被测试内 `#[cfg(test)] use …` 引用的局部项跟随其使用方。

- [ ] **Step 4: 验证等价**

Run: `cargo test -p worktree-ui-gpui rows::diff_text::syntax 2>&1 | tail -2`
Expected: 全绿，通过数 == S8。
Run: `cargo test -p worktree-ui-gpui rows::diff_text::syntax -- --list 2>/dev/null | grep -c ": test$"`
Expected: 与 Step 1 的对照数字一致。
Run: `cargo clippy -p worktree-ui-gpui -- -D warnings 2>&1 | tail -3`
Expected: 零警告。
Run: `wc -l crates/worktree-ui-gpui/src/view/rows/diff_text/syntax.rs`
Expected: ≈ 450。

- [ ] **Step 5: 提交**

```bash
git add crates/worktree-ui-gpui/src/view/rows/diff_text
git commit -m "Move syntax tests into syntax/tests"
```

---

### Task 9: 阶段收尾——完整 CI 验证

**Files:** 无代码改动。

- [ ] **Step 1: 完整三件套**

```bash
cargo clippy --workspace --no-default-features --features gix -- -D warnings
cargo test --workspace --no-default-features --features gix
cargo build -p worktree --features ui-gpui,gix
```
Expected: 三条全部通过。

- [ ] **Step 2: benchmark 编译检查**

Run: `cargo test --workspace --features benchmarks --no-run 2>&1 | tail -2`
Expected: `Finished`（Task 6/8 搬移了 rows 层符号，`rows/benchmarks/` 经 feature gate 引用它们，必须确认未被破坏）。

- [ ] **Step 3: 度量记录（写进 PR 描述）**

Run: `find crates -name '*.rs' -not -path '*/target/*' -exec wc -l {} + | sort -rn | head -12`
Expected: `rows/diff_text/syntax.rs` ≈ 450（原 9,899）；`rows/history.rs` ≈ 2,912（原 5,134）；`rows/sidebar.rs` ≈ 4,540（原 5,020，其中 ~480 行移入 sidebar/details.rs）。

- [ ] **Step 4: 推送/开 PR**

按仓库流程推送 dev 或按执行者工作流开 PR；PR 描述引用 spec 路径与本计划路径。

---

## Self-Review 记录

- **Spec 覆盖**：spec §5 P0 七行动作 → Task 1（空壳）、2（whitespace 拷贝）、3+4（镜像参数化，含偏差说明）、5（grammar 表驱动）、6（history markdown）、7（sidebar details）、8（syntax 测试外迁）。P0 行"conflict_jump 六件套"按偏差说明改由 Task 4 承接。全覆盖。
- **占位符**：Task 5 的"其余臂按规则类推"指向文件内现存完整清单（language.rs:249-511），非 TBD；Task 8 的分桶表覆盖全部前缀（兜底桶 engine.rs）。无其它占位符。
- **类型一致性**：Task 3 保留 `diff_jump_prev/next` 原签名；Task 4 四个 `pub(in crate::view)` 函数签名逐字保留；Task 6/7 的可见性换算表已按模块深度推导；Task 2 的 `pub(super)` 从 `rows/diff_text.rs` 发出 = rows 子树可见，与消费方 `rows/conflict_resolver.rs` 匹配。
