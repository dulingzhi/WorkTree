# 特性 4：原生 `.git/hooks` UI — 设计（2026-09-18，2026-09-19 修正）

状态：**已确认（2026-09-19），落地中**。（P5 已废弃；本特性零 trait 冻结风险、零模型重构冲突。）

## §1 目标与范围

在 GitComet 内直接管理仓库的 `.git/hooks/`：

- 列出该仓库已定义 / 已启用的 git 钩子（pre-commit、pre-push、commit-msg…）。
- 一键启用 / 禁用（切换文件可执行位）。
- 编辑钩子脚本：复用既有 `Msg::OpenFileEditor`，在外部编辑器打开 `hooks/<name>`。
- 从 `.sample` 模板或空白新建钩子；删除钩子。
- 命令面板 `edit-hook` / `toggle-hook`。

**范围边界（in-bounds，零冻结风险）**：

- 不新增 / 不改任何 `worktree-core` trait 方法 → 不碰 P5 冻结（且 P5 已废弃）。
- 不改 `model.rs`/`sidebar.rs` 既有结构 → 不撞 P5 模型重构（已废弃）。
- 仅在 `worktree-core` 加**数据模型类型**（domain.rs）与**纯 `std::fs` 自由函数**（`hooks` 后端，Route B），在 `worktree-state` 加 effect/reducer/model 字段，在 `worktree-ui-gpui` 加一个 popover + 命令面板条目 + i18n。**不进入 `worktree-git-gix`、不改 `repo/mod.rs`、不新增/不改任何 trait 方法。**

## §2 架构与数据流（镜像 merge-tree / directory-diff）

```
用户打开仓库「Manage Hooks…」(context menu)
  → Msg::LoadRepoHooks { repo_id }
  → reducer: 置 model.repo_hooks = Loadable::Loading
  → Effect::LoadRepoHooks { repo_id }
  → effects 调度器 spawn_with_repo(repo) → worktree_core::hooks::list_hooks(repo.spec().workdir)
      返回 Arc<RepoHookList>
  → InternalMsg::RepoHooksLoaded { repo_id, result: Result<Arc<RepoHookList>, String> }
  → reducer: model.repo_hooks = Loadable::Ready(list) | Loadable::Error(msg)
UI PopoverKind::RepoHooks { repo_id } 读 model.repo_hooks 渲染：
  - 每个钩子行：名称 + 状态（defined/enabled）+ 开关 + 编辑按钮
  - 新建 / 删除
编辑      → Msg::OpenFileEditor { repo_id, path: hooks_dir.join(name) }
启用/禁用 → Msg::SetRepoHookEnabled { repo_id, name, enabled }
  → Effect::SetRepoHookEnabled → worktree_core::hooks::set_hook_enabled(hooks_dir, name, enabled)  (chmod ±x)
  → 重列或就地更新该行 enabled 位
```

**复用点（避免重复造轮子）**：

- `list_hooks` / `set_hook_enabled` / `create_hook` / `delete_hook` 是 **`worktree-core` 纯 `std::fs` 自由函数**（Route B），**不进入 `worktree-git-gix`、不改 `repo/mod.rs`、不新增/不改任何 trait 方法**。理由：`spawn_with_repo` 的闭包拿到的是 `Arc<dyn GitRepository>` trait object（`store/effects/util.rs:12`），`GixRepo` 固有方法不可达；而 `GitRepository::spec`（`services/mod.rs:447`）提供 `RepoSpec.workdir` 路径，`state` 层据此解析 `common_dir/hooks` 后直接调用这些自由函数。完全镜像 `schedule_load_directory_diff` 调 `repo.diff_range_files` 的「state 层编排」模式，只是后端从 trait 方法换成同 crate 自由函数。
- `Msg::OpenFileEditor`（`msg/message.rs:461`）已存在，编辑钩子直接 dispatch。
- hooks 目录解析：`git_dir_for_workdir(workdir)`（`path_utils.rs`）→ 普通仓库返回 `workdir` 本身，再 `join("hooks")`；bare / `.git` 后缀特例已由其处理。思路同 `lfs.rs:16` 用 `common_dir()` 取钩子目录。
- UI popover 八处注册镜像 `MergePreview`（`host/kinds.rs:141` + `geometry.rs:224` / `fingerprint.rs:163,464,671` / `dispatch.rs:156` / `dialog.rs:296` / `open.rs:189,588` / 面板 / 菜单入口）。
- `InternalMsg` 枚举在 `msg/message.rs:1176`，`DirectoryDiffLoaded` 在 `:1455`（仿其加 `RepoHooksLoaded`）。
- `Effect::LoadMergePreview` 在 `msg/effect.rs:636`（仿其加 `LoadRepoHooks` 等）。
- `LoadMergePreview` 发射点在 `actions_emit_effects.rs:911`（仿其加 `LoadRepoHooks` 发射）。
- `directory_diff_loaded` reducer 在 `store/reducer/diff_selection.rs:924`（仿其建 `repo_hooks_loaded`）。

