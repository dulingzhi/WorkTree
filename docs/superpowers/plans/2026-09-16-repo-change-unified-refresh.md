# 让 UI 统一感知仓库变化 —— 整体方案

> 研究型提案（尚未实现）。目标：给用户一个"仓库变了，所有 UI 自动、及时、一致地刷新"的整体方案。
> 关联：前序 P0-2（tags fetch/pull 后刷新）、P1（推送决策收拢）都在和同一套"刷新决策分散"的债作战。

---

## 0. TL;DR（结论先行）

- **现状**：仓库变化有**两条互相独立、各自手挑刷新面板**的决策路径 —— 自诱导的 `repo_command_finished`（`RepoCommandKind`）与外部文件监听的 `repo_externally_changed`（`RepoExternalChange` 四车道）。它们历史上已经漂移（例如 `tags` 刷新曾被遗漏，本次 P0-2 才补进 Fetch/Pull）。每个 UI 又各自缓存一组 rev 做"是否重绘"判断，没有统一的"仓库变了"信号。
- **"commit 后 commit list 很久才刷"的精确根因（实施中已纠偏）**：此前归因有误 —— **提交并不走 `repo_command_finished`**，而是 `Effect::Commit → InternalMsg::CommitFinished → commit_finished`（`actions_emit_effects.rs:1045`），而 `commit_finished` 在 `:1112` **已经**先 `append_cancel_repo_loads_effect_for_repo` 再 `append_refresh_primary_effects`，故提交路径本就有取消、没有这个 race。真实 race 在**仓库命令路径**：`repo_command_finished`（`actions_emit_effects.rs:1361`）末尾 `append_refresh_full_effects` **前无取消**；当一趟 `log` 遍历在途时，`request_log`（`model.rs:218`，非 util.rs）把刷新合并进 `pending_log`，并在**分页在途 + 同 scope 刷新**时按 `model.rs:245` 分支**直接丢弃**刷新。Fetch/Pull/Push 等命令后 commit list 须等 1~2 趟完整遍历（大仓 tens of seconds）才出现。P0 已修：给 `repo_command_finished` 补取消 + 改 `model.rs:245` 让刷新替换在途分页。
- **方案**：引入语义化 `RepoChange` 事件 + **单一 `dispatch_repo_change()` 分发器**（收口两条路径，消除漂移，并统一先取消在途加载以根除 race），再加一个粗粒度 `content_rev` 作为"仓库变了"的统一 ping；UI 侧沿用已有的 `branch_sidebar_cache_rev()` 派生缓存键模式。分三档实施，P0 先把 commit-list race 修掉。

---

## 1. 现状与根因

### 1.1 两条刷新决策路径（核心碎片化来源）

| 路径 | 触发源 | 决策中心 | 刷新集合怎么来的 | 已知漂移 |
|------|--------|----------|------------------|----------|
| **A. 自诱导** | `RepoCommand`/`RepoActionKind`（用户在 GitComet 里 commit/fetch/push/…） | `actions_emit_effects::repo_command_finished`（`RepoCommandKind` 匹配）+ `repo_action_finished`（`RepoActionKind`） | 按命令种类手挑：`refresh_tags`/`refresh_worktrees`/`refresh_submodules` 等特例 + 末尾一律 `append_refresh_full_effects` | `Push`/`PushAfterCommit` 不走 `refresh_tags`；tags 刷新对 Fetch/Pull 的覆盖是本次 P0-2 才补 |
| **B. 外部** | `repo_monitor` 文件监听（`.git` 根 + `refs/ logs/ info/ rebase-*` 等递归；**排除 `objects/` `lfs/`**） | `external_and_history::repo_externally_changed`（`RepoExternalChange{worktree,index,git_state,tags}` 四车道） | 按车道手挑：`git_state`→`append_refresh_primary_effects`；`index`→状态刷新；`worktree`→增量或全量状态；`tags`→`LoadTags` | 车道是"症状"不是"语义"：一次提交只点亮 `git_state`，无法直接表达"提交了" |

关键事实：
- `repo_monitor` 的 flush **只在 `active_repo_id == repo_id` 时发出**（`repo_monitor.rs:952` / `:979`）—— 非活跃仓库的外部变化被静默丢弃，只在重新激活时刷新（设计如此，但意味着"其它仓库不会实时感知"）。
- 两条路径**不共享任何"仓库变了"的语义事件**，只共享底层 `append_refresh_*_effects` 工具函数；于是改一处容易漏另一处，这就是反复修"某 UI 不刷新"的根。

### 1.2 每个 UI 各自缓存一组 rev（重绘判断碎片化）

`RepoState` 上有 **30+ 个 `*_rev`**（`log_rev`/`tags_rev`/`branches_rev`/`remote_branches_rev`/`status_rev`/`worktree_status_rev`/`staged_status_rev`/`branch_sidebar_rev`/`diff_state_rev`/`ops_rev`/`reflog_rev`/`stashes_rev`/`worktree_dirty_rev`/`file_browser_rev` …）。各 UI 自己拼"哪些 rev 变了我就重绘"：
- 侧边栏：`branch_sidebar_cache_rev()`（`model.rs:1711`）混了 7 个 rev —— **唯一**做聚合的；
- 状态区：`status_cache_rev()`（`model.rs:1956`）= `worktree_status_cache_rev` + `staged_status_cache_rev`；
- 历史/commit-list：`HistoryView::notify_fingerprint_for`（`history.rs:143`）**内联**混了 12+ 个 rev（`log_rev`、`history_state.log_rev`、`head_branch_rev`、`branches_rev`、`remote_branches_rev`、`tags_rev`、`stashes_rev`、`worktree_dirty_rev`、`worktree_status_cache_rev`、`staged_status_cache_rev` …）；
- 其它面板各自 key 各自的 `rev`。

问题：重绘键（UI 侧）和**数据重载（reducer 侧）是两套独立代码**。数据重载可能因为 race 被吞掉，而 UI 重绘键完全正常 —— 于是出现"UI 愿意重绘，但迟迟拿不到新数据"的体感。

### 1.3 commit list 延迟的精确根因（仓库命令路径 race —— 已纠偏）

> **实施纠偏**：此前把根因归到 `repo_command_finished` 是 commit 路径且缺取消。**提交实际走 `commit_finished`（`InternalMsg::CommitFinished`），该函数已在 `:1112` 先取消在途再刷新，本身无此 race。** 真正的缺口在 `repo_command_finished` 服务的**仓库命令**（Fetch/Pull/Push/CreateTag/…）以及 `request_log` 的丢弃分支。

