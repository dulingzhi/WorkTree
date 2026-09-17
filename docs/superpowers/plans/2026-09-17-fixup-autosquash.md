# fixup / autosquash：设计

状态：**范围已确认（2026-09-17）** —— 走 **方案 B（完整工作流）**，且 **Autosquash 前先预览确认**（不照 squash 的「直接执行」尺度）。
确认结论：

- **范围 = 方案 B**：两个动作 —— ①「Fixup into…」（把当前改动提交为 `fixup! <subject>`）②「Autosquash from here…」（一键归并，不打开编辑器）。
- **安全尺度**：Autosquash **加预览确认弹窗**——执行前列出「哪些提交将被折叠进哪个目标」，用户过目后再跑。Fixup into 本身只产生一个新提交、不改写历史，**不弹窗**。
- **§2 决策点取 B1**（`compute_autosquash` 下沉 `worktree-core`）。
- **§3 取「拼 message + 复用现有 `Msg::Commit`」**（实测确认等价，后端零改动）。

按 §1–§6 落地，并在本文追加「落地说明」小节。

---

## §0 先看清现状：这功能比想象的窄

调研（含全仓库 grep 与逐函数追踪）结论——**已有的比预想多得多**：

| 组件 | 现状 | 位置 |
|---|---|---|
| `fixup! ` / `squash! ` / `amend! ` 前缀分组 | ✅ **已实现** | `panes/main/interactive_rebase.rs:25-48`（`autosquash_group_key`、`is_autosquash_prefixed`） |
| 按分组折叠成 Fixup 动作 | ✅ **已实现**（三种策略 ToTop/Neighbor/ToBottom） | 同文件 `autosquash_folds` / `compute_autosquash`；枚举 `popover/host/kinds.rs:36` |
| 折叠策略的 UI 入口 | ✅ **已实现**（右键菜单三项） | `context_menu/helpers.rs:252-266` → `impl_menu.rs:1217` |
| 用 `InteractiveRebaseEntry` 列表驱动真实 rebase | ✅ **已实现**（生产路径） | `git-gix/src/repo/history.rs:969-1069`（`run_planned_rebase` 调 `git rebase -i` + `GIT_SEQUENCE_EDITOR` 注入自建 todo） |
| 「自动归并 + 非交互 rebase」的 reducer 模板 | ✅ **已实现**（squash 在用） | `store/reducer/loaded_results/squash.rs:86-163`（加载 → 复验 HEAD/计数 → 改 action 为 `Fixup` → 发 `Effect::InteractiveRebase { interactive: false }`） |

**所以真正缺的只有两件事**：

1. **没有「生成 `fixup! <target>` 提交」的入口**。现在要产生一个 fixup 提交，用户只能手敲提交信息为 `fixup! xxx`——UI 完全没有这个动作。全仓库 `fixup!` 只出现在**解析侧**（`interactive_rebase.rs`），**没有任何构造侧**。
2. **没有「一键把 fixup 提交归并掉」的入口**。折叠逻辑挂在 **interactive rebase 编辑器内部**（打开编辑器 → 右键 → 选策略 → 再点 Start rebase），用户必须先进编辑器、且必须先有一个 rebase 范围。缺的是提交列表上的直接操作。

即：**解析与执行已齐，差的是「生产 fixup 提交」与「提交列表直达」两端。**

---

## §0.1 三个可选范围（**已定：方案 B**）

### 方案 A —— 只做「一键归并」（最小，S 档）

提交列表右键某提交 → **「Fixup 到上一个提交」**：等价于 `git commit --fixup <parent>` 吗？不是——那是给**工作区**改动生成 fixup 提交，需先有暂存内容。

更纯粹的 A 是：右键提交 X → **「把 X fixup 进其父提交」**，即自动构造一个 rebase：`pick <X 的父>` + `fixup <X>`，然后非交互执行。效果 = 把 X 合并进父提交且丢弃 X 的信息。