## §3 对冻结 / P5 / 命名冲突的影响

- **P5 冻结（trait 签名）**：零影响 —— 不新增/不改任何 worktree-core trait 方法（`list_hooks` 等是 `worktree-core` 纯 `std::fs` 自由函数，不进 `worktree-git-gix`、不碰 `GitRepository*` trait）。且 P5 已废弃。
- **P5 模型重构冲突**：零 —— 只在 `model.rs` 追加一个 `repo_hooks: Loadable<Shared<RepoHookList>>` 字段，不改既有字段。
- **迭代 08 agent 钩子语义**：用 `RepoHook*` 命名，不与 `Msg` 既有/未来 agent 钩子 variant 撞名（见 D-H1）。

## §4 分步落地

**Step 1 — 数据模型（worktree-core）**

- `worktree-core/src/domain.rs` 加：
  - `pub struct RepoHookName(pub String)` + 标准名常量表（`PRE_COMMIT="pre-commit"` …）与 `as_str()`/`from_str()`；或 `pub enum RepoHookName { PreCommit, PrePush, … }`。**建议用 newtype + 常量表**，便于容纳用户自定义钩子名。
  - `pub struct RepoHook { pub name: RepoHookName, pub defined: bool, pub enabled: bool, pub has_sample: bool }`
  - `pub struct RepoHookList(pub Vec<RepoHook>)`
- 不碰任何 trait。

**Step 2 — worktree-core 纯 `std::fs` 后端（Route B，不进 worktree-git-gix）**

- 新文件 `worktree-core/src/hooks.rs`（与 `path_utils` 同级的兄弟模块）：
  - `pub fn list_hooks(workdir: &Path) -> Result<RepoHookList, String>`：用 `git_dir_for_workdir(workdir)`（`path_utils.rs`）解析 common dir → `join("hooks")`；对每个 curated 标准名 + 目录中任何额外非 `.sample` 文件，判定 `defined`（文件存在）、`enabled`（存在且可执行位）、`has_sample`（`<name>.sample` 存在）。
  - `pub fn set_hook_enabled(hooks_dir: &Path, name: &RepoHookName, enabled: bool) -> Result<(), String>`：`hooks/<name>` 设/清可执行位（`std::fs::set_permissions`；unix mode；Windows 走 `set_readonly(false)` + 备注 git-for-windows 机制差异）。
  - `pub fn create_hook(hooks_dir: &Path, name: &RepoHookName, from_sample: bool) -> Result<PathBuf, String>`：从 `<name>.sample` 复制或建空白 `#!/bin/sh` 骨架。
  - `pub fn delete_hook(hooks_dir: &Path, name: &RepoHookName) -> Result<(), String>`。
- `worktree-core/src/lib.rs` 加 `pub mod hooks;`（位次按 rustfmt；在 `path_utils` 附近）。
- 单测（temp repo）：写 hook 文件 + `set_permissions` 设 +x → `list_hooks` 返回 `enabled=true`；`set_hook_enabled(false)` 后 `enabled=false`；`create_hook(from_sample)` 生成文件且内容一致。
- **不进 `worktree-git-gix`、不改 `repo/mod.rs`、不新增/不改任何 trait 方法** —— 兑现「零 trait 改动」。

