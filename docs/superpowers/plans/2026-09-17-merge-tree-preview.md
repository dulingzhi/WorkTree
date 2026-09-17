# merge-tree 预演合并（Merge Preview）

状态：**已完成（2026-09-17）**。生态缺口 S 档第 2 项（fixup/autosquash 之后），差异化强、开源侧稀缺、低风险（shell out 到 git，不碰 gix 不成熟 merge / 不碰 P5 冻结的 trait）。落地详情见文末「落地说明」。

实测确认的命令语义（Git 2.53，`git merge-tree --write-tree <b1> <b2>` 把 b2 合并进 b1，写树不碰工作区/索引）：

- **干净合并**：stdout 仅打一行结果 tree OID（40-hex），exit 0。
- **有冲突**：首行仍是结果 tree OID（冲突文件带 conflict markers 的内容已写入该树），随后逐文件 `CONFLICT (<type>): Merge conflict in <path>`，exit 1。实测样例：
  ```
  87f0a29b…            # 结果 tree
  100644 587be6b… 2	f.txt
  100644 975fbec… 3	f.txt
  Auto-merging f.txt
  CONFLICT (add/add): Merge conflict in f.txt
  ```

---

## §0 现状：差一个「只看不合并」的入口

- 真实合并能力**已有**：`Msg::MergeRef`（`message.rs:769` 一带）+ gix `merge` 后端（Tower/Git Flow finish 在用）。但那会改写工作区/索引/历史。
- **缺的是**：在动手前预览「把某提交 merge 进当前 HEAD 会得到什么」——冲突了没、改了哪些文件。SmartGit/Tower 的 "Merge Preview" 即此。GitComet 完全没有这个只读入口。
- gix 的 merge 仍属 initial development，**不**用 gix 跑——`git merge-tree --write-tree` 是 Git 2.38+ 官方给的「无副作用合并」原语，正好对应。

---

## §1 数据流（右键提交 X → 预览「merge X 进 HEAD」）

```
UI: 提交行右键 → "Preview merging <sha> into <branch>"
  → ContextMenuAction::OpenPopover { kind: PopoverKind::MergePreview { repo_id, other: CommitId } }
  → 打开弹窗时 dispatch Msg::PreviewMerge { repo_id, other }
  → reducer: head = current HEAD commit; head == other 则警告且不发请求
  → Effect::LoadMergePreview { repo_id, head, other }
  → git-gix: git merge-tree --write-tree <head> <other>   // 只读
      → 解析：result_tree(OID) + conflicts[path] + has_conflict(exit!=0)
      → 干净时再跑 git diff --numstat -z <head>^{tree} <result_tree> → files[]
  → InternalMsg::MergePreviewLoaded { repo_id, head, result }
  → reducer: 存 HistoryState::merge_preview（head 漂移则丢弃 + 警告）
```

- **预览只展示，不执行合并**：弹窗只有「关闭」，无「真的合并」按钮（真实合并走既有 `Msg::MergeRef`，不在本特性范围）。
- **冲突展示**：列出冲突文件 + 「该合并会产生 N 处冲突」警告；可选进一步渲染冲突内容（结果树里冲突文件已含 markers）。
- **干净展示**：结果树 vs HEAD 树的 diff —— 文件清单 + ± 计数（树级 diff，gix `diff_tree_to_tree`）；MVP 先给清单+计数，逐文件 unified diff 作为后续增强。

---

## §2 后端（`worktree-git-gix`）：shell out 到 git

新增（**不是 trait 方法**，是 `repo` 上的自由函数，零 P5 影响）：

```rust
pub struct MergeConflictFile { pub path: String, pub conflict_type: String }
pub struct MergeTreePreview {
    pub result_tree: String,         // 40-hex OID
    pub has_conflict: bool,
    pub conflicts: Vec<MergeConflictFile>,
}
pub fn merge_tree_preview(&self, head: &str, other: &str) -> Result<MergeTreePreview, Error>;
```

- 命令：`git merge-tree --write-tree <head> <other>`（2-arg 形式自动求 merge-base）。
- 解析：
  - 首行匹配 `^[0-9a-f]{40}$` → `result_tree`。
  - 含 `CONFLICT (…): Merge conflict in <path>` 的行 → 收集冲突（type 取自括号，path 取行尾）。
  - exit 0 → `has_conflict=false`；exit !=0 → `has_conflict=true`（但仍拿到 result_tree）。
- 错误处理：`git` 失败（如无共同历史 `fatal: …`、git 版本过旧不支持 `--write-tree`）→ 返回 `Error`，reducer 弹 Error 通知。
- 版本门槛：要求 Git ≥ 2.38；函数开头可探测，或在文档/help 注明。本机 2.53 通过。

---

## §3 对 P5 冻结的影响

**零影响**。不新增任何 `GitRepositoryDiff` trait 方法；新增的是 `worktree-git-gix` 里一个 shell-out 自由函数（`repo.merge_tree_preview`），不进入 trait。state/UI 层与 fixup 同构，无新 backend 接口。