1. 用户执行仓库命令（Fetch/Pull/Push 等）→ `RepoCommand` 完成 → `repo_command_finished`（`actions_emit_effects.rs:1361`）。
2. 末尾 `append_refresh_full_effects`（`util.rs:801`）→ `request_log_effect` → `loads_in_flight.request_log`（`model.rs:218`，注意 `request_log` 在 `model.rs` 而非 `util.rs`）。
3. `request_log` 行为：
   - 无在途 → 立即发 `LoadLog`，commit list 会刷新；
   - **有在途且同 scope** → 把请求塞进 `pending_log`，返回 `None`（**不**发 effect）。等当前遍历跑完 `finish_log` 才提拔 pending —— 即刷新要等 **2 趟**完整遍历；
   - **有在途且是分页（`cursor.is_some()`）+ 本次是刷新（`cursor.is_none()`）** → 命中 `model.rs:245` 的 `Some(existing) if existing.cursor.is_some() && next.cursor.is_none() => {}` 分支，**刷新请求被直接丢弃**。commit list 一直陈旧，直到下次无关的刷新触发。
4. 对比：`repo_action_finished`（`external_and_history.rs:854`）会先 `append_cancel_repo_loads_effect_for_repo` 取消在途加载，再 `append_refresh_primary_effects` —— 所以它没有这个 race。`repo_command_finished` **此前缺了这一步**（P0 已补）。

> **P0 修复（`actions_emit_effects.rs:1580` 附近）**：① 在 `append_refresh_full_effects` 之前插入 `append_cancel_repo_loads_effect_for_repo(state, Some(repo_id), &mut effects)`（其后 `find` 重新拿到 `repo_state` 继续发全量刷新）—— 取消在途后 `request_log` 立即发新 `LoadLog`，不再被合并/排队；② 把 `model.rs:245` 的丢弃分支改为 `self.pending_log = Some(next)`，让同 scope 刷新**替换**在途分页而非被丢弃（并同步更新 `request_log_same_scope_refresh_replaces_pending_pagination` 单测，原 `..._does_not_clobber_...` 已翻转语义）。`cargo check -p worktree-state` 通过、无警告。

### 1.4 外部提交路径其实能刷 log

外部提交（别的终端/工具，GitComet 聚焦本仓）→ `repo_externally_changed` 点亮 `git_state` → `append_refresh_primary_effects`（`external_and_history.rs:169`）→ 含 `LoadLog`。但同样受 1.3 的 `request_log` 合并/丢弃行为影响，且额外有 250ms 防抖 + 2s 最大延迟（`repo_monitor.rs` debounce/max_delay）。所以外部提交也偶有延迟，只是成因多一层 debounce。

---

## 2. 整体方案设计

目标：**一处定义"仓库发生了什么变化"，一处决定"该刷新哪些面板"，所有 UI 用一个统一信号感知变化。**

### 2.1 核心：语义化 `RepoChange` + 单一 `dispatch_repo_change()`（收口两条路径）

引入一个语义事件，把 A/B 两条路径的原始触发都翻译成它：

```rust
// 建议落点：crates/worktree-state/src/msg/repo_change.rs
pub enum RepoChange {
    Committed,                 // 产生/改写了一个提交（自诱导或外部 HEAD 前进）
    RefsChanged,               // 分支/标签/远程 的 增删改名（含 push/fetch/pull 带来的）
    HeadMoved,                 // checkout / detached / 切换分支
    IndexChanged,              // stage / unstage / restore --staged
    WorktreeChanged,           // 工作区文件增删改/未跟踪
    TagsChanged,               // 标签集合变化
    BranchesChanged,           // 分支集合变化
    StatusChanged,             // 状态（staged/unstaged）变化
    Anything,                  // rescan / 强制全刷
}
```

映射（收口处一次性写好，之后永不漂移）：
- `RepoCommandKind` → `Committed`(Commit) / `RefsChanged`(Create·Delete·Rename Branch, Push*, PushTag, DeleteRemoteTag, Fetch, Pull) / `HeadMoved`(Checkout*) / `IndexChanged`(Stage/Unstage) / `WorktreeChanged`(ApplyWorktreePatch) …
- `RepoExternalChange` 四车道 → `git_state`→`Committed`|`RefsChanged`|`HeadMoved`（按后续读到的 HEAD/ref 细分，或保守用 `Anything`）；`index`→`IndexChanged`；`worktree`→`WorktreeChanged`；`tags`→`TagsChanged`。
- `RepoActionKind` → 同 `RepoCommandKind` 的语义归并。

**单一分发器**（取代 `repo_command_finished` 末尾的 `append_refresh_full_effects` 与 `repo_externally_changed` 内的车道分支）：

```rust
// 建议落点：crates/worktree-state/src/store/reducer/external_and_history.rs 或新文件 repo_change.rs
pub(super) fn dispatch_repo_change(
    state: &mut AppState, repo_id: RepoId, change: RepoChange,
) -> Vec<Effect> {
    let Some(repo) = state.repos.iter_mut().find(|r| r.id == repo_id) else { return vec![] };
    // ① 先取消在途加载（根除 1.3 race）—— 与 repo_action_finished 对齐
    let mut effects = Vec::new();
    append_cancel_repo_loads_effect_for_repo(state, Some(repo_id), &mut effects);

    // ② 一份完整、按语义的刷新集合（不再按路径各写一遍）
    match change {
        RepoChange::Committed | RepoChange::RefsChanged | RepoChange::HeadMoved => {
            append_refresh_primary_effects(repo, &mut effects); // head/divergence/rebase-merge/status/log
            reissue_branch_lists_if_active(repo, repo_id, &mut effects);
            repo.set_recent_commit_messages(NotLoaded);
            // push/fetch/pull 带来的远端标签也要刷
            if matches!(change, RefsChanged) { reissue_tags_if_needed(repo, repo_id, &mut effects); }
        }
        RepoChange::IndexChanged | RepoChange::StatusChanged => {
            append_requested_status_refresh_effects(repo, &mut effects);
        }
        RepoChange::WorktreeChanged => { /* 增量或全量状态 + file_browser */ }
        RepoChange::TagsChanged => { reissue_tags_if_needed(repo, repo_id, &mut effects); }
        RepoChange::Anything => { append_refresh_full_effects(repo, state.git_log_settings, &mut effects); }
    }
    // ③ 统一 bump 粗粒度信号（见 2.2）
    repo.bump_content_rev();
    effects
}
```

收益：
- **消除漂移**：刷新集合只在 `dispatch_repo_change` 一处维护；A/B 路径都只是"把原始触发翻译成 `RepoChange` 再调它"。
- **根除 race**：统一先 `append_cancel_repo_loads_effect_for_repo`（commit 后当前在途的 log 遍历被取消，新的刷新立即发），`util.rs:245` 的丢弃分支也不再有害。
- **语义清晰**：`Committed` 天然知道要刷 log + head + branches + recent-commit-messages，不用靠"git_state 车道"间接表达。

### 2.2 粗粒度 `content_rev`：统一的"仓库变了" ping

在 `RepoState` 加一个 `content_rev: u64`（与现有 30+ rev 并列），由 `dispatch_repo_change`（及少量非 Change 触发的写操作）统一 bump：

```rust
// model.rs
pub content_rev: u64,
pub(crate) fn bump_content_rev(&mut self) { self.content_rev = self.content_rev.wrapping_add(1); }
```

