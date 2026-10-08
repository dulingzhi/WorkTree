# Stacked-PR（堆叠分支）设计 / 接线文档

> 迭代 07 候选特性。本文档是 **P5 收口前**的预研设计：在 P5 模型冻结解除后可直接据此落地 `Msg` / `Effect` / reducer / UI，**不引入任何 `GitRepository*` trait 的 required 方法变更**（沿用迭代 06 已定的"零新 backend 接口"纪律）。

## 0. 状态总览

| 项 | 状态 | 说明 |
|---|---|---|
| 数据模型 `StackBranch` / `StackMetadata` | **已落地** | commit `91d92a71`，`worktree-core/src/domain.rs`，P5 安全 |
| 本文档（设计 / 接线草图） | **本文件** | P5 收口前可定稿 |
| `Msg` / `Effect` / reducer | **已落地** | commit `becb43ac`（`RestackStack` 接线 + `GitRepositoryHistory::restack_stack` default + reducer 单测） |
| gix `restack_stack` 编排 | **已落地** | commit `e3d39ce1`，`worktree-git-gix/src/repo/stack.rs`（`rebase --onto` 逐分支重放 + 冲突回滚单测） |
| 侧栏可视化 / 命令面板 | **已落地** | commit `1daaf842`（侧栏缩进 + 链线 + 顺序标识；`stack-branch` / `reorder-stack` 命令） |
| 命令面板 `restack` 入口 | **已落地** | `command_palette.rs` + `view/mod.rs` 派发 `Msg::RestackStack { base_branch: None }` |

## 1. 目标

- 在一个仓库内维护一组「堆叠」分支：`base → b1 → b2 → …`，每个分支建在其父分支之上。
- 支持：创建堆叠分支、重排顺序、整栈 restack（基于 gix `rebase --onto` 逐分支重放）、为任意分支创建 PR（已支持 `base_branch` 参数）。
- 可视化：侧栏以缩进 + 链线 + 顺序展示堆叠关系。

## 2. 数据模型（已落地，commit `91d92a71`）

`worktree-core/src/domain.rs`：

```rust
pub struct StackBranch {
    pub name: String,            // 普通分支 refname，复用 Branch.name 约定
    pub parent: Option<String>,  // 堆叠中的父分支；根分支为 None
    pub order: usize,            // 从 base 向上的位置
}

pub struct StackMetadata {
    pub branches: Vec<StackBranch>,
}
```

附 `ordered()` / `roots()` / `by_name()` / `parent_chain()`（含环检测）。P5 安全：仅 `domain` 原语，未碰 trait / `model.rs`。

## 3. 持久化（可直接做，P5 已取消）

- `StackMetadata` 落盘到 `UiSettings` / `session.json`，keyed by `RepoId`。
- 复用 `fetch_prune_deleted_remote_tracking_branches` 的 session 持久化范式（`RepoState` 加载 / `repo_management.rs` reducer / `session.json` 写入）。
- 单测：顺序持久化往返 + restack 后顺序守恒。

## 4. Msg（P5 已取消 2026-09-22，可直接开工）

| id | 载荷 | 备注 |
|---|---|---|
| `CreateStackedBranch` | `repo_id, name, parent: Option<String>` | 后端复用 `Msg::CreateBranch`，仅多记 `parent` |
| `ReorderStack` | `repo_id, ordered_names: Vec<String>` | 重排 `order` |
| `RestackStack` | `repo_id, base_branch: Option<String>` | 触发 gix restack 编排 |
| `DeleteStackBranch` | `repo_id, name` | 删分支 + 清除指向它的 `parent` 引用 |
| `LoadStackMetadata` | `repo_id` | 读盘 / 探测 |

> 不新增 `GitRepository*` required 方法；restack 走 `GitRepositoryHistory` 的 rebase default + gix override。

## 5. Effect（P5 已取消，可直接开工）

- `Effect::LoadStackMetadata { repo_id }` → `InternalMsg::StackMetadataLoaded { repo_id, result: Result<…> }`
- `Effect::PersistStackMetadata { repo_id, metadata }`
- `Effect::RestackStack { repo_id, plan }`（后端编排，见 §7）

