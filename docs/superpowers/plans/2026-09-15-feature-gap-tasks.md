
---

## A 档补漏：接线缺口与测试加固（2026-09-17）

CI 因 GitHub 账号 billing 全 failure（详见当日 memory），本机 `cargo check` 是唯一门禁，因此优先清「能防止未来漏接线」的低风险项。

### 1. `fetch_prune_deleted_remote_tracking_branches` UI 接线

**缺口**：字段在 `RepoState`（`model.rs:1224`）已加载 / 持久化 / 真正参与 fetch 决策（`actions_emit_effects.rs:377`），`Msg::SetFetchPruneDeletedRemoteTrackingBranches` 与 reducer（`repo_management.rs:889`）齐备且有单测——**唯独没有任何 UI 能触发它**，等于一个只能手改 `session.json` 的设置。即上一轮探索报的"A 档①"。

**改动**（纯 UI 层，state 侧零改动）：
- `popover/host/state.rs`：`RepoSettingsState` 加 `repo_settings_fetch_prune: bool`。注释点明它与同组 `user.*` / `commit.gpgsign` 字段的**语义差别**——后者是本地 `git config` 覆盖（三态可继承），此字段是 WorkTree 自身偏好（持久化在 `session.json`），**只有开/关**，无"继承"态。
- `popover/host/impl_new.rs`：初值 `true`（与 `RepoState::new_opening` 的默认一致）。
- `popover/open.rs`：打开仓库设置时从 `state.repos[i].fetch_prune_deleted_remote_tracking_branches` 播种草稿（`is_none_or(...)` 兜"仓已消失"场景为 true，与初值同）。
- `popover/repo_settings.rs`：加一行 toggle（镜像既有 sign 行的 `debug_selector` 结构），并在 `submit_repo_settings` 里**在写入 plan 之外**dispatch——因为它不是 git-config key，不能进 `repo_settings_apply_plan`（那个 plan 的契约是"按 config key 收敛差异"）。仅当值与 `RepoState` 现值不同才 dispatch，reducer 自带 no-op 与 session 持久化。**关键**：dispatch 放在 `plan.is_empty()` 提前返回**之前**，否则"只改该开关、不动 config 字段"的提交会被早退吞掉。
- `locales/inputs.{en,zh-CN}.yml`：`fetch_prune_label` / `fetch_prune_on` / `fetch_prune_off`。

### 2. handler 测试加固（`every_command_has_a_registered_handler`）

**缺口**：原测试把一份**硬编码 id 清单**与 `COMMANDS` 比对——它只能发现"`COMMANDS` 变了"，**完全不能发现"`execute_command` 漏了分支"**（清单是人手抄的，不是从 match 派生的）。即"A 档③"的实质：`execute_command` 的 `_ => {}` 会静默吞掉未接线的 id，用户看到命令、能点、什么都不发生。

**改动**（`command_palette.rs`）：
- 新增 `pub(crate) const REGISTERED_COMMAND_HANDLERS: &[&str]`——手工维护的"已接线 id"单一事实源（Rust 无法反射 match 分支，只能手工，但手工表**放在源码里**就成了可对账的声明）。
- 删除原快照测试（其职责被新测试完全覆盖）。
- 新测试 `every_palette_command_has_a_registered_handler` 双向校验：`COMMANDS\handlers` 非空 → 死命令（真 bug）；`handlers\COMMANDS` 非豁免项非空 → 未声明的外部调用点。豁免表当前仅 `apply-patch`（工作区右键菜单 dispatch，非面板命令），注释说明"加进这张表 = 声明存在非面板调用点"。
- 新测试 `every_command_label_is_a_translation_key`：所有 `label` / `category` 必须以 `palette.` 开头。
- 新测试 `every_command_key_resolves_in_both_catalogs`：每个键在 en / zh-CN 目录都必须解析得到（`t!(key) != key`），把"漏翻译"从运行时静默回退变成测试失败。

### 3. `show-reflog` 补国际化（A 档②）

`COMMANDS` 里 `id: "show-reflog"` 的 `label` / `category` 是**字面英文**（`"Show Reflog"` / `"History"`），而同表其余 73 条全是 `palette.cmd.*` / `palette.cat.*` 键——中文构建下这一条会突兀地显示英文。改为 `palette.cmd.show-reflog` + `palette.cat.history`（后者已存在），补 en / zh-CN 两个 `show-reflog` 字符串。新增的 `every_command_label_is_a_translation_key` 测试即为此设的回归网。

### 核账结果（本次实测）

- `COMMANDS` 74 条 ⇔ `execute_command` 75 个已接线 id（多出的 `apply-patch` 已在豁免表声明）。原以为 `stash-drop` 漏接线，实为它与 `stash-pop`/`stash-apply`/`stash-branch` 共用 `|` 分支——用朴素正则逐行扫描会误判，**对账脚本必须支持或分支**。
- 除 `show-reflog` 外无其它未国际化的 label / category。

**验证**：
- `cargo check -p worktree-state --tests` 通过。
- `CARGO_TARGET_DIR=<C盘> cargo check -p worktree-ui-gpui --tests` 通过。
- `CARGO_TARGET_DIR=<C盘> cargo test -p worktree-ui-gpui --lib command_palette` → **22 passed, 0 failed**（含 3 个新测试）。
- `cargo fmt --check` 干净。