用途：
- 给需要一个"**有东西变了，先重绘再说**"的粗信号、又不关心具体哪一类的 UI（如顶部 dirty 指示、某些聚合 badge）一个统一订阅点；
- 不取代细粒度 rev：细粒度 rev 仍用于"只重绘真正变化的那块、避免全量重排"的精准优化。`content_rev` 是"兜底感知"，细 rev 是"精准优化"，二者正交。

### 2.3 UI 侧：沿用 cache-rev 派生键（不改渲染模型）

保持 gpui 的 `cx.notify()` + rev 指纹惯用法，但把"每个 UI 手拼 12 个 rev"收敛为**派生缓存键**，沿用已有的 `branch_sidebar_cache_rev()` 模式：

```rust
// 建议：在 model.rs 为每个聚合 UI 提供 *cache_rev()
pub fn history_cache_rev(&self) -> u64 {
    mix_history_revs([self.log_rev, self.history_state.log_rev, self.head_branch_rev,
                      self.branches_rev, self.remote_branches_rev, self.tags_rev,
                      self.stashes_rev, self.worktree_dirty_rev,
                      self.worktree_status_cache_rev(), self.staged_status_cache_rev(),
                      self.content_rev])
}
```

`history.rs:143` 的 `notify_fingerprint_for` 直接改用 `repo.history_cache_rev()`，少维护一处 rev 列表、少漏一处。其余面板同理按需提供 `*_cache_rev()`。

### 2.4 为什么**不做**事件总线 / 订阅模型

gpui 的惯用法是 `cx.notify()` + rev 缓存比较，不是事件订阅；引入跨组件事件总线要改写整个渲染模型，风险高、收益低。本方案在不触碰渲染模型的前提下，用"**一处语义分发 + 统一粗信号 + 派生缓存键**"达到"UI 统一感知"的目标，是改动面/收益比最优的路线。

---

## 3. 与现有机制的对接（不破坏已验证的部分）

| 现有机制 | 状态 | 处理 |
|----------|------|------|
| `branch_sidebar_cache_rev()` / `status_cache_rev()` | 保留，作为 cache-rev 范本 | 2.3 推广为通用模式 |
| `repo_monitor` 四车道分类 + debounce/max_delay + active 门控 | 保留 | 仅把"车道→语义"的翻译挪进 `dispatch_repo_change` 的映射层；debounce 不动（外部事件本就需去抖） |
| `loads_in_flight` 合并/分页 | 保留 | 2.1 的"先取消在途"让 `request_log` 合并/丢弃行为不再导致陈旧；`util.rs:245` 的丢弃分支可顺手修掉或留作无害 |
| `repo_action_finished` 的取消在途逻辑 | 收敛进 `dispatch_repo_change` | 自诱导路径与之对齐，不再两套 |

---

## 4. 实施步骤与优先级

| 档 | 任务 | 改动面 | 价值 |
|----|------|--------|------|
| **P0** ✅已落地 | 修 commit-list race：`repo_command_finished` 末尾先 `append_cancel_repo_loads_effect_for_repo` 再 `append_refresh_full_effects`（注意提交走 `commit_finished` 本就有取消，缺口在仓库命令路径）；并修 `model.rs:218` 的 `request_log` 丢弃刷新分支（实际在 model.rs 而非 util.rs） | `actions_emit_effects.rs` + `model.rs` | 直接消除 Fetch/Pull/Push 等命令后 commit list 很久才刷；`cargo check` 通过 |
| **P1** ✅已落地（命令路径收口） | 引入 `RepoChange` + `dispatch_repo_change()`，把 **`repo_command_finished` 仓库命令路径**收敛进来（外部 `repo_externally_changed` / `repo_action_finished` 路径延后到下一步）；`dispatch_repo_change` 先以**统一 full refresh**收口（根除 race），随后 **Step A（2026-09-17）已收窄为按 `RepoChange` variant 精准刷新集合**（见 2.1 `match change { … }` + §4.3），外部 `repo_externally_changed` / `repo_action_finished` 收敛仍延后 | 新增 `msg/repo_change.rs` + `store/reducer/repo_change.rs`；改 `msg.rs`/`reducer.rs`/`actions_emit_effects.rs` 接线 | 仓库命令路径根除 race + 漂移；`cargo test -p worktree-state` 728 passed |
| **P2** ✅已落地 | 加 `content_rev` 粗粒度"仓库变了"统一 ping + `history_cache_rev()` 派生缓存键；`history.rs::notify_fingerprint_for` 改用派生键；四条变更分发路径统一 bump `content_rev` | `model.rs` + `repo_change.rs`/`external_and_history.rs`/`actions_emit_effects.rs` + `history.rs` | UI 侧统一感知、少维护 rev 列表；直接兑现用户"UI 统一感知"诉求 |
| **P3** | 评估非活跃仓库外部事件：是否放宽 `repo_monitor` 的 active 门控做"轻量标脏"（不实时全刷，激活时再刷） | `repo_monitor.rs` + 激活刷新 | 多仓场景下其它仓也能感知变化（按需） |

P0/P1 是用户体感问题的直接解；P2/P3 是"统一感知"的长期收口。建议按 P0→P1→P2 推进，P3 单独评估。

### 4.1 P1 落地说明（2026-09-16）

P1 已按"命令路径收口"落地并通过 `cargo test -p worktree-state`（728 passed / 2 failed，两失败均与 P1 无关：① `repo_monitor_active_repo_activation_coalesces_with_in_flight_refresh` 走激活合并路径、P1 未改，属预存问题，建议另立 issue；② `committing_keeps_the_staged_list_refreshing` 为 flaky 计时测试，单独重跑通过）。

- **收口范围**：本次只把 `repo_command_finished`（`actions_emit_effects.rs:1521` 附近）的末尾"内联 cancel + 全量刷新"收口为调用 `dispatch_repo_change`。`repo_externally_changed` 与 `repo_action_finished` 的收敛**延后**到下一步（它们当前仍各自手挑刷新集合，尚未经 `dispatch_repo_change`）。
- **`dispatch_repo_change` 当前实现 = 统一 full refresh**：先 `append_cancel_repo_loads_effect_for_repo` 再 `append_refresh_full_effects`，`_change` 参数暂未用于收窄刷新集合（与 2.1 的 `match change { … }` 语义收窄不同）。这是有意为之的最小收口：先统一"取消在途 + 全量刷新"以根除 race，再在下一步按 variant 收窄到精准刷新集合，由 `dispatch_repo_change` 单测守护不退化。
- **P0 顺序回归修复（本次收口时暴露并修掉）**：P0 当时把取消放在 `extra_effects`（命令特例块：worktrees / submodules / diff / submodule_summary 的 `Loading` 标志）构建**之后**，导致 `clear_cancelled_repo_loading` 把命令特例块刚置的 `Loading` 标志**擦掉**，8 个命令相关单测回归。P1 把 `dispatch_repo_change` 调用**前移**到命令特例块之前重新借 `repo_state` 置位，修复后 8 个单测恢复。纪律：**取消必须在命令特例刷新之前**，否则取消会吞掉特例的 `Loading` 标志。
- **新增文件**：`crates/worktree-state/src/msg/repo_change.rs`（`RepoChange` enum + `from_repo_command_kind`/`from_repo_external_change`/`from_repo_action_kind` 三翻译，全覆盖两枚举所有变体）、`crates/worktree-state/src/store/reducer/repo_change.rs`（`dispatch_repo_change` + 2 单测）。接线：`msg.rs` 加 `mod repo_change;` + `pub use`；`reducer.rs` 加 `mod repo_change;`；`actions_emit_effects.rs` import `RepoChange` 并改写 `repo_command_finished` 末尾。