- 复用：`loaded_results/squash.rs` 模板（改 action 为 Fixup 的那 5 行）。
- 新增：1 个 `Msg::FixupCommitIntoParent`、1 个 reducer 分支、1 个右键菜单项、1 条 i18n。
- 风险：低。完全不碰 `squash.rs` / todo 组装 / 前端折叠。
- 局限：不是用户熟悉的 SmartGit/Tower 工作流（那边是「工作区改动 → fixup 到某个历史提交」）。

### 方案 B —— 完整工作流（**推荐**，M 档）

在 A 之上补上真正的 fixup 工作流，分两个动作：

1. **「Fixup into…」**：提交列表右键某提交 T（target）→ 菜单项「Fixup into this commit…」。语义：**把当前工作区/暂存的改动**，提交为 `fixup! <T 的 subject>`（即 `git commit --fixup=T`）。
   - 需要：在既有 `Msg::Commit` 上扩一个 `fixup_of: Option<CommitId>` 字段（或新增 `Msg::CommitFixup`），后端走 `git commit --fixup=<sha>`。
   - 这是用户最熟悉的一步（改了点东西 → fixup 到之前那个提交）。
2. **「Autosquash…」**：提交列表右键任意提交 → 「Autosquash from here…」。语义：以该提交为 base 打开/直接执行一次 rebase，**复用已有的前端 autosquash 折叠**把 `fixup!`/`squash!` 提交归并，非交互执行。
   - 需要：一个直接调 `compute_autosquash` 后发 `Effect::InteractiveRebase { interactive: false }` 的 reducer 路径（现有折叠逻辑在 UI 编辑器里，需抽到可复用处或从 UI 取 `entries`）。
   - 这一步是「一键」的关键：不必打开编辑器。

- 复用：折叠算法、todo 组装、rebase 执行**全都不用改**。
- 新增：`Msg` 2 个、reducer 2 个分支、右键菜单 2 项、i18n 若干、可能把 `compute_autosquash` 从 UI 抽到 `worktree-core`（**决策点，见 §2**）。
- 风险：中。抽 `compute_autosquash` 会动一个已有大量测试的文件（`interactive_rebase.rs` 内有约 150 行测试）。
- 价值：**这才是 SmartGit/Tower 的完整能力**，且开源侧少见。

### 方案 C —— B + `--autosquash` 语义对齐（L 档，不建议现在做）

额外对齐 git 原生 `--autosquash`（支持 `squash!` 的消息合并语义、`amend!` 的完整替换语义）。

- 现状：项目的 `compute_autosquash` 是**自研简化版**——它按 normalized subject 分组，`squash!` 与 `fixup!` 被**同等对待**（都折叠、都丢信息），与 git 语义不同（git 的 `squash!` 会开编辑器合并消息）。
- 风险：高（改语义会波及现有折叠测试与 squash 消息组装）。
- 建议：**单独排期**，不要混进本次。

> **我的建议：先做 A（半天级，立即可用、零风险），确认手感后再上 B。** 若你希望一步到位、接受中等改动面，直接选 B。

---

## §1 数据与时序（方案 B 为准，A 是其子集）

### 「Fixup into」一步

```
UI: 提交行右键 → "Fixup into this commit…" → ContextMenuAction::FixupIntoCommit { repo_id, target_commit_id }
  → Msg::CommitFixup { repo_id, target: CommitId, message? }   // 无需 message，git 自己拼 "fixup! <subject>"
  → reducer: 校验 target 在当前 HEAD 链上、工作区有可提交内容
  → Effect::Commit { ..., fixup_of: Some(target) }
  → git-gix: git commit --fixup=<sha>
```

- 前提校验：target 必须是 HEAD 的祖先（否则 `git commit --fixup` 仍会成功，但后续 rebase 归并不了——应提前警告）。
- 无暂存内容时：`Unsupported`→ 提示「没有可提交的改动」（与 `Msg::Commit` 同路径的错误处理）。