**Step 3 — state 层（worktree-state）**

- `msg/message.rs`：`Msg::LoadRepoHooks { repo_id }`、`Msg::SetRepoHookEnabled { repo_id, name, enabled }`、`Msg::CreateRepoHook { repo_id, name, from_sample }`、`Msg::DeleteRepoHook { repo_id, name }`、`Msg::CancelRepoHooks { repo_id }`（关闭清 Loading）。
- `msg/message.rs` `InternalMsg` 加 `RepoHooksLoaded { repo_id, result: Result<Arc<RepoHookList>, String> }`（仿 `DirectoryDiffLoaded`）。
- `msg/effect.rs`：`Effect::LoadRepoHooks { repo_id }`、`Effect::SetRepoHookEnabled { repo_id, name, enabled }`、`Effect::CreateRepoHook { .. }`、`Effect::DeleteRepoHook { .. }`（仿 `LoadMergePreview`）。
- `store/effects/repo_load.rs` 或新 `schedule_load_repo_hooks`：仿 `schedule_load_directory_diff:2091` 用 `spawn_with_repo` 调 `worktree_core::hooks::list_hooks(repo.spec().workdir)`，回 `InternalMsg::RepoHooksLoaded`。`set/create/delete` 走 `spawn_with_repo` 后重列或就地更新。
- `store/reducer/actions_emit_effects.rs`：`Msg::LoadRepoHooks` → `vec![Effect::LoadRepoHooks { .. }]`（仿 `LoadMergePreview` 发射点 `:911`）。
- `model.rs`：`RepoState` 加 `pub repo_hooks: Loadable<Shared<RepoHookList>>`（默认 `NotLoaded`）；新 `store/reducer/repo_hooks.rs`：`repo_hooks_loaded` + `set_repo_hook_enabled` 等 reducer（仿 `directory_diff_loaded:924`）。
- `reducer.rs` 分发臂 + `message_debug.rs` Debug 派生（仿 merge preview 的 `message_debug.rs:449`）。

**Step 4 — UI popover（worktree-ui-gpui）**

- 新文件 `view/panels/popover/repo_hooks.rs`：读 `repo_hooks` 的 `Loadable`，列表渲染每个钩子（名称 + 状态徽标 + 启用开关 + 编辑按钮），底部「新建钩子」下拉（标准名 + 从 sample / 空白）、删除按钮。编辑 → `Msg::OpenFileEditor`；启用/禁用 → `Msg::SetRepoHookEnabled`。仅「关闭」按钮 → `Msg::CancelRepoHooks`。
- `popover.rs` 加 `mod repo_hooks;`（位次按 rustfmt）。
- `host/kinds.rs` 加 `RepoHooks { repo_id }`（仿 `MergePreview:141`）。
- 八处注册（geometry/fingerprint×3/dispatch/dialog/open×2 + 菜单入口）仿 MergePreview。
- 仓库菜单加「Manage Hooks…」→ `ContextMenuAction::OpenPopover { kind: PopoverKind::RepoHooks { repo_id } }`（复用 `OpenPopover` variant，不新增），打开位置镜像 Repo Settings 菜单项。

**Step 5 — 命令面板**

- `view/command_palette.rs`：加 `edit-hook`（→ 弹 hooks popover 或直达 `OpenFileEditor`）、`toggle-hook`（→ `Msg::SetRepoHookEnabled`）。复用既有 `COMMANDS` / `REGISTERED_COMMAND_HANDLERS` 机制（A 档补漏已建双向对账网，新命令须入表且 label 以 `palette.` 开头）。

**Step 6 — i18n**

- `locales/panels.{en,zh-CN}.yml`：`repo_hooks: { title, close, enabled, disabled, not_defined, new_hook, from_sample, blank, delete, edit }`。
- `locales/context_menu_dynamic.{en,zh-CN}.yml`：`manage_hooks: "Manage Hooks" / "管理钩子"`。
- `locales/palette.{en,zh-CN}.yml`：`edit_hook` / `toggle_hook` 键（A 档补漏要求所有 palette label 以 `palette.` 开头）。