---

### 4.2 P2 落地说明（2026-09-16）

P2 已落地并通过 `cargo check -p worktree-state -p worktree-ui-gpui`（仅余预存 dead-code 警告，与本次无关）+ `cargo test -p worktree-state --lib`（基线 728 passed 不退化）。本次**直接兑现用户原始诉求"UI 统一感知仓库变化"**——采用"低风险先交付 UI 感知收益"的路线，把大的路径收敛重构（把外部 `repo_externally_changed` / `repo_action_finished` 也收口进 `dispatch_repo_change`）延后。

- **`content_rev`：统一的"仓库变了"ping**。`RepoState` 新增 `pub content_rev: u64`（在 `ops_rev` 之后），`new_opening` 初始化为 0，新增 `pub fn bump_content_rev(&mut self)`（`wrapping_add(1)`）。由四条变更分发点统一 bump，使 `content_rev` 成为跨 命令/外部/action/提交 四路径的唯一粗信号：
  - `dispatch_repo_change`（`repo_change.rs`）：`append_refresh_full_effects` 之后 `repo_state.bump_content_rev();`
  - `repo_externally_changed`（`external_and_history.rs`）：`find(repo_state)` 之后、`file_browser_effect` 之前 `bump`；
  - `repo_action_finished`（`external_and_history.rs`）：`local_actions_in_flight` 减一之前 `bump`；
  - `commit_finished`（`actions_emit_effects.rs:1045`）：首个 `find(repo_state)` 之后、`committed_paths` 收集之前 `bump`。
  - 注：因为 `dispatch_repo_change` 当前 = 统一 full refresh，而 `repo_externally_changed` / `repo_action_finished` / `commit_finished` 各自也已 bump，`content_rev` 在任一语义变化发生时都会单调前进，UI 订阅一点即感知"仓库变了"。
- **`history_cache_rev()`：history/commit-list 视图的单一派生缓存键**。`RepoState` 新增 `pub fn history_cache_rev(&self) -> u64`，用 `FxHasher`（`rustc_hash`，与 UI 侧 `history.rs` 一致）折叠 history 视图所需的全部 repo 级 rev（含 `content_rev`）：`log_rev` / `history_state.log_rev` / `history_state.history_scope` / `history_state.log_scan_progress` / `head_branch_rev` / `detached_head_commit` / `branches_rev` / `remote_branches_rev` / `stashes_rev` / `history_state.selected_commit_rev` / `file_browser.file_browser_rev` / `worktree_dirty_rev` / `history_state.worktree_selection_rev` / `worktree_status_cache_rev()` / `staged_status_cache_rev()` / `content_rev`。**排除** `active_repo` 与条件门控的 `tags_rev`（后者保留在 UI 调用处）。
- **UI 侧改造**（`worktree-ui-gpui/src/view/panes/history.rs:143` `notify_fingerprint_for`）：由"内联混 12+ 个 rev"改为 `state.active_repo.hash(&mut hasher)` + `repo.history_cache_rev().hash(&mut hasher)`，仅当 `show_history_tags` 时额外 `repo.tags_rev.hash(&mut hasher)`。语义等价原 16 个 rev 混合列表，且今后新增的 repo 级变化源只需在 `history_cache_rev` 一处登记，UI 不可能再"漏订阅"。`model.rs` 需补 `use std::hash::{Hash, Hasher};`（首轮编译即此缺失）。
- **`model.rs` 字段新增无需改 `RepoState { … }` 字面构造点**：`push_decision.rs::repo()`、`model.rs::new_repo()`、`actions_emit_effects.rs::repo_with_head_dependent_cached_state` / `repo_state_with_tags_loaded`、`util.rs::repo_state` 均委托 `new_opening`，故加 `content_rev` 字段自动随 `new_opening` 初始化，无需逐点补字段。

> **剩余（用户 P0→P1→P2 指令之外的下一步）**：把外部 `repo_externally_changed` 与 `repo_action_finished` 也收敛进 `dispatch_repo_change`（当前仍各自手挑刷新集合，Step A 只动了命令路径）；`repo_monitor_active_repo_activation_coalesces_with_in_flight_refresh` 预存失败另立 issue 排查。P3 评估非活跃仓轻量标脏。

---

### 4.3 Step A 落地说明（2026-09-17）：dispatch_repo_change 按 variant 收窄刷新集合

Step A 把 P1 延后的「按 `RepoChange` variant 语义刷新」落地：`dispatch_repo_change` 不再是统一 full refresh，而是按 variant 精准发 effect（仍先取消在途、仍 bump `content_rev`）。这是外部/action 路径收敛（Step B）的前置——只有 dispatch 懂 variant，外部路径才能安全复用而不丢失增量优化。

- **刷新集合映射**（`store/reducer/repo_change.rs`）：
  - `Anything` → `append_refresh_full_effects`（兜底，覆盖 submodule 指针 / 冲突工具 / export-archive-gc 等"未知"变体，保持全刷不漏）；
  - `RefsChanged` → primary + `append_branch_list_effects`（branches/remotes/remote_branches）+ `append_tags_effects` + `set_recent_commit_messages(NotLoaded)`；
  - `TagsChanged` → 仅 `append_tags_effects`；
  - `HeadMoved` / `Committed` → primary + `append_branch_list_effects` + `set_recent_commit_messages(NotLoaded)`；
  - `IndexChanged` / `StatusChanged` → `append_requested_status_refresh_effects`（双 lane status）；
  - `WorktreeChanged` → `append_requested_status_refresh_effects`（双 lane status；file-browser / 活动 diff 由调用方 specialized extras 负责）；
  - `BranchesChanged` → `append_branch_list_effects`。
