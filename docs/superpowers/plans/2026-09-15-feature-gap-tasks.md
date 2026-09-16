
---

## T-C 实施（2026-09-15 已落地命令面板入口，#18 半）

目录 diff 的 UI 入口第一步：命令面板 `compare-directory`。复用已落地的 T-B 管线（`Msg::RequestDirectoryDiff` → `Effect::LoadDirectoryDiff` → `schedule_load_directory_diff`）。

落点文件：
- `crates/worktree-ui-gpui/src/view/command_palette.rs`：COMMANDS 表新增 `compare-directory`（category `palette.cat.history`，keywords 便于检索）；同步加入 `every_command_has_a_registered_handler` 测试期望清单（该测试对表/清单强一致，缺臂会 fail）。
- `crates/worktree-ui-gpui/src/view/mod.rs`：`execute_command` 新增 `"compare-directory"` 臂，复用 `active_diff_target(&self.state)`：
  - 命中 `DiffTarget::CommitRange { from, to, path }` → 原样转发 `Msg::RequestDirectoryDiff`（path 即目录根，空=仓库根），并弹 Success toast「正在对比目录变更…」。
  - 否则（WorkingTree / Commit / 无活动 diff）→ Error toast「对比目录需在提交区间对比视图中使用」。
- `crates/worktree-ui-gpui/locales/palette.en.yml` / `palette.zh-CN.yml`：新增 `compare-directory` / `compare-directory-started` / `compare-directory-needs-range` 三键（中英文）。

设计要点 / 局限：
- 当前只支持「提交区间（CommitRange）对比视图」内发起；工作区（WorkingTree）/ 单提交对比暂不支持，因 T-B 后端 `schedule_load_directory_diff` 仅处理 `CommitRange`（与计划 D-G2 一致：base/target 默认取选中两 commit）。
- 范围（path）直接沿用活动 diff 的 path；sidebar 右键指定具体文件夹的入口待补（T-C 下半）。
- 验证：`cargo check -p worktree-ui-gpui` 通过（仅预存 user_survey 死代码 warning）。`cargo test` 因 tree-sitter 原生语法构建在本机报 C1056/C1056 os error 5 时间戳权限错无法运行（环境问题，非本改动所致）；handler 测试为结构性断言，COMMANDS 表与期望清单已同步。