### 「Autosquash from here」一步

```
UI: 提交行右键 → "Autosquash from here…" → ContextMenuAction::AutosquashFrom { repo_id, base: CommitId }
  → Msg::Autosquash { repo_id, base }
  → reducer: 复用 squash 的两段式——先 Effect::LoadInteractiveRebaseSetup（已有），
     再在 loaded_results 里对 entries 跑折叠 → 发 Effect::InteractiveRebase { interactive: false }
```

**关键**：这与 squash 的异步两段式**完全同构**（`LoadSquashRebaseSetup` → `squash_rebase_setup_loaded`）。照抄即可，包括「加载后 HEAD 漂移就放弃」的复验。

---

## §2 决策点：`compute_autosquash` 放哪

现状在 `crates/worktree-ui-gpui/src/view/panes/main/interactive_rebase.rs`（UI 层），而 reducer（`worktree-state`）要用它 → **state 不能依赖 ui**。

| 选项 | 说明 | 取舍 |
|---|---|---|
| **B1 抽到 `worktree-core`** | 把 `autosquash_group_key`/`is_autosquash_prefixed`/`choose_autosquash_survivor`/`autosquash_folds`/`compute_autosquash` 移入 `crates/worktree-core/src/squash.rs`（与 `squash_run_final_entry` 等同类），UI 改为 re-export/调用 | **推荐**：语义归属正确（纯函数、无 UI 依赖）；UI 侧测试随函数一起搬。风险：`AutosquashMode` 在 UI 的 `kinds.rs`，需一并下沉或做映射 |
| B2 reducer 只发「加载」信号，折叠仍在 UI | reducer 收到 entries 后不折叠，改为把 entries 塞回 UI，由 UI 折叠后再 dispatch `Msg::InteractiveRebase` | 保住 UI 边界，但多一次往返、且「一键」变成两跳，时序脆弱 |
| B3 复制一份折叠逻辑到 core | 不动 UI | **拒绝**：两份实现必然漂移 |

**结论：走 B1**。`AutosquashMode` 下沉到 `worktree-core`（它是纯领域概念，不是 UI 概念），UI 侧 `kinds.rs` 改为 `pub use`。

---

## §3 对 P5 冻结的影响

**零影响**。本设计**不新增任何 `GitRepositoryDiff` trait 方法**：

- `git commit --fixup` 若走现有 commit 路径，只需给已有 `commit_*` 方法加一个参数 vs 新增 `commit_fixup_*`——**决策点见下**。
- autosquash 完全复用 `interactive_rebase_with_output`（已存在、已实现）。

> `commit_fixup` **会**动 `GitRepositoryCommit` trait（若有），需先确认该 trait 的 impl 数量。若 impl 多，则**不改 trait**，改为在 reducer/effect 层把 `fixup!` 前缀拼进 message 传给现有 `Msg::Commit`——`git commit --fixup=<sha>` 与 `-m "fixup! <subject>"` 的**唯一差别**是后者不会自动解析 target，但既然我们已知道 target、且能取到其 subject，**用普通 commit + 拼好的 message 即可达到等价效果**，零 trait 改动。

**✅ 已实测（2026-09-17，用真实 git 二进制验证）**：两种做法产出的提交信息**逐字节相同**——

```
$ git commit --fixup=HEAD        → subject: "fixup! feat: add thing"
$ git commit -m "fixup! feat: add thing"  → subject: "fixup! feat: add thing"
```

并且 `git rebase -i --autosquash` 能把这样产生的提交正确重写成 `fixup <sha>` 行：

```
pick  6d588d6 # add feature: 初版
fixup 09a1aca # fixup! add feature: 初版
```

**→ 结论：走「拼 message + 复用现有 `Msg::Commit`」路线，`worktree-git-gix` 与 git trait 零改动。** 这消除了本设计最大的风险点。