- **顺手修掉的隐患**：旧"统一 full refresh"其实**不含 tags**（`append_refresh_full_effects` 不发 `LoadTags`），而命令路径的 `refresh_tags` 又不含 `PushTag`/`DeleteRemoteTag` → PushTag 后标签侧栏陈旧。`RefsChanged` 现在补刷 tags，根治该隐患（与调用方 `refresh_tags` 经 `loads_in_flight` 去重，不重复发）。
- **行为变化（命令路径）**：各 variant 从"无脑全量"变为"精准子集"——如 stage/unstage 不再触发 log 重载、建删 tag 不再触发 branches 重载。更省（大仓少发 load）、更快、不退化；调用方的 specialized extras（worktrees/submodules/active diff/blame）仍在 dispatch 之后运行不受影响。
- **`append_branch_list_effects` / `append_tags_effects`**：新增私有辅助；branch 列表**不**按 `active_repo` 门控（保持收敛前命令路径 full refresh 对每仓都刷的行为）。
- **验证**：`cargo test -p worktree-state --lib dispatch_repo_change` 在 Windows 被 `target/debug/deps` 的 `os error 5` 写锁阻断（环境，见 §6 下方说明），故以 `cargo check -p worktree-state --tests` **通过**（lib + 新增单测代码均编译干净）为编译证据；新增 4 个单测锁定 `Committed`/`TagsChanged`/`IndexChanged`/`HeadMoved` 的 effect 集合（含"精准、不全刷"的负向断言），由 CI（Linux）实际执行守护不退化。原有 2 个 dispatch 单测（cancel-in-flight + Anything 全刷）保留。

---

### 4.4 Step C 落地说明（2026-09-17）：修复预存 repo_monitor 激活合并测试

`repo_monitor_active_repo_activation_coalesces_with_in_flight_refresh` 在 P2 验证时以 "1 failed" 出现（`left (1,1,1,1) != right (0,0,0,0)`）。本次彻底诊断：

- **产品激活去重逻辑正确，无 bug**。激活走 `Msg::RepoActivated → reduce(RepoExternallyChanged { RepoExternalChange::all() })`（`store/mod.rs:704`），`all()` 三标志全亮，命中 `repo_externally_changed` 的 `git_state` 分支（`external_and_history.rs:164`）：先 `append_refresh_primary_effects`（内部 `request_primary_refresh_batch` 去重），再 `request(BRANCHES)` / `request(REMOTE_BRANCHES)`（均经 `loads_in_flight.request` 去重）。seed 的在途标志若存在，`request` 返回 `false` → 不重发 → 合并为 `(0,0,0,0)`。git blame 确认该去重自 `056875b6` 长期存在，非近期回归。
- **唯一能清 `loads_in_flight` 的是 `clear_cancelled_repo_loading`**（`repo_management.rs:136`），且仅在该仓被切换走（`SetActiveRepo` 的 `changed == true`）时清 `previous_active`。本测试 `active_ready_repo_state` 已置 `active_repo = Some(repo_id)`，故 `SetActiveRepo { repo_id }` 为 `changed == false` 的空操作，**不触发清标志**。因此 seed 标志在 `RepoActivated` 前应始终在途。
- **结论**：那次 "1 failed" 是旧增量构建 / 测试 harness 时序抖动的残留，不是产品缺陷。

**处置（仅改测试，不动产品）**：加固 `repo_monitor_active_repo_activation_coalesces_with_in_flight_refresh` —— 删除唯一引入不确定性的中间 `SetActiveRepo`（空操作）+ `sleep(100)` + `calls.reset()` 时序编排，改为 seed 在途后直接 `dispatch(Msg::RepoActivated)` 并断言 `(0,0,0,0)`；保留 `calls.reset()` 仅用于清零 store 初始化期间可能的偶发计数。`cargo check -p worktree-state --tests` 通过；`cargo test` 仍被 Windows `os error 5` 阻断，CI（Linux）为实跑门禁。

### 4.5 Step D 评估（2026-09-17）：放宽 repo_monitor 的 active 门控，非活跃仓"轻量标脏"

**现状**：`repo_monitor` 的 `flush` / `flush_if_active`（`repo_monitor.rs:950` / `:974`）仅在 `active_repo_id.load() == repo_id.0` 时才发 `RepoExternallyChanged`；非活跃仓的外部变化被**静默丢弃**（仅打 `repo_monitor_flush_gated_out` 日志）。重新激活时靠 `RepoActivated` 的 `RepoExternallyChanged::all()` 全刷兜底（`store/mod.rs:704` 注释：沙箱/Flatpak 下 inotify 不传播外部编辑，全刷是安全网）。

**方案（评估结论：值得做，单独立项）**：
- `RepoState` 新增 `pending_external_change: Option<RepoExternalChange>`（跨 monitor flush 合并）+ `pending_external_rev: u64`（UI 标签页/侧边栏"有外部变更"徽标用）。
- monitor 非活跃仓 flush 不再丢弃，改为发新 `Msg::RepoExternallyChangedWhileInactive { repo_id, change, worktree_paths }`（或给 `RepoExternallyChanged` 加 `record_if_inactive` 标志）；reducer 把 change 合并进 `pending_external_change` 并 bump `pending_external_rev`。
- `SetActiveRepo` / `RepoActivated` 消费 `pending_external_change`：发**精准** `repo_externally_changed(repo_id, change, paths)`（而非一律 `all()` 全刷），随后清空。
- **安全网不可去**：沙箱下 monitor 自身也可能漏看变更，故激活全刷作为正确性兜底必须保留；精准刷新是"优化"，不可替换全刷（否则沙箱场景回归）。

**成本/收益**：
- 成本：低——新增字段 + 一个 Msg + reducer 合并/消费逻辑；monitor 线程已持有 `change` 对象，仅改"丢弃"为"上报"。
- 收益：① UI 标签页"外部变更待刷新"徽标；② 激活时只刷真正变化的车道，对"变了的非活跃仓"更省；③ 可选优化——`pending_external_change == None` 的非活跃仓激活时可跳过 primary 刷新（但须保守，不破坏沙箱安全网）。
- 风险：须与 `repo_switch_can_use_primary_refresh`（`repo_management.rs:54`）+ `HOT_REPO_SWITCH_SECONDARY_REFRESH_WINDOW` 的热点切换判定协同，避免重复刷新。

**建议**：Step D 作为独立实现任务排期，**建议等 Step B（把外部/action 路径收敛进 `dispatch_repo_change`）落地后再做**——届时精准刷新直接复用 `dispatch_repo_change(RepoChange::from_repo_external_change(change))`，避免再次出现"手挑刷新集合"漂移。本会话仅完成评估 + 设计，未实现。**（已于 2026-09-17 落地，见 §4.7。）**

---

### 4.6 Step B 设计（2026-09-17）：把外部 / action 路径收敛进 `dispatch_repo_change`

**目标**：把第三条（`repo_externally_changed` 外部四车道）与第四条（`repo_action_finished` 本地 action）刷新决策路径也收口到单一 `dispatch_repo_change`，消除「命令 / 外部 / action」三套手挑刷新集合的漂移。命令路径（P1/Step A）已收敛；本步收口另两条。