**新发现（值得记）**：`worktree-ui-gpui` 的**单元测试也能跑**（不只有 `check`）——只要用 C 盘 `CARGO_TARGET_DIR`，首次链接约 12 分钟，之后增量。此前 MEMORY 只记了"用 C 盘 target 跑 check"，实测 `cargo test` 同样可用，本机验证能力比预想强。
DiffDetails` 模式：按 `DirectoryNode.children` 递归渲染文件行（path + 增删计数）。
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

---

## T-C / T-D / T-E 完成（2026-09-16，目录 diff 端到端可用）

三个提交落地于 `dev` 分支（用户要求本地 commit，未 push；`.workbuddy/` 按约定不入库）：
- `30a0857a` T-C 下半：`ContextMenuAction::CompareDirectory{repo_id,path}` + `file_browser_folder.rs` 右键「Compare Directory…」+ `impl_menu.rs` 中央 dispatcher 臂（复用活动 `CommitRange` diff target + 右键目录 path）。
- `ba27f833` T-D：`DetailsPaneView::render` 在 `directory_diff_target` 置位时改渲染 `DirectoryDiffResult` 树（`render_directory_tree` 递归缩进 + 每节点 change kind / ± / 文件数；Ready/Loading/Error/NotLoaded 全处理）。
- `fab82b80` T-E：`render_directory_diff` 顶部统计条「N files changed, +A -D」（取 root `DirectoryNode` 聚合）。

验证：`cargo check -p worktree-ui-gpui` 通过（仅预存 user_survey warning）；`cargo fmt` 干净。

目录 diff 特性状态：T-A✓ T-B✓ T-C✓ T-D✓ T-E✓；**T-F 大目录性能档位半段 = 推迟**：其优化需在 git-gix 给 `GitRepositoryDiff` trait 加 pathspec `root/` 前缀裁剪（改动 trait 签名 → 逼改数十 mock，撞 P5 冻结），与 T-B 已定的零新 backend 接口决策相悖；当前 `diff_range_files` 已返回全量路径、聚合正确，大目录靠 T6 虚拟化兜底。

下一步（待用户指定）：堆叠分支 / Git Flow / 原生 hooks（均 P5+ 后，D-G1 决策）；或补 T-D 的目录展开/折叠、T-E 放置位置复核（details vs main pane）。

## T-C / T-D / T-E 收尾（2026-09-17）

核对发现上一节「下一步」已过时：T-D 的**折叠/展开**早已落地（commit `046734b0`：`[+]/[-]` + 点击切换 + `directory_diff_collapsed`）。但 **#19 原意的两种交互都没实现**——落地的是折叠/展开（不是"目录行点击→下钻一层"），文件行是死的；而且 core 的 `filter_by_prefix`（下钻 re-root 的现成帮手）**已实现并有单测，却全仓库无人调用**。T-E 统计条存在但是一行裸文本，缺 #20 要求的"直接子目录数"。

本次补齐 4 项：

- **文件行点击 → 打开该文件 diff**（#19 原意）：`details/mod.rs::render_directory_tree` 文件行改为 dispatch `Msg::SelectDiff { DiffTarget::CommitRange { from, to, path: file } }`（from/to 取自活动 `directory_diff_target`），主面板据此渲染逐文件 unified diff。
- **对比视图不被打断**：`diff_selection.rs::fill_select_diff_inline` 原先**无条件** `clear_directory_diff_state`，会让树在点文件时消失。新增 `directory_diff_survives_selection(repo_state, next)`——当新选择是**同一对 commit 的 `CommitRange`**（即点的是树里的文件，只有 `path` 变）时**不清理**；其它任何选择（不同 range / 单 commit / 工作区…）照旧退出目录对比模式。state 单测 `select_diff_within_the_directory_comparison_keeps_the_tree`。
- **目录下钻 + 返回上级**（#19 原意的"下钻"）：`DetailsPaneView` 新增 `directory_diff_root: Option<PathBuf>`；目录行尾部加 `›` 下钻入口（**不动**现有点击=折叠的交互，避免同一元素单/双击手势冲突），顶部出现 `↑ Up` + 当前路径面包屑；显示树 = `filter_by_prefix(&result.root, root)`；下钻根失效（新对比加载后路径不存在）时自动回退到对比根。
- **统计条补全（T-E）+ 位置结论**：新增纯函数 `directory_diff_stats_line`，按**当前显示目录**输出 `N files changed, +A -D, M subdirectories`（补上 #20 的"直接子目录数"，且随下钻变化）；样式加粗为头部。**位置结论：保留在 details 面板顶部**——目录对比本身就是 details 面板的一种模式（`render_directory_diff` 取代 `commit_details_view`），统计条与树是同一视图的头/体；放主面板会与其"逐文件 diff"职责冲突。UI 单测 `directory_diff_stats_line_reports_direct_subdirectories_and_follows_drill_down`。

**验证**：`cargo check -p worktree-state --tests` + `CARGO_TARGET_DIR=<C盘> cargo check -p worktree-ui-gpui --tests` 通过；`cargo fmt --check` 干净。（`worktree-ui-gpui` 在本机默认 D 盘 target 编不过——tree-sitter C 语法 `C1056`，需 C 盘 target 目录，见项目 MEMORY。）

**仍未做**：大目录虚拟化（T-F 半段，需动 `GitRepositoryDiff` trait 的 pathspec 裁剪，撞 P5 冻结，见上节）。