**仍待确认**：
- target 的 subject 含换行/双引号等特殊字符时的拼接安全性（`%s` 只取首行 subject，但需确认后端取 subject 的现有函数是否也只取首行）。
- `git commit --fixup` 对**合并提交** target 的行为（计划：target 为合并提交时直接禁用菜单项，不做特殊处理）。

---

## §4 UX 细节

- **菜单分组**：放在 commit 右键菜单的 history-rewrite 组，紧邻现有 `SquashSelectedCommits`（`context_menu/commit.rs:234`）与 `LoadInteractiveRebaseSetup`（`:588`）。
  - 新增项 ①：`FixupIntoCommit`（label `ui.label.commit.fixup_into`）
  - 新增项 ②：`AutosquashFrom`（label `ui.label.commit.autosquash_from`）
- **Autosquash 预览确认弹窗**（用户明确要求，与 squash 尺度不同）：
  - 时序：右键 → `Msg::Autosquash` → **加载 entries**（复用 `LoadInteractiveRebaseSetup` 的 Effect）→ 折叠（`compute_autosquash`）→ **不直接执行**，而是把「折叠计划」放进 state，打开一个确认弹窗。
  - 弹窗内容：一对一行 —— `fixup! xxx  →  折叠进  abc1234 xxx`；底部「N 个提交将被折叠，历史将被重写」。
  - 无可折叠项时：**不开弹窗**，直接 toast「没有找到可自动折叠的提交（fixup!/squash!）」，与现有 `apply_autosquash_mode` 的提示语义一致（`impl_menu.rs:1224`）。
  - 确认后：发 `Effect::InteractiveRebase { interactive: false }`（**复用 squash 的执行路径**，含 `begin_local_action` 以支持 undo）。
  - 取消：「取消」按钮关闭弹窗，state 里的折叠计划丢弃，不发任何 Effect。
  - 落点：`PopoverKind::AutosquashConfirm { repo_id, plan }` — 需在 `host/kinds.rs` 加变体，在 `popover/dispatch.rs` 注册面板，`fingerprint.rs` / `geometry.rs` / `open.rs` 各补一臂（照 `RepoSettingsPrompt` 的四处注册模式，本次刚做过一遍）。
- **重入保护**：`SequencerState != None`（rebase/cherry-pick 进行中）时两个菜单项都禁用或 toast 提示。
- **文案**：`ui.label.commit.fixup_into` / `autosquash_from`，中英各一。
- **危险提示**：两者都改写历史。现有 squash 有确认弹窗吗？——squash 走 `LoadSquashRebaseSetup`（先加载再确认）或直接执行；**本次应照 squash 的既定尺度**，不额外发明确认流程。
- **不可用态**：target 是 root 提交 / 非 HEAD 祖先 / HEAD 未处于可 rebase 状态（`SequencerState != None`）→ 菜单项置灰或点击给 toast。
- **命令面板**：是否同时加 palette 条目？**建议先不加**——palette 的 `REGISTERED_COMMAND_HANDLERS` 对账测试（本次刚加固）要求每个 palette 条目有 handler，而这两个动作**需要参数**（哪个 commit），不适合无参 palette。留待后续做「palette 支持带参命令」时再议。

---

## §5 测试计划

| 层 | 测什么 | 放哪 | 手法 |
|---|---|---|---|
| core | `compute_autosquash` 搬迁后行为不变 | `worktree-core/src/squash.rs` 的 `#[cfg(test)]` | 直接搬现有 UI 侧测试（纯函数） |
| core | `fixup!` message 拼接（含特殊字符 subject） | 同上 | 纯函数 |
| state | `FixupIntoCommit` → 发对的 Effect / 校验失败不发 | `store/tests/actions_emit_effects.rs` | 仿 `squash_ref_emits_effect`，构造 `AppState` 断言 `Vec<Effect>` |
| state | `Autosquash` 两段式：加载后 HEAD 漂移 → 放弃 | `store/tests/loaded_results.rs`（或 squash 同目录） | 仿 `squash_rebase_setup_loaded` 的现有测试 |
| git-gix | 端到端：造 repo → fixup 提交 → autosquash → 断言历史 | `git-gix/tests/squash_integration.rs`（**已用真实 git + `GIT_SEQUENCE_EDITOR` 注入 todo**） | 照抄该文件既有 interactive_rebase 用例 |
| ui | 右键菜单项在可用/不可用态的出现 | `popover/context_menu/tests.rs` | 纯函数 |