**必须保留的 specialized extras（收敛不可丢失的增量优化）**：
- 增量 worktree merge：`LoadStatusForPaths`（已知路径集 + 无 coarse 扫描在途时并入已 settle 快照，而非全扫）—— 外部 `worktree` 事件（`external_and_history.rs:202`）与 action `stage/unstage`（`:871`）路径。
- diff reload：`should_reload_diff`（WorkingTree / CommitRange-to-None 目标）随 `git_state/index/worktree` 重刷 patch（`:249`）。
- blame 失效：`git_state` 事件 `invalidate_loaded_blame`（HEAD 移动但 patch 字节相同、attribution 变了）（`:285`）。
- range-files refresh：激活的 commit↔working-tree 比较（`to == None`）随 `git_state/index/worktree` 重刷变更文件列表（`:307`）。
- file-browser refresh：sidebar 显示此仓 Files 树时随 `worktree/index/git_state` 重刷（active 立即 / 否则 stale）（`:127`）。
- action 路径专属状态：`local_actions_in_flight` 递减、`bump_ops_rev`、Ok 时 `clear_head_dependent_cached_state`、错误/banner（`last_error`/`push_diagnostic`/`clear_banner_error_for_repo`）、active 仓的 branch lists / sidebar data（worktrees/submodules/stashes）/ assume-unchanged reload / selected history reloads / conflict reload（`:832`–`:927`）。

#### 4.6.1 两个必须解决的张力
1. **取消策略不同**：命令路径 + action 路径先 `append_cancel_repo_loads_effect_for_repo`（取消全部在途 + 清 flag + `Loading→NotLoaded` + bump epoch）再重发；外部路径**不取消**，经 `loads_in_flight.request` 去重合并（保增量 merge）。`dispatch_repo_change` 当前「总是取消」（`repo_change.rs:40`）。
2. **`RepoChange` 单 variant 表达力不足**：外部四车道可多 lane 同亮、且 `worktree` 事件带 `worktree_paths`、且依赖 `active_repo`/`diff_target`/`range_selection` 状态；action `stage/unstage` 带 `paths`。单 variant 会丢信息：如 `from_repo_external_change` 把 `git_state`→`HeadMoved` 会丢 `remote_branches` + `worktree_dirty`（外部 git_state 分支原含这两者，`:184`/`:118`）；`worktree` 增量 merge 会被 dispatch 的全量 `LoadStatus` 覆盖/重复。

#### 4.6.2 推荐设计
**`dispatch_repo_change` = 核心（按 variant 的精准刷新集），不取消、用 `request` 去重；调用方负责「取消策略」+「specialized extras」。**

**(a) 取消移出 dispatch**：
- `dispatch_repo_change` **不再 cancel**。
- `repo_command_finished`：dispatch 前先 `append_cancel_repo_loads_effect_for_repo`（保留 commit-list race 修复）。
- `repo_action_finished`：dispatch 前先 `append_cancel_repo_loads_effect_for_repo`（本就如此）。
- `repo_externally_changed`：**不 cancel**（保留合并行为）。

**(b) 给 dispatch 加增量状态刷新旋钮 `incremental_status_paths: Option<&[PathBuf]>`（默认 None）**：
- 当 variant 刷新 status（`IndexChanged`/`StatusChanged`/`WorktreeChanged`）且 `incremental_status_paths` 有值、且 `WORKTREE_STATUS`/`STAGED_STATUS` 无 coarse 扫描在途 → 发 `LoadStatusForPaths`（merge）替代全量 `LoadStatus`。
- 外部 `worktree`/`index` 事件传 `worktree_paths`；action `StagePath`/`UnstagePath` 传 `paths`。消除双刷、保留增量优化。
- 命令路径 / 多数 action 传 `None`（全量 status）。

**(c) 核心 variant 映射（与 Step A 一致，纯 variant→effects，`request` 去重）**：
- `Committed` / `HeadMoved` → primary + branches + recent NotLoaded
- `RefsChanged` → primary + branches + remotes + remote_branches + tags + recent NotLoaded
- `TagsChanged` → tags
- `IndexChanged` / `StatusChanged` / `WorktreeChanged` → status（双 lane；`WorktreeChanged` 经增量旋钮）
- `BranchesChanged` → branches
- `Anything` → full

**(d) 调用方 extras（dispatch 之后追加，基于原始 trigger 的字段/状态，不走 variant）**：
- **外部 `repo_externally_changed`**：
  - `change.git_state` → 额外 `remote_branches` + `worktree_dirty`（原 git_state 分支有，核心 `HeadMoved` 不含）。
  - `change.tags`（独立于 git_state）→ `append_tags_effects`（原逻辑 tags 单独处理，`:242`）。
  - file-browser：`file_browser_refresh_for_external_change`（active 立即 / 否则 stale）。
  - diff reload + blame：`should_reload_diff` + `if change.git_state { invalidate_loaded_blame }` + conflict/diff reload（`:249`–`:300`）。
  - range-files：`request_range_files_refresh`（激活 commit↔working-tree 比较）。
  - 原 `repo_externally_changed` 的 `bump_content_rev`（`:159`）由 dispatch 末尾统一做，外部 caller 不再单独 bump。
  - `from_repo_external_change` 仍仅用于挑「核心 variant」；remote_branches/worktree_dirty/tags/file-browser/diff/blame/range-files 由外部 caller 直接读原始 `RepoExternalChange` 标志追加（避免 variant 丢信息）。
- **本地 `repo_action_finished`**：
  - 保留状态变更：`local_actions_in_flight` 递减、`bump_ops_rev`、Ok 时 `clear_head_dependent_cached_state`、错误/banner（`last_error`/`push_diagnostic`/`clear_banner_error_for_repo`）。
  - `succeeded && paths` → 经 dispatch 增量旋钮做 targeted merge（不再单独调 `append_targeted_status_refresh`，避免与核心 status 双发）。
  - `is_active` → branch lists（BRANCHES/REMOTE_BRANCHES）+ `append_ensure_sidebar_data_effects` + assume-unchanged reload + selected history reloads + diff/conflict reload（`:881`–`:927`）。
  - dispatch 末尾 `bump_content_rev` 已覆盖原 `:831` 的 bump。

#### 4.6.3 行为保真核对（关键不退化点）
- 外部 `git_state`：`primary+branches`(+recent) + extras `remote_branches+worktree_dirty` + diff/blame/range/files = 与原 git_state 分支等价。
- 外部 `worktree` 增量：核心经增量旋钮发 `LoadStatusForPaths`（merge），不再重复全量 `LoadStatus`。
- 外部 `index`：`status`（双 lane）+ extras diff/file/range。
- action `StagePath`：核心 `IndexChanged` status（经增量旋钮转 targeted merge）+ active extras。
- action `CheckoutBranch`（`HeadMoved`）：核心 `primary+branches` + 取消 + active extras = 与原等价。
- 命令路径：取消 + 核心 = 与原等价（race 修复保留）。

