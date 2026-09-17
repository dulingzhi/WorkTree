# merge-tree 预演合并（Merge Preview）

状态：**范围待确认（2026-09-17）**。生态缺口 S 档第 2 项（fixup/autosquash 之后），差异化强、开源侧稀缺、低风险（shell out 到 git，不碰 gix 不成熟 merge / 不碰 P5 冻结的 trait）。

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
UI: 提交行右键 → "Preview merge into HEAD" → ContextMenuAction::PreviewMerge { repo_id, other: CommitId }
  → Msg::PreviewMerge { repo_id, other }
  → reducer: head = current HEAD commit; 校验 other 不是 head 祖先（否则合并即快进/无变化，无意义）
  → Effect::LoadMergePreview { repo_id, head, other }
  → git-gix: git merge-tree --write-tree <head> <other>   // 只读
      → 解析：result_tree(OID) + conflicts[path] + has_conflict(exit!=0)
  → InternalMsg::MergePreviewLoaded { repo_id, other, result }
  → reducer: 存 HistoryState::merge_preview; 打开 PopoverKind::MergePreview
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

- **菜单项**：commit 右键 history-rewrite 组（紧邻 `Autosquash from here` / `SquashSelectedCommits`）。
  - label `cm.commit.merge_preview`（short）；icon 复用 `icons/git_merge.svg`（若已有）或 `git_commit.svg`。
  - `disabled`：current HEAD 为 detached 且无可合并目标时、或 `other` 是 HEAD 祖先时（合并即空操作）。
  - `action: ContextMenuAction::PreviewMerge { repo_id, other: commit_id }`。
- **弹窗 `PopoverKind::MergePreview { repo_id, other }`**：照 `AutosquashConfirm` / `SquashPrompt` 的 8 处注册模式（`host/kinds.rs` / `popover.rs` mod / `dispatch.rs` / `fingerprint.rs` 3 臂 / `geometry.rs` / `open.rs` dismiss+打开时 dispatch `Msg::PreviewMerge` / `dialog.rs`）。
- **面板 `merge_preview.rs`**：从 `history_state.merge_preview` 读 `Loadable`。
  - `Loading` → 加载中。
  - `NotLoaded` → 空提示。
  - `Ready(preview)`：
    - `preview.has_conflict` → 标题「合并将产生冲突」+ 红色警告 + 冲突文件列表（每行 `path` + `conflict_type`）。
    - 否则 → 标题「合并预览」+ 结果树 vs HEAD 树 diff 摘要（N files changed, +A −D，取自 gix `diff_tree_to_tree(head_tree, result_tree)` 聚合）+ 变更文件清单（path + ±）。
  - 按钮：仅「关闭」（`Msg::CancelMergePreview { repo_id }` 清 preview + close）。不做真实合并按钮（留待既有 Merge 流程）。
- **i18n**：`context_menu_dynamic.en/zh-CN` 加 `commit.merge_preview`；`prompts.en/zh-CN` 加 `merge_preview:` 块（title / conflict_title / conflict_line / changed_files / loading / empty）。

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

## 落地说明

（待确认范围后落地）