**门禁**：CI 因 billing 停摆（见 MEMORY），本轮以
`cargo check -p worktree-state --tests` + `CARGO_TARGET_DIR=<C盘> cargo test -p worktree-ui-gpui --lib <filter>` + `cargo test -p worktree-core --lib squash` + `cargo fmt --check` 为准。

---

## §6 落地顺序（确认范围后）

1. **B1 搬迁**：`compute_autosquash` 一族 → `worktree-core/src/squash.rs`（含 `AutosquashMode` 下沉 + UI 侧改引用 + 测试搬迁）。**先做这步且独立提交**，以便回归面清晰。
2. **core 纯函数**：fixup message 构造 + 单测。
3. **state**：`Msg` + reducer（照 `squash.rs` 模板）+ 单测。
4. **git-gix**：仅当拼 message 方案不成立时才动后端；否则零改动。
5. **UI**：右键菜单 2 项 + dispatcher + i18n + 菜单测试。
6. **集成测试**：`squash_integration.rs` 加端到端用例。
7. **文档**：本文补「落地说明」；`2026-09-15-feature-gap-tasks.md` 标注该项完成。

---

## 落地说明

### Step 1 — `compute_autosquash` 一族下沉 `worktree-core`（已完成，独立提交）

**做了什么**：
- `worktree-core/src/squash.rs` 新增（原样搬迁 + 加 `pub`）：`autosquash_group_key`、`is_autosquash_prefixed`、`choose_autosquash_survivor`、`autosquash_folds`、`compute_autosquash`。
- `AutosquashMode` 从 `worktree-ui-gpui/src/view/panels/popover/host/kinds.rs` 下沉到 `squash.rs`，UI 侧改为 `pub(in crate::view) use worktree_core::squash::AutosquashMode;`——**其余 20 余处 UI 引用零改动**（re-export 保住了原路径）。
- 6 个 autosquash 单测（`autosquash_to_bottom_folds_into_oldest` 等）随函数迁到 core 的 `#[cfg(test)] mod tests`，连带 `sc` 辅助函数。

**搬迁踩到的三个坑（都是 Rust 语义必然，非意外）**：

1. **孤儿规则**：原 `impl AutosquashMode { fn label() }` 在 `kinds.rs` 是**固有方法**；类型一旦移出本 crate，固有 impl 就不合法（`E0116`）。→ 改成自由函数 `autosquash_mode_label(mode)`，放 `state.rs`，并在 `host/mod.rs` 加 `pub(in crate::view) use` 对外暴露（`state` 模块是私有的，光 `pub` 函数还不够）。
2. **调用点导入**：`interactive_rebase.rs` 里 `autosquash_group_key` 的两处调用要显式 `use worktree_core::squash::autosquash_group_key`。
3. **`mod.rs` re-export 顺序**：`cargo fmt` 会把新增的 `pub use` 按字母序排到 `#[cfg(feature)]` 之后——首次手放位置不对会被 `fmt --check` 打回。

**没动的**：`expand_folded`（todo 重展开）留在 UI——它服务于编辑器视图的重展开，不属于"折叠规则"这一领域概念；它的 2 个测试也留在 UI。

**验证**：
- `cargo test -p worktree-core --lib squash` → **47 passed**（含 6 个搬迁来的）。
- `CARGO_TARGET_DIR=<C盘> cargo check -p worktree-ui-gpui --tests` → 无 error。
- `cargo fmt --check` 干净。