---

## §4 UX 细节

- **菜单项**：commit 右键 history-rewrite 组（紧邻 `Merge into current`）。
  - label `cm.commit.merge_preview`（`%{short}` + `%{current}`）；icon `icons/git_merge.svg`。
  - `disabled`：与 `Merge into current` 同一判据（`commit_is_ancestor_of_head` —— 祖先是空操作，预览「已是最新」是噪音）。
  - `action: ContextMenuAction::OpenPopover { kind: PopoverKind::MergePreview { repo_id, other } }`（不新增 action variant；参数随 kind 走，同 `AutosquashConfirm`）。
- **弹窗 `PopoverKind::MergePreview { repo_id, other }`**：照 `AutosquashConfirm` / `SquashPrompt` 的 8 处注册模式（`host/kinds.rs` / `popover.rs` mod / `dispatch.rs` / `fingerprint.rs` 3 臂 / `geometry.rs` / `open.rs` dismiss+打开时 dispatch `Msg::PreviewMerge` / `dialog.rs`）。
- **面板 `merge_preview.rs`**：从 `history_state.merge_preview` 读 `Loadable`。
  - `Loading` → 加载中。
  - `NotLoaded` / `Error(_)` → 不可用提示（`Error` 打印 git 原文，如 unrelated histories）。
  - `Ready(preview)`：
    - `preview.has_conflict` → 红色冲突文件列表（每行 `path` + `conflict_type`）+ 计数。
    - 否则 →「无冲突」+ 变更文件清单（每行 `path  +A −D`，二进制打 `(binary)`）+ 计数；`files` 为空 → 「不会带来改动」。
  - 按钮：仅「关闭」（`Msg::CancelMergePreview { repo_id }` 清 preview + close）。不做真实合并按钮（留待既有 Merge 流程）。
- **i18n**：`context_menu_dynamic.en/zh-CN` 加 `commit.merge_preview`；`prompts.en/zh-CN` 加 `merge_preview:` 块（title / target / loading / empty / clean / no_changes / changed_count / file_row / file_row_binary / conflict_count / conflict_row）；`panels.en/zh-CN` 加 `merge_preview.close`。

---

## §5 测试计划

| 层 | 测什么 | 放哪 | 手法 |
|---|---|---|---|
| git-gix | `merge_tree_preview` 干净：返回 result_tree + has_conflict=false | `git-gix/tests/merge_tree_integration.rs`（新建，照 `squash_integration` 用真实 git） | plumbing 造 base+两分支不同改同文件 → 干净合并断言 result_tree 非空、conflicts 空 |
| git-gix | `merge_tree_preview` 冲突：返回 has_conflict=true + 冲突文件 | 同上 | plumbing 造 add/add 冲突（本次实测同构）→ 断言 conflicts 含该 path、has_conflict=true |
| git-gix | 无共同历史 → Err | 同上 | 两 orphan 提交 → 断言 Err |
| state | `Msg::PreviewMerge` → 发 `Effect::LoadMergePreview`；HEAD 祖先 → 不发/警告 | `store/tests/actions_emit_effects.rs` | 仿 `autosquash` 测试 |
| state | `MergePreviewLoaded` → 存 preview / HEAD 漂移放弃 | `loaded_results/tests.rs` | 仿 `autosquash_rebase_setup_loaded` 测试 |
| ui | 右键菜单项可用/不可用态 | `popover/context_menu/tests.rs` | 纯函数 |

**门禁**（CI billing 停摆，本地为准）：
`cargo fmt --check` + `cargo check -p worktree-state --tests` + `CARGO_TARGET_DIR=<C盘> cargo check -p worktree-ui-gpui --tests` + `cargo test -p worktree-git-gix --test merge_tree_integration` + `cargo test -p worktree-state --lib` + `cargo test -p worktree-core --lib`。

---

## §6 落地顺序

1. **后端**：`worktree-git-gix` 自由函数 `merge_tree_preview`（shell out + 解析）+ 集成测试（新建 `merge_tree_integration.rs`）。独立提交。
2. **core 类型**：`MergeTreePreview` / `MergeConflictFile`（已在 gix 定义，state 侧引用）。
3. **state**：`Msg::PreviewMerge` / `CancelMergePreview` + `InternalMsg::MergePreviewLoaded` + `Effect::LoadMergePreview` + `HistoryState::merge_preview` + reducer（照 autosquash 模板）+ 单测。
4. **UI**：`PopoverKind::MergePreview` 8 处注册 + `merge_preview.rs` 面板 + `ContextMenuAction::PreviewMerge` + 右键菜单项 + dispatcher + i18n + 菜单测试。
5. **文档**：本文补「落地说明」；`2026-09-15-feature-gap-tasks.md` 标注特性 6 完成。

---

## 落地说明（2026-09-17 完成）

