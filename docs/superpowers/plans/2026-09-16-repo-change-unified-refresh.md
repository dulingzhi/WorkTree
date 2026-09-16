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
| **P1** | 引入 `RepoChange` + `dispatch_repo_change()`，把 `repo_command_finished`/`repo_externally_changed` 收敛进来；统一先取消在途 | 新增 `repo_change.rs`；改 `external_and_history.rs`、`actions_emit_effects.rs` | 根除漂移 + race；刷新集合一处维护 |
| **P2** | 加 `content_rev` + 各 UI 的 `*_cache_rev()`，`history.rs` 等改用派生键 | `model.rs` + 各 pane | UI 侧统一感知、少维护 rev 列表 |
| **P3** | 评估非活跃仓库外部事件：是否放宽 `repo_monitor` 的 active 门控做"轻量标脏"（不实时全刷，激活时再刷） | `repo_monitor.rs` + 激活刷新 | 多仓场景下其它仓也能感知变化（按需） |

P0/P1 是用户体感问题的直接解；P2/P3 是"统一感知"的长期收口。建议按 P0→P1→P2 推进，P3 单独评估。

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