待办（T-C 下半 / T-D / T-E）：
- T-C 下半：sidebar 右键文件夹 → 弹 `compare-directory`（带具体 path）。
- T-D（#19）：`DirectoryDiffDetails` 渲染 `DirectoryDiffResult` 树（逐文件展开）。
- T-E（#20）：目录增删行数 / 文件数统计条。
- 三项均依赖 P5 收口后的 UI crate 热文件改动。
it/hooks` UI；另 rerere、Gitea 为次档。

## 四项任务拆解总览

| 特性 | 归属 | 与 P5/Wave2 冲突 | TaskCreate |
|---|---|---|---|
| 目录级对比 diff | **迭代 06 Wave2（与 T6 同波次）** | 强协同、非冲突 | #16–#21（已建） |
| 堆叠分支 / Stacked-PR | 迭代 07 | 高 | 待立项 |
| Git Flow 图形化 | 迭代 08 / P5 收口后小迭代 | 中高 | 待立项 |
| 原生 `.git/hooks` UI | P5+Wave2 收口后 | 中 | 待立项 |

---

## 特性 1：目录级对比 diff（SmartGit Folder Comparison 式）— 进迭代 06

底层数据已齐：`CommitFileChange{path,additions,deletions}`（`worktree-core/src/domain.rs:154`）、`diff_range_files`（`worktree-git-gix/src/repo/log.rs:565`）、`FileBrowser`、`render_range_file_rows` 均存在。仅缺「按目录前缀聚合树 + 目录树 UI 入口 + 统计条」。

### TaskCreate 已建（#16–#21）

**#16 目录变更聚合模型 (T-A)** `S`
- 新增类型 `DiffEndpoint` / `DirectoryDiffRequest{repo_id,base,target,root}` / `DirectoryNode{name,path,kind,additions,deletions,file_count,children:Vec<DirectoryNode>}`（递归）/ `DirectoryDiffResult{root: DirectoryNode}`。
- 纯函数 `aggregate_to_tree(changes: &[CommitFileChange], root: &Path) -> DirectoryNode`、`filter_by_prefix(nodes, prefix) -> DirectoryNode`。
- 落点：`worktree-core/src/domain.rs` 或新 `worktree-state/src/diff_tree.rs`。
- 单测验证多层嵌套下 file_count / additions / deletions 聚合正确。
- **core 层可先于 P5 开工**。

**#17 后端取目录树 diff (T-B)** `M`
- 新增 `repo.diff_endpoint_tree_changes(base, target, root)`（`worktree-git-gix/src/repo/diff.rs` 或 `log.rs`）：gix `diff_tree_to_tree` + pathspec 前缀 `root/`（`build_unified_diff_command` 的 `path` 字段作 `-- path` 传入，天然支持目录前缀）。
- 新增 `Effect::LoadDirectoryDiff`（`worktree-state/src/msg/effect.rs`）+ reducer `directory_diff_loaded`；`model.rs` 加 `directory_diff: Loadable<Shared<DirectoryDiffResult>>` + rev。
- 复用 `COMMIT_STATS_MAX_FILES` 阈值（大目录截断保护）。
- **与 T6 协同**。

**#18 目录树 UI 入口 (T-C)** `M`
- `view/panels/sidebar.rs` 的 FileBrowser 目录节点（`FileEntry.kind == Directory`）右键菜单加 "Compare directory…"；`view/command_palette.rs` 加 `compare-directory` 命令（repo_id + 目录前缀 root）。
- 新增 `Msg::RequestDirectoryDiff { repo_id, base: Refish, target: Refish, root: PathBuf }`（`msg/message.rs`，紧邻 `RequestRangeDiff`）。
- 依赖 T-A（类型）、T-B（Effect+reducer）。

**#19 逐文件展开复用 (T-D)** `M`
- `view/panes/details.rs` 增 `DirectoryDiffDetails` 模式：按 `DirectoryNode.children` 递归渲染文件行（path + 增删计数）。
- 文件行点击 → 复用 `view/panes/main/diff_text.rs` 逐文件 unified diff（`render_range_file_rows` 现有逻辑），不新写文本 diff。
- 目录行点击 → 递归下钻一层。
- 大目录套用 T6 虚拟列表。
- 依赖 T-A、T-B、T-C。

**#20 目录级统计条 (T-E)** `S`
- 目录对比面板顶部统计条：当前目录（含递归）总 additions/deletions + 文件数 + 直接子目录数。
- 数据读 `RepoState.directory_diff`（T-A 聚合字段）；视觉复用 `FileBrowser` 统计聚合。
- 可先于 T-D 完成。

**#21 测试 + 大目录性能档位 (T-F)** `M`
- 单测：`aggregate_to_tree` / `filter_by_prefix` 聚合与剪枝正确。
- 大目录档位（> `COMMIT_STATS_MAX_FILES`）：验证 `diff_endpoint_tree_changes` 走 pathspec `root/` 不被全量截断；perf 挂到 T6 大 diff 虚拟化档位。
- 依赖 T-A、T-B；与 T6 强协同。

---

## 特性 2：堆叠分支 / Stacked-PR — 建议归迭代 07

当前零概念（无 parent/stack/chain）。底层 restack 用 gix rebase API。与 P5（`model.rs`/`branch_sidebar.rs`）和 Wave2（`git-gix/repo/`）**高冲突**，故排迭代 07（可复用 07 agent 工作台分支 / `RepoId` 管线）。

| # | 任务 | 文件/函数 | 工作量 |
|---|---|---|---|
| 1 | 数据模型 `StackMetadata{branches: Vec<StackBranch>}` / `StackBranch{name,parent: Option<BranchName>,order}` | `worktree-core/src/domain.rs`（新类型） | M |
| 2 | gix restack 编排 `repo.restack_stack(base_branch)`：`rebase --onto` 逐分支重放 | `worktree-git-gix/src/repo/`（新 `stack.rs`） | M |
| 3 | 侧栏渲染堆叠分支可视化（缩进 + 链线 + 顺序） | `view/panels/sidebar.rs` `BranchSidebarRow` | M |
| 4 | 命令面板 `stack-branch` / `restack` / `reorder` | `view/command_palette.rs` | S |
| 5 | `Msg::CreateStackedBranch{repo_id,name,parent}` + reducer | `msg/message.rs` / `model.rs` | S |
| 6 | PR 落点 `create_request_url` 已支持 `base_branch: Option<&str>`（`forge_request.rs:75`）→ 堆叠建 PR 直连 | 复用，无需新写 | S |
| 7 | 持久化（StackMetadata 落盘 UiSettings/session）+ restack 顺序单测 | `session.rs` / `worktree-git-gix` 单测 | M |

---

## 特性 3：Git Flow 图形化 — 建议归迭代 08 / P5 收口后小迭代

底层建/合/打 tag 的 Msg 与 gix 全已有，只缺「编排 + 角色徽标 + 命令面板 + 可配置前缀」。与 P5/Wave2 **中高冲突**。

| # | 任务 | 文件/函数 | 工作量 |
|---|---|---|---|
| A | 配置模型：Git Flow 前缀可配置（feature/release/hotfix…） | `UiSettings`（`session.rs:799`） | S |
| B | 分支编排 feature/release/hotfix 的 start/finish，复用 `Msg::CreateBranch`/`MergeRef`/`CreateTag`（`message.rs:561/769/903`） | 新 `worktree-git-gix/src/flow.rs` | M |
| C | 角色徽标：branch_sidebar.rs 分支行按前缀显示 feature/release/... 徽标 | `view/panels/sidebar.rs` | S |
| D | 命令面板 `gitflow: feature start` / `release finish` 等 | `view/command_palette.rs` | S |
| E | 图形化视图：基于现有 git graph 叠加 Git Flow 角色着色/泳道 | `view/panes/main/` graph 模块 | M |
| F | 测试：各 finish 全链路（建+合+打 tag）gix 单测 | `worktree-git-gix` 单测 | M |

---

## 特性 4：原生 `.git/hooks` UI — 建议归 P5+Wave2 收口后

复用 `Msg::OpenFileEditor{repo_id,PathBuf}`（`message.rs:448`）+ `lfs.rs` 的 `common_dir().join("hooks")` 定位范式。**命名须用 `GitHook*` / `RepoHook*`** 避开迭代 08 agent 钩子（`Msg` 已有 agent 钩子语义）的语义坍塌。与 P5/Wave2 **中冲突**。

| # | 任务 | 文件/函数 | 工作量 |
|---|---|---|---|
| T1 | 钩子清单模型 `GitHook`（枚举 pre-commit/pre-push/...）/ `RepoHookList{enabled,defined}` | `worktree-core/src/domain.rs`（新类型，命名避 `GitHook*` 冲突） | S |
| T2 | 钩子定位 `repo.list_hooks()`：`common_dir().join("hooks")`（`lfs.rs:16` 范式） | `worktree-git-gix/src/repo/hooks.rs` | S |
| T3 | `Effect::LoadRepoHooks` + reducer `repo_hooks_loaded` + model `repo_hooks: Loadable<...>` | `msg/effect.rs` / `model.rs` | S |
| T4 | 设置页 hooks 面板（列表 + 启用开关 + 编辑）复用 `Msg::OpenFileEditor` 打开脚本 | `view/panes/settings/` 或 `details.rs` | M |
| T5 | 启用/禁用：钩子文件 +x 位切换 `repo.set_hook_enabled(name,bool)` | `worktree-git-gix/src/repo/hooks.rs` | S |
| T6 | 新建/模板：从 `.sample` 复制或建骨架脚本 | `worktree-git-gix/src/repo/hooks.rs` | S |
| T7 | 命令面板 `edit-hook` / `toggle-hook` | `view/command_palette.rs` | S |
| T8 | 测试：钩子清单/启用状态单测（mock common_dir） | `worktree-git-gix` 单测 | M |

---

## 并行约束（与 P5 的边界）

- 目录 diff 的 **T-A（core 类型）/ T-F 单测** 几乎不碰 `worktree-ui-gpui/src` → **可先于 P5**。
- 目录 diff 的 **T-B（Effect+reducer）/ T-C/T-D/T-E（UI）** 碰 `view/`、`git-gix/repo/` → **与 T6 同波次，等 P5 收口或文件级避让**。
- 堆叠分支 / Git Flow / 原生 hooks 三项整体撞 P5+Wave2 热文件 → **P5 收口后立项**。

## 验证（沿用迭代 06 四腿基线）

| 腿 | 命令 / 判据 |
|---|---|
| a | `cargo test --workspace --no-default-features --features gix` |
| b | `cargo test --workspace` |
| c | live clippy vs `clippy-baseline.txt` 逐字节 diff（零 diff） |
| d | `cargo test -p worktree-ui-gpui -- --list` 名单比对（零测试丢失） |

目录 diff 额外：T-A / T-F 纯函数单测 0 failed；大目录档位耗时可测、UI 走虚拟列表不卡。

## 决策点

| # | 决策 | 触发时机 |
|---|---|---|
| **D-G1** | 堆叠分支 / Git Flow / 原生 hooks 三项是否正式立项（当前仅目录 diff 建任务） | 本计划评审后 |
| **D-G2** | 目录 diff 的 base/target 默认取什么（工作区 vs 选中两 commit vs 分支） | T-C 开工前 |
| **D-G3** | 原生 hooks 命名最终定 `GitHook*` 还是 `RepoHook*`（避迭代 08 agent 钩子） | T1 开工前 |

## 风险与缓解

| 风险 | 缓解 |
|---|---|
| P5 与 T-B/T-C/T-D 同改 UI crate 冲突 | core 层（T-A/T-F）先行；UI 层等 P5 收口或文件级避让 |
| 大目录 diff 性能 | 复用 `COMMIT_STATS_MAX_FILES` 阈值 + T6 虚拟化；T-F 补档位 |
| 原生 hooks 与迭代 08 agent 钩子语义混淆 | 强制 `GitHook*`/`RepoHook*` 命名前缀，独立枚举臂 |
| 堆叠/Git Flow 撞 P5 模型拆分 | 排 P5 收口后；若提前则文件级避让 `model.rs` |

---

## T-B 实施更正（2026-09-15 已落地 #17）

计划文档中 T-B 相关符号经代码核对**与真实代码不符**，已按真实 API 实现，后续 T-C/T-D/T-E 请直接用下列真实符号：

- ~~`Msg::RequestRangeDiff { repo_id, base: Refish, target: Refish, root: PathBuf }`~~ —— `Refish` 类型**不存在**。
- 真实请求：`Msg::RequestDirectoryDiff { repo_id: RepoId, target: DiffTarget }`，其中 `target` 用 `DiffTarget::CommitRange { from_commit_id, to_commit_id: Option<CommitId>, path: Option<PathBuf> }`，`path` 即目录根（空 = 仓库根）。
- 真实管线（worktree-state 层，未新增 git-gix trait 方法）：
  - `Effect::LoadDirectoryDiff { repo_id, target }`
  - `InternalMsg::DirectoryDiffLoaded { repo_id, target, result: Result<Arc<DirectoryDiffResult>, String> }`（载体类型来自 `worktree_core::diff_tree`）
  - `DiffState` 落点字段：`directory_diff_target: Option<DiffTarget>` + `directory_diff: Loadable<Shared<DirectoryDiffResult>>`
  - `store/effects/repo_load.rs::schedule_load_directory_diff`：`spawn_with_repo` 闭包内调既有 `repo.diff_range_files(from, to)` → `DirectoryDiffResult::new(&changes, &root)`
  - reducer：`store/reducer/diff_selection.rs` 的 `RequestDirectoryDiff` 臂 + `directory_diff_loaded`（mirror `diff_loaded`）
- 理由：`GitRepositoryDiff` trait 有数十个实现者，加 trait 方法会逼改所有 mock；在 state 层复用既有 `diff_range_files` + core 聚合零新 backend 接口，规避 P5 冻结风险。
- 验证：`cargo check -p worktree-state -p worktree-ui-gpui -p worktree` 通过；`cargo test -p worktree-core --lib diff_tree` 17 passed。改动本地未提交。