### Step 2 — core 的 fixup message 纯函数 + 单测（已完成）

- `worktree-core/src/squash.rs::fixup_message(target_subject)` 构造 `fixup! <subject>`。逐字节等价于 `git commit --fixup=<target>`（§3 已实测），故走普通 `Msg::Commit` 路径、后端零改动。
- 单测：`fixup_message_matches_gits_prefix`（中英/带单双引号 subject）、`fixup_message_keeps_only_the_first_line`（多行只取首行）、`fixup_message_folds_back_into_its_target`（构造出的 message 必须被 `autosquash_group_key` 归回 target）。全过。

### Step 3 — `Msg` / reducer（照 `loaded_results/squash.rs` 模板）+ 单测（已完成）

- `Msg::CommitFixup { repo_id, target: CommitId, push_after_commit: bool }`（`msg/message.rs:770`）—— 走 §3「拼 message + 复用 `Msg::Commit`」路线，不新增任何 git trait 方法。
- `Msg::Autosquash { repo_id, base }` / `ConfirmAutosquash { repo_id }` / `CancelAutosquash { repo_id }`（`msg/message.rs:779/785/790`）。
- `InternalMsg::AutosquashSetupLoaded { repo_id, base, result }`（`msg/message.rs:1313`）。
- `Effect::LoadAutosquashSetup { repo_id, base }`（`msg/effect.rs:630`）。
- `HistoryState::autosquash_preview: Loadable<AutosquashPlan>` + `autosquash_preview_rev: u64`（`model.rs:889-890`）；`set_autosquash_preview` 推进 rev（`model.rs:2346`）。
- reducer `autosquash` / `confirm_autosquash` / `cancel_autosquash`（`actions_emit_effects.rs:847/858/878`）：`confirm` 复用 `begin_local_action` + `Effect::InteractiveRebase { interactive: false }`，与 squash 执行路径同构；无 Ready plan 时只清 preview、不发 Effect。
- `loaded_results::autosquash_rebase_setup_loaded`（`loaded_results/autosquash.rs`）：块作用域解决借用冲突；HEAD 漂移 → 放弃 + `Warning` 通知；无可折叠 → `NotLoaded` + `Info`；`Err` → `NotLoaded` + `Error`。
- 单测 4 个（`loaded_results/tests.rs:1017` 起）：`autosquash_preview_ready_when_fixup_folds_into_target` / `_cancelled_when_head_drifted` / `_nothing_to_fold_notice` / `_cleared_on_load_error`，全过。

**🔴 本轮修的真实 bug（`compute_autosquash` todo 漏 fixup）**：
- 原 `Some(survivor)` 分支只把 fixup 塞进 `folded`（人看视图）、只把 survivor 推入 `collapsed`（rebase todo）。→ `plan.entries` 不含 fixup 提交。
- `worktree-git-gix/src/repo/history.rs` 的 `interactive_rebase_with_output`（约 969-996）在真正 rebase 前会 re-list `base..HEAD` 的 commit-id 集合，并与 plan todo 的 commit-id 集合排序后比较；**两者不一致即报错「branch changed since the rebase was set up」并中止**——漏 fixup 必然触发该 guard，且会错误丢弃 fixup。
- **修复**：fixup 也以 `InteractiveRebaseAction::Fixup` 留在 `collapsed`（todo）原位（注释见 `compute_autosquash`）。`plan.entries` 现在覆盖 `base..HEAD` 中每一个提交，guard 通过。
- 同步更新的测试：`build_autosquash_plan_folds_fixup_into_target_with_subject`（todo 断言补 `("F", Fixup)`）+ 6 个 `compute_autosquash` 单测的 `collapsed` id 列表（补上 fixup 提交 id，原位）。

### Step 4 — Autosquash 预览确认弹窗（已完成）