**Step 1 后端** — `8511682e feat(git-gix): read-only merge preview via merge-tree`
- `GixRepo::merge_tree_preview(head, other)`：`git merge-tree --write-tree`（`run_git_raw_output` 容忍 exit≠0），首行 40-hex → `result_tree`，`CONFLICT (…)` 行 → `conflicts`；无 tree OID → `git_command_failed_error`（覆盖 unrelated histories，exit 128）。
- `crates/worktree-git-gix/tests/merge_tree_integration.rs` 新建，3 测试（干净 / 冲突 / 无共同历史）。
- `GitRepositoryHistory::merge_tree_preview` 以 **default method**（返回 `Unsupported`）加到 trait，gix 侧经 `delegate_git_repository!` 覆写 → **P5 冻结零影响**，未动 `GitRepositoryDiff`。

**Step 2–3 state** — 与 autosquash 同构
- `Msg::PreviewMerge { repo_id, other }` / `Msg::CancelMergePreview { repo_id }`；`InternalMsg::MergePreviewLoaded { repo_id, head, result }`（**去掉了设计里的 `other` 字段**：面板从 `PopoverKind` 已拿到 `other`，回传是冗余）。
- `HistoryState::merge_preview: Loadable<MergeTreePreview>` + `merge_preview_rev`。
- `preview_merge` reducer：`head == other` 或拿不到 HEAD → Warning「不可预览」，**不发 Effect**；否则置 `Loading` + 发 `Effect::LoadMergePreview`。
- `merge_preview_loaded`：head 漂移 → `NotLoaded` + Warning；`Ok` → `Ready`；`Err` → 诊断 toast + `NotLoaded`。

**Step 4 UI**
- `PopoverKind::MergePreview { repo_id, other }` 8 处注册齐备；`open.rs` 打开时 dispatch `Msg::PreviewMerge`（同 `AutosquashConfirm` 范式，每次打开按 live HEAD 重算）。
- `merge_preview.rs` 面板：**只有「关闭」**（回顾性只读工具，不提供真实合并按钮）。
- 菜单项插在 `Merge into current` **之前**，共用 `commit_is_ancestor_of_head` 判据；i18n 三处（`context_menu_dynamic` / `prompts` / `panels`）。
- **偏差**：设计里的 `ContextMenuAction::PreviewMerge` 未新增，改用既有的 `ContextMenuAction::OpenPopover { kind }`（参数随 kind 走，与 `AutosquashConfirm` 一致，少一个 variant）。

**补充（干净合并的变更清单）** — 设计 §4 要求干净时给出「改了哪些文件」
- `MergeTreePreview` 增 `files: Vec<MergeChangedFile>`（`path` + `additions`/`deletions`，二进制为 `None`）。
- 取数**复用 `submodules::git_range_numstat_counts`**（`pub(super)` 放开），跑 `git diff --numstat -z --find-renames <head>^{tree} <result_tree>`；**没有**走 gix `diff_tree_to_tree`——同一解析器已在用，避免第二套。
- **仅干净合并填充**：冲突时结果树里是 conflict markers，行数会描述 marker 而不是合并效果 → `files` 留空，冲突列表才是答案（集成测试钉死这一点）。

**顺带修的真 bug（构建级）**
- `crates/worktree-ui-gpui/src/view/panels/popover.rs` **缺 `mod autosquash_confirm;`**：`autosquash_confirm.rs` 在 `84ae999d` 已提交，但模块声明从未落盘（`84ae999d` 未触碰 `popover.rs`）。即 **HEAD 上 `worktree-ui-gpui` 编不过** —— fixup/autosquash 那次「UI 门禁通过」的结论不成立。本轮补上声明并首次真正编译通过。
- 教训：UI 层的 `check` 不能只看「改的文件有没有报错」，必须以 rc=0 为准。

**门禁（本轮实测，CI billing 停摆 → 本地为准）**

| 命令 | 结果 |
|---|---|
| `cargo fmt --check` | clean |
| `cargo check -p worktree-state --tests` | 无 error |
| `CARGO_TARGET_DIR=<C盘> cargo check -p worktree-ui-gpui --tests` | 无 error |
| `cargo test -p worktree-core --lib squash` | 53 passed |
| `cargo test -p worktree-state --lib` | **760 passed / 0 failed** |
| `cargo test -p worktree-git-gix --test merge_tree_integration` | 3 passed |

**未做（明说）**：UI 侧无法加菜单/面板自动化测试——`worktree-ui-gpui` 没有 `tests/`，popover 面板需要重量级 `PopoverHost` 脚手架，仓库内无先例。UI 的正确性靠**编译器**兜底（8 处注册点的 `match` 臂 + `PopoverKind` 新增变体会强制穷尽匹配），行为正确性由 state 层测试承担。技能/惯例：若后续要补 UI 测试，得先建 `PopoverHost` 测试脚手架，那是独立任务。