**Step 7 — 测试 / 门禁**

- `worktree-core` 单测（Step 2：`hooks.rs`）。
- `worktree-state` 单测：`LoadRepoHooks` 发射正确 effect；`repo_hooks_loaded` Ready/Error 落点；`SetRepoHookEnabled` 发射 effect。
- `cargo fmt --check` 干净。
- `cargo check -p worktree-state --tests` + `CARGO_TARGET_DIR="C:/Users/81468/AppData/Local/Temp/gc-target" cargo check -p worktree-ui-gpui --tests`（以 rc=0 为准，非扫 error 行）。
- `cargo test -p worktree-core --lib` / `-p worktree-state --lib` 全绿。

## §5 测试计划

| 层 | 用例 | 验收 |
|---|---|---|
| worktree-core | `list_hooks` 识别 defined/enabled/has_sample | temp repo 写 hook + 设 +x → enabled=true；清 +x → false |
| worktree-core | `set_hook_enabled` 切换可执行位 | chmod 前后 `list_hooks.enabled` 翻转 |
| worktree-core | `create_hook(from_sample)` | 生成文件且内容与 `.sample` 一致 |
| state | `LoadRepoHooks` → effect | `actions_emit_effects` 命中 `Effect::LoadRepoHooks` |
| state | `repo_hooks_loaded` Ready/Error | `model.repo_hooks` 落 Ready/Error |
| state | `SetRepoHookEnabled` → effect | 发射 `Effect::SetRepoHookEnabled` |
| ui | popover 八处注册 + mod 声明 | `cargo check` 无 error；mod 审计脚本无 MISSING |
| ui | （无自动化，编译器穷尽匹配兜底） | — |

## §6 决策点（建议值，可调整）

- **D-H1 命名**：默认 `RepoHook*`（`RepoHookName` / `RepoHook` / `RepoHookList` + `Msg/Effect/InternalMsg::*RepoHooks*`）。理由：避开迭代 08 agent 钩子在 `Msg` 的语义（feature-gap D-G3）。若坚持 `GitHook*`，需先确认 `Msg` 无 agent 钩子 variant 冲突。
- **D-H2 面板落点**：默认**独立 `PopoverKind::RepoHooks` popover**（从仓库菜单「Manage Hooks…」打开），不塞进 `repo_settings` popover（hooks 是文件、config 是键值，语义不同）。备选：塞进 `repo_settings`。
- **D-H3 钩子枚举**：默认**curated 标准名全列 + 额外文件也显示**（稳定清单 + 用户自定义钩子可见）。启用判定 = 文件存在且可执行位。Windows 下 git-for-windows 机制不同，标注 caveat。

## 风险与缓解

| 风险 | 缓解 |
|---|---|
| Windows 可执行位语义与 git-for-windows 不符 | v1 跟现有测试用的 +x 模型；UI 标注「启用 = 可执行位」，跨平台细节后续补 |
| 列表与「编辑后文件变化」不同步 | 编辑/启用/禁用/新建/删除后重发 `LoadRepoHooks` 重列（或就地更新该行） |
| 与迭代 08 agent 钩子命名冲突 | 强制 `RepoHook*` 前缀 |
| UI popover 漏注册致「假绿」 | 落地后跑 mod 审计脚本 + `cargo check` 以 rc=0 为准（非扫 error 行） |

## 验证（门禁 legs）

- `cargo fmt --check` 干净
- `cargo check -p worktree-state --tests` 通过
- `CARGO_TARGET_DIR="C:/Users/81468/AppData/Local/Temp/gc-target" cargo check -p worktree-ui-gpui --tests` 通过（rc=0，无 error）
- `cargo test -p worktree-core --lib` / `-p worktree-state --lib` 全绿

## 落地说明（待填）

（每步落地后在此补 commit + 验证结果）