## 6. model / reducer（P5 已取消，model.rs 可直接改；沿用 default 方法 + state 层复用纪律，不新增 trait required 方法）

- `RepoState` 增 `stacks: Loadable<Shared<StackMetadata>>`（keyed by `RepoId`；与 `DiffState` 同级容器）。
- reducer：`create_stacked_branch` / `reorder_stack` / `restack_stack` / `stack_metadata_loaded` / `delete_stack_branch`。
- 与 P5 模型拆分同波次，文件级避让 `model.rs`（参考目录 diff T-B 的 `diff_selection.rs` 隔离模式）。

## 7. gix 后端（P5 已取消，可直接做；restack 走 GitRepositoryHistory rebase default + gix override）

- 新 `worktree-git-gix/src/repo/stack.rs`：`restack_stack(base_branch)` 用 `rebase --onto` 逐分支重放。
- restack 顺序由 `StackMetadata::ordered()` 决定（base 在上，子分支在下）。
- 冲突处理：restack 中途冲突 → 中断并回滚到 restack 前状态，UI 提示用户在冲突分支手动解决后重试。

## 8. UI（P5 已取消，可直接做）

- `view/panels/sidebar.rs` `BranchSidebarRow`：堆叠分支缩进 + 链线 + 顺序标识。
- 命令面板 `stack-branch` / `restack` / `reorder`（`view/command_palette.rs`）。
- PR 落点：`create_request_url` 已支持 `base_branch: Option<&str>`（`forge_request.rs:75`）→ 堆叠建 PR 直连，无需新写。

## 9. 决策点

| # | 决策 | 触发 |
|---|---|---|
| **D-S1** | P5 模型拆分（本特性的原前置） | **已于 2026-09-22 取消**（分支 `p5-split-remaining` 删除）—— 不再是需收口的闸门；Stacked-PR 现可直接开工 |
| D-S2 | `StackMetadata` 落盘位置（UiSettings vs session） | 开工前 |
| D-S3 | restack 冲突时 UI 行为（中断 / 交互解决） | **已定：中断并回滚** —— 冲突 → `git rebase --abort`，再把已重放分支强制回退到 restack 前 tip，UI 提示用户在冲突分支手动解决后重试。已随 §7（commit `e3d39ce1`）实现 |

## 10. 与 P5 / P6 边界

- P5 已于 2026-09-22 取消，原「撞 P5 热文件」前提失效；`Msg` / `Effect` / reducer / UI 现在即可开工。沿用迭代 06 已定纪律：**不加 `GitRepository*` trait 的 required 方法**，用 default 方法 + state 层复用（同目录 diff T-B）。
- 命中率阈值收紧（P6-D1）与本特性正交，独立推进（见 §11）。P6-D1 已定：用 GitHub Actions hosted runner、不起自托管；`real_repo/*` 命中率预算在 hosted runner 按缺失跳过，收紧需日后有真仓库数据才验证。

## 11. 关联：命中率预算阈值收紧（P6-D1，独立项）

当前 `STRUCTURAL_BUDGETS`（`perf_budget_report/budgets/structural/history_cache.rs`）：

| 域 | 当前阈值 | 注释语义 |
|---|---|---|
| `hit_rate.log` | 20.0 | 低地板只抓 gross 回归 |
| `hit_rate.search` | 3.0 | break-even ~3% |
| `hit_rate.reflog` | 10.0 | 二次打开走磁盘 |
| `hit_rate.blame` | 30.0 | 内容寻址，near-always |

**卡点**：hosted CI 整段跳过 `real_repo/*`，这些预算在 CI 上从不求值。收紧阈值后没有任何真仓库数据可验证，必须自托管 runner（P6-D1）。在 runner 就绪前，改数值 = 盲改，不提交。

## 12. 验证（沿用迭代 06 四腿基线）

| 腿 | 命令 / 判据 |
|---|---|
| a | `cargo test --workspace --no-default-features --features gix` |
| b | `cargo test --workspace` |
| c | live clippy vs `clippy-baseline.txt` 逐字节 diff（零 diff） |
| d | `cargo test -p worktree-ui-gpui -- --list` 名单比对（零测试丢失） |

新增：restack 全链路 gix 单测（base→b1→b2 重放 + 冲突中断回滚）。