- `PopoverKind::AutosquashConfirm { repo_id, base }`（`host/kinds.rs`），照 `SquashPrompt` 的注册模式补全 8 处：`host/kinds.rs` / `popover.rs`(mod) / `dispatch.rs` / `fingerprint.rs`(repo_id 提取链 + preview-rev 哈希 + 类型判别哈希) / `geometry.rs`(`DIALOG_420_WIDTH`) / `open.rs`(dismiss 链 + 打开时 dispatch `Msg::Autosquash`) / `dialog.rs`(modal 链)。
- 新面板 `autosquash_confirm.rs`：从 `autosquash_preview` 读 `Loadable`；仅 `Ready(plan)` 且 `folded_count()>0` 渲染 fold 行（`fixup → 合并进 sha summary`）+ count 行；`Loading`/`NotLoaded` 显示加载中/空提示；确认按钮 `disabled` 当 plan 为空。确认 `ConfirmAutosquash` + close，取消 `CancelAutosquash` + close；标题按有无 fold 用 `title_count` / `title`。
- i18n：`prompts.en.yml` / `prompts.zh-CN.yml` 加 `autosquash:` 块（title / title_count / loading / empty / fold_row / count_line / confirm）。

### Step 5 — 右键菜单 2 项 + dispatcher + i18n（已完成）

- ① `Fixup into this commit`：`ContextMenuAction::FixupCommit { repo_id, commit_id }`（`context_menu_action.rs`），`commit.rs` 加菜单项（`git_commit.svg`，受 `history_rewrite_disabled` 约束），`impl_menu.rs` 加 dispatch 臂 → `Msg::CommitFixup { target: commit_id, push_after_commit: false }`。
- ② `Autosquash from here`：`commit.rs` 加菜单项（`git_commit.svg`，受 `history_rewrite_disabled` 约束，仅 `!is_head_commit` 时显示），`action: OpenPopover { kind: AutosquashConfirm { repo_id, base: sha } }`；走既有 `OpenPopover` 统一臂，无新增 dispatch。
- i18n：`context_menu_dynamic.en.yml` / `.zh-CN.yml` 加 `commit.autosquash` / `commit.fixup`（`%{short}` 插值）。

### Step 6 — `squash_integration.rs` 端到端用例（已完成）

- `autosquash_folds_fixup_commit_into_target_end_to_end`：造 `root → Feature X → fixup! Feature X`；`list_commits_for_interactive_rebase(&root)` → `build_autosquash_plan(entries, ToTop, root)` → `interactive_rebase_with_output(&root, &plan.entries)` → 断言历史 2 提交、`HEAD^ == root`、内容含 `feature fixed`、subject `Feature X`。全过。
- `autosquash_plan_is_none_when_no_fixup_commits`：First/Second 无可折叠 → `build_autosquash_plan(...).is_none()`。全过。
- 这两个 case 是暴露 Step 3「todo 漏 fixup」bug 的关键：纯 `build_autosquash_plan` 单测只看 `folds` 视图，没覆盖真实 rebase 后端的 commit-set guard。

### 剩余 / 遗留

- 命令面板带参命令（`Autosquash from here` 需 base 参数，不适合无参 palette）：按 §4 计划留待「palette 支持带参命令」时再议。
- §3 待确认项（合并提交 target 禁用、特殊字符 subject 拼接）已随 `fixup_message` 单测 + `history_rewrite_disabled` 约束消解。
- 统一刷新重构遗留的 `repo_monitor_active_repo_activation_coalesces_with_in_flight_refresh` flaky 不在本次范围。

**门禁结果（2026-09-17）**：`cargo fmt --check` 干净；`cargo check -p worktree-state --tests` 无 error；`CARGO_TARGET_DIR=<C盘> cargo check -p worktree-ui-gpui --tests` 无 error；`cargo test -p worktree-core --lib squash` **53 passed**；`cargo test -p worktree-state --lib` 全过；`cargo test -p worktree-git-gix --test squash_integration autosquash` **2 passed**。CI billing 停摆，本地门禁为准。