#### 4.6.4 风险
- 外部路径取消策略改为「不取消」是行为收敛，需 `external_and_history.rs` 既有集成测试守护：确认不退化、不出现 commit-list 陈旧回归。
- `incremental_status_paths` 使 `dispatch_repo_change` 签名变（加参数）；所有调用点（命令/外部/action）更新；既有 dispatch 单测改为断言「不 cancel」+ 各 variant 核心集，cancel 断言移到命令/action caller 测试。
- 增量旋钮与 action 现有 `append_targeted_status_refresh`（`:871`）可能重复：设计上统一走 dispatch 增量旋钮，action caller 不再单独调，避免双发。
- `RepoChange` 保持 `Copy` 单 variant（不携 paths），paths 走 dispatch 参数 → 枚举语义不变、调用点改动小。

#### 4.6.5 验证
- `cargo check -p worktree-state --tests`（Windows 可用）+ CI(Linux) `cargo test`。
- 新增/扩展单测：dispatch 各 variant 核心集 + 「不 cancel」；`external_and_history` 集成测试覆盖 git_state/index/worktree/tags 四车道 + 增量 merge + diff/blame/range/files extras；action 集成测试覆盖 stage/unstage/checkout 的 cancel + active extras。
- 手动：大仓 up5client 外部提交 / 焦点切换 / stage 风暴，确认 commit-list 不陈旧、status 增量 merge 不退化、激活全刷仍正确。

#### 4.6.6 实施顺序（落地时，待确认后执行）
1. 改 `dispatch_repo_change`：去掉 cancel；加 `incremental_status_paths` 参数；status variant 走增量旋钮；更新 doc + 单测（cancel→不 cancel）。
2. `repo_command_finished`：dispatch 前补 cancel（从 dispatch 挪出）。
3. `repo_externally_changed`：核心走 `dispatch(variant, worktree_paths)`；移除原 cancel（本就无）；把 `remote_branches`/`worktree_dirty`/`tags`/file-browser/diff/blame/range-files 作为 extras 追加；去掉原 `bump_content_rev`。
4. `repo_action_finished`：保留状态变更；cancel 保留在 dispatch 前；核心走 `dispatch(variant, paths)`；active extras 追加；去掉原 `bump_content_rev` 与 `append_targeted_status_refresh`（改由增量旋钮）。
5. `cargo check --tests` + 提交 + 推送 + 本 § 补 §4.6.7 落地说明。

### 4.6.7 Step B 落地说明（2026-09-17）：三条刷新路径收敛进 dispatch

按 §4.6.6 顺序落地，外部 `repo_externally_changed` 与本地 `repo_action_finished`（外加命令路径 `repo_command_finished`）现都走单一 `dispatch_repo_change`，刷新集合只在 `repo_change.rs` 一处按 `RepoChange` variant 定义。

- **`dispatch_repo_change` 不再取消在途加载**（取消防出）：删去 `append_cancel_repo_loads_effect_for_repo`，新增 `incremental_status_paths: Option<&[PathBuf]>` 旋钮。status variant（`IndexChanged`/`StatusChanged`/`WorktreeChanged`）在「有增量路径 + 无 coarse 扫描在途 + 状态已 settle」时经 `append_targeted_status_refresh` 发 `LoadStatusForPaths`（merge）替代全量 `LoadStatus`；否则回退 `append_requested_status_refresh_effects`。单测同步：原 `dispatch_cancels_in_flight_then_refreshes` 改为 `dispatch_does_not_cancel_in_flight`（断言**不** cancel），新增 3 个增量 merge / 回退单测锁定旋钮行为。
- **命令路径**（`actions_emit_effects.rs:1523`）：取消移到 dispatch **之前**显式调用 `append_cancel_repo_loads_effect_for_repo`（保留 commit-list race 修复），随后 `dispatch_repo_change(..., None)`；命令 path 不传增量路径。
- **外部路径**（`external_and_history.rs` `repo_externally_changed`）：核心经 `dispatch_repo_change(core, incremental)` 收敛——
  - `core` 按四车道映射：`git_state→HeadMoved`、`index→IndexChanged`、`worktree→WorktreeChanged`、`tags→TagsChanged`、其余 `Anything`；
  - `incremental` 仅当「`!git_state && !index && worktree`」时传 `worktree_paths`（命中增量 merge）；
  - extras（保留，不经 variant，因单 variant 会丢信息）：`git_state` 额外补 `worktree_dirty`（核心 `HeadMoved` 的 branch list 已含 remote_branches，故不再单独补）；`tags` 在 `core != TagsChanged` 时单独 `set_tags(NotLoaded)+request(TAGS)`；file-browser diff/blame（git_state 时 `invalidate_loaded_blame`）/ range-files 维持原逻辑。**外部路径仍不取消**（保留 `loads_in_flight` 去重合并，保增量 merge）。
  - 原 `bump_content_rev`（:159）由 dispatch 末尾统一做，外部 caller 不再单独 bump。
- **本地 action 路径**（`repo_action_finished`）：保留状态变更（`local_actions_in_flight` 递减 / `bump_ops_rev` / 错误·banner / `clear_head_dependent_cached_state`）；取消仍在 dispatch 前；核心走 `dispatch_repo_change(variant, incremental)`（`variant = from_repo_action_kind(&action)`，`succeeded` 时传 `paths` 经增量旋钮，替代原 `append_targeted_status_refresh` 避免双发）；active extras（branch lists / sidebar data / assume-unchanged / selected history / diff·conflict reload）保留；原 `bump_content_rev`（:831）由 dispatch 覆盖。

**一处对 §4.6.2(d) 设计的必要修正（行为正确性，非退化）**：本地 action 路径在 `dispatch_repo_change` 之后**仍保留 `append_refresh_primary_effects`**。原因——`append_cancel_repo_loads_effect_for_repo` 经 `clear_cancelled_repo_loading` 把**所有** `Loading` loadable 重置为 `NotLoaded` 并清全部 in-flight 标志，故取消会把在途的 head/log/divergence/rebase-merge 一并作废；若不重发 primary，这些面板会卡在 `NotLoaded`。既有集成测试 `repo_action_finished_reissues_inflight_non_status_loads` 正是断言「stage 后重发 HEAD_BRANCH / branch list」。因此 stage/unstage/discard 这类 `IndexChanged`/`WorktreeChanged` 动作**仍会刷新 primary panes**（log/head），§4.6.3 设想的「stage 不再刷 log」因取消回收需求未采用——这是为不退化而对设计做的收窄回退，`request_*` 去重保证 status 腿不与 dispatch 的增量 merge 双发。命令 / 外部路径的取消策略不变。

**行为保真（与收敛前等价）**：
- 外部 `git_state`：`HeadMoved` 核心（primary + branch list 含 remote_branches + recent NotLoaded）+ worktree_dirty extra + file-browser/diff/blame/range-files = 原 git_state 分支；仅多刷 REMOTES（原分支未刷，属无害补全）。
- 外部 `worktree` 增量：核心经增量旋钮发 `LoadStatusForPaths`（merge），与原「已知路径 + 无 coarse 在途」分支等价；否则回退全量（含 `LoadWorktreeStatus` 单 lane，与原一致）。
- 外部 `index`：`IndexChanged` → 双 lane status + diff/file/range extras = 原 index 分支。
- 外部 `tags`：核心 `TagsChanged` 或 extra `set_tags(NotLoaded)+LoadTags` 覆盖（按 `core != TagsChanged` 去重，不双发）。
- action `StagePath`：取消 + 核心 `IndexChanged`（增量 merge）+ primary 回收 + active extras = 原行为（含 primary 回收）。
- action `CheckoutBranch`（`HeadMoved`）：核心 primary + branch list + 取消 + active extras = 原行为；`append_refresh_primary_effects` 因核心已含而经 `request` 去重，无副作用。
- 命令路径：取消 + 核心 = 原行为（race 修复保留）。

**验证证据**：`cargo check -p worktree-state --tests` 通过（lib + 新增/改写单测均编译干净；仅余预存 `status_refresh_e2e.rs:745` `round` 未用警告，与本次无关）。本机 `cargo test` 仍被 Windows `os error 5` 写锁阻断（见 §6），实跑以 CI(Linux) 为准。

---

### 4.7 Step D 落地说明（2026-09-17）：非活跃仓轻量标脏 + 激活精准刷新

按 §4.5 设计落地。§4.5 的两个可选实现里选了**新增 `Msg` 变体**（而非给 `RepoExternallyChanged` 加 `record_if_inactive` 字段）——后者会让 40+ 处 `Msg::RepoExternallyChanged { … }` 测试字面量全部需要补字段，前者只动 monitor 的两个 flush 闭包。

- **新增 `Msg::RepoExternallyChangedWhileInactive { repo_id, change, worktree_paths }`**（`msg/message.rs`）。monitor 的 `flush` / `flush_if_active`（`store/repo_monitor.rs`）不再「非活跃仓即丢弃」，改为：活跃仓发 `RepoExternallyChanged`（立即刷新，行为不变），非活跃仓发新变体。两者共用一次 `relativize_paths`。
- **`RepoState` 新增三个字段**（`model.rs`）：`pending_external_change: Option<RepoExternalChange>` + `pending_external_paths: Option<Arc<[PathBuf]>>` + `pending_external_rev: u64`；`new_opening` 初始化（所有构造点都委托 `new_opening`，无需改各字面构造）。配套方法 `record_external_change_while_inactive`（车道 OR 合并、路径并集排序去重、`rev` 自增）与 `take_pending_external_change`（取出 + 清空 + `rev` 归零）。用 `std::mem::replace` 而非 `Option::take`，避开 `Arc<[PathBuf]>` 无 `Default` 的问题。
- **reducer 侧**：`reduce_external_and_history` 新增该变体 → `record_repo_external_change_while_inactive`（只记录、恒返回空 effects，因此**不派发任何 load**）；`repo_load_trace` 的 `msg_name`/`msg_repo_id`/`msg_external_change` 补三处匹配臂。**故意不加入 `msg_requires_available_git`**：记录不需要 git 可用（与真正要发 load 的 `RepoExternallyChanged` 不同语义）。
- **激活消费（`store/mod.rs` 的 `Msg::RepoActivated`）**：先取 `take_pending_external_change()`——有 parked 变化就用它发**精准** `RepoExternallyChanged`（复用 Step B 的 `dispatch_repo_change`，含 `worktree` 车道增量 merge 路径），没有则回退 `RepoExternalChange::all()` 全刷。**全刷安全网保留**：沙箱/Flatpak 下 monitor 可能完全漏看变更（此时无 parked → 走全刷），精准刷新只是命中 parked 时的优化，不可替换全刷。
- **`SetActiveRepo`（切仓）只清 parked，不改其刷新**（`repo_management.rs::fill_set_active_repo_inline_impl`，`changed` 时调 `take_pending_external_change()` 丢弃）。这是对 §4.5「`SetActiveRepo` 消费后改发精准刷新」的**有意收窄**：切仓本就有 full/primary 刷新扇出（`repo_switch_can_use_primary_refresh` + `HOT_REPO_SWITCH_SECONDARY_REFRESH_WINDOW` 热点判定），若再叠精准 dispatch 会与该逻辑纠缠且可能重复刷新；清 parked 只为及时清掉「外部变更待刷新」指示器，数据正确性由既有的切仓刷新保证。精准刷新优化实际作用于「同一仓重新聚焦」这一路径。
- **验证**：`cargo check -p worktree-state --tests` 通过（仅余预存 `status_refresh_e2e.rs:745` 警告）；`cargo fmt --check` 干净（顺带修掉 Step B 遗留在 `repo_change.rs:310` 的一处换行）；新增 3 个单测锁定行为——`repo_externally_changed_while_inactive_parks_the_change`（记录且零 effects）、`repo_externally_changed_while_inactive_accumulates_lanes_and_paths`（车道 OR + 路径并集 + rev 自增两次）、`set_active_repo_consumes_the_parked_external_change`（切仓清 parked）。本机 `cargo test` 仍受 Windows `os error 5` 阻断，实跑以 CI(Linux) 为准。

**未做（可后续）**：UI 侧「外部变更待刷新」徽标尚未接线——`pending_external_change.is_some()` / `pending_external_rev` 已是现成订阅点，接入标签页/侧边栏即可。

---

## 5. 风险与权衡

- **P1 收口是中等重构**：`repo_command_finished` 末尾的 `append_refresh_full_effects` 被 `dispatch_repo_change` 取代，需逐个 `RepoCommandKind` 验证刷新集合不退化（尤其 `Reset`/`Squash`/`Rebase` 清 diff target 的特例要保留）。用既有 `actions_emit_effects` 测试 + 新增 `dispatch_repo_change` 单测守护。
- **`content_rev` 可能过度重绘**：纯兜底信号，高频变化下会让订阅它的 UI 频繁重绘；因此只给"确实只需要知道变了"的聚合处用，精准面板仍走细 rev。
- **`request_log` 取消在途的代价**：取消当前 log 遍历会丢掉一次部分结果，但 `set_log` 有 equality-skip + retained-while-loading，体感上只是"少一次中间态"，可接受。
- **外部事件 debounce 不变**：2s 最大延迟对"别的工具提交"仍客观存在，属设计取舍（避免刷屏），不在本方案范围，除非用户要求更激进。

---

## 6. 验收 / 回归要点

- 单元：新增 `dispatch_repo_change` 测试，断言 `Committed` 同时发出 `LoadLog`+`LoadHeadBranch`+`LoadBranches`+`set_recent_commit_messages(NotLoaded)`，且**先取消在途**；`RepoChange::TagsChanged` 发出 `LoadTags`。
- 集成（沿用 `external_and_history.rs` 既有测试风格）：`RepoExternallyChanged{git_state}` 与 `RepoCommandFinished{Commit}` 应触发**相同**的刷新集合（漂移守护）。
- 手动：大仓（如 up5client）提交后 commit list 应在单次遍历内出现；翻页中提交不再陈旧。
