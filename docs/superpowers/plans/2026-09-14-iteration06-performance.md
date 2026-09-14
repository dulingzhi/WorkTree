# 迭代 06 — v0.7.0「性能可证明」实施计划

- 日期：2026-09-14（分支 `dev`，HEAD = `1c466575`）
- 上位方案：[docs/roadmap-long-term.md](../../roadmap-long-term.md) §3 迭代 06
- 前置：roadmap 迭代 01–05 全部完成；重构 P0–P4 已合入 dev，**P5 进行中**（本迭代与 P5 并行，见「并行约束」）
- 执行模式：SDD（实现者 agent + 独立评审 agent + 台账 + 修复循环），沿用 P4 纪律
- 状态：**待批准**

## 背景：初稿判断已被实测修正

上位方案初稿写的是「没有可公开复现的基准，需要建基准套件」。实测后**证伪**——测量基建已经齐备：

| 已有 | 位置 |
|---|---|
| 预算框架 structural 7 域 + timing 4 域 | `worktree-ui-gpui/src/bin/perf_budget_report/budgets/` |
| 合成仓库档位 | `open_repo/*`、`history_cache_build/50k_commits_2k_refs_200_stashes`、`branch_sidebar/20k_branches_100_remotes`、`repo_switch/20_repos_all_hot` |
| 真实仓库接入点 | `benches/performance/real_repo.rs`（`WORKTREE_PERF_REAL_REPO_ROOT`） |
| 冷启动 harness | `perf-app-launch`（cold/warm × 1/5/20 repos） |
| 空闲资源 harness | `perf_idle_resource`（CPU、内存增长、后台刷新、睡眠唤醒） |
| 工具链 | `scripts/{run-full-perf-suite,compare-perf-runs,archive-perf-run}.sh` |

真实缺口是三处，**都不是"建基准"**：门控未通、数字未出仓、冷启动有测量无优化。

## 目标与非目标

**目标**（按优先级）：
1. 让已有 strict 门控**真正运行**（或明确记录它为何不能运行）
2. 让性能数字**出仓**——README 可引用，每个 release 可对比
3. 冷启动与大 diff 两条**用户体感最强**的链路做出可测量改善

**非目标**：
- 不新建基准框架（现有 `perf_budget_report` 够用）
- 不做 UI 行为变更（本迭代不引入任何用户可见功能）
- 不追求全仓库性能翻番——先拿能对外承诺的数字，再谈优化

## 并行约束（与 P5 的边界）

P5「split remaining」正在 `p5-split-remaining` 分支上跑（45 提交），dev 已合 2 件。两者同改 UI crate 的概率高：

- **T1/T2/T3**（CI 与文档）几乎不碰 `worktree-ui-gpui/src` → **与 P5 无冲突，可先行**
- **T4/T5/T6**（冷启动、history cache、大 diff）会碰 `view/`、`git-gix/repo/` → **等 P5 收口或与其协商文件级避让**
- 建议顺序：T0 → T1/T2/T3 → （P5 收口）→ T4/T5/T6/T7

## 任务波次

### Wave 0 — 前置核查（**阻塞后续，先做**）

**T0 核查 strict 门控是否真的跑过** `S`
- 目标：确认 GitHub repo variables `PERF_RUNNER` 与 `PERF_REAL_REPO_ROOT` 的配置状态。
- 判据：`perf.yml` 的 `performance-budgets-full` job 中，`vars.PERF_RUNNER` 非空才走 `--strict`；否则 fallback 到 `--skip-missing` 且 job 本身 `continue-on-error: true`。
- 三种结论分别导向不同排期：
  - **A 两者均已配置** → strict 已在跑，T1/T2 降级为"数字出仓 + PR 子集拆分"，性能底数已可直接读取
  - **B 仅配置其一或全未配置** → strict **从未生效**，T2 升级为高优先级（这是本迭代最高价值的发现）
  - **C 无法配置（无自持 runner）** → 需决策：接受共享 runner 的噪声门控，或改为"基准只对外发布不做 CI 门控"
- 交付：结论 + 证据（CI run 链接或变量截图）写入本文件「T0 结论」小节。

### Wave 1 — 数字出仓（不与 P5 冲突）

**T1 真实仓库靶子快照** `M`
- 选一个公开超大仓库作为固定靶子（README 已点名 Chromium 量级），产出**版本化 manifest**（仓库、commit sha、clone 参数、体积、ref/提交数）。
- 关键：快照必须**可复现**——manifest 之外还要有生成脚本，否则半年后无法重测。
- 与合成档位的关系：合成档位用于 CI（快、稳），真实靶子用于对外数字（可信）。

**T2 perf.yml 双层拆分** `S/M`
- 现状：单一 job 既跑 PR 子集（60 分钟）又跑全量（120 分钟），且整体 `continue-on-error: true`。
- 目标：PR 触发 → 轻量子集 + `--strict`（真门控）；周调度 → 全量 + 专用 runner（告警/归档）。
- 若 T0 结论为 C，则本任务改为"仅发布、不做 PR 门控"，需显式记录理由。

**T3 README 实测表 + release 对比** `S`
- README（中英双版）加一个「Performance」实测表：档位 × 操作矩阵，数字来自固定靶子。
- 发布流程挂 `compare-perf-runs.sh`：每个 release 产出与上一版的对比。

### Wave 2 — 体感优化（等 P5 收口）

**T4 冷启动四连优化** `M`
移植 C# 版已验证的四项：跳过冷启动探测、重活离 UI 线程、恢复 tab 不实例化全部仓库（第四项按现状核对后确定）。
- 基线先测：用 `perf-app-launch` 的 `app_launch/cold_*` 取现状数字，**不预设目标值**。
- 验收：优化后数字与基线同表对比，退化即回滚。

**T5 history cache 纵深** `M`
刚落地的 ref-fingerprint 磁盘缓存（805 行）从 log 扩展到 blame / reflog / 提交搜索；补失效策略：
- ref 指纹失效 + LRU + 磁盘占用上限
- 设置页手动清理入口（local-first：缓存路径对用户可见可控）
- 缓存命中率指标进 `perf_budget_report`

**T6 大文件 / 大 diff 专项** `M`
>10MB 单文件与 >5 万行 diff 的虚拟化与增量解码。现状是继"全量 status"之后最主要的卡顿源（status 已由迭代 05 的增量 status 解决）。
- 需先补档位基准（现有合成档位未覆盖"超大单文件"），否则无法证明改善。

**T7 增量 status 收尾** `S`
- `.git/index` 事件走 index-lane 定向（现 index 变更一律全量双道）
- watcher pathspec 对齐大小写不敏感文件系统（macOS 默认）的路径形态

## 验证

沿用 P4 四腿基线 + 新增性能腿：

| 腿 | 命令 / 判据 | 基线 |
|---|---|---|
| a | `cargo test --workspace --no-default-features --features gix` | 50 行 / 0 failed / **5,935** |
| b | `cargo test --workspace` | 51 / 0 / **6,020** |
| c | live clippy vs `clippy-baseline.txt` 逐字节 diff | 零 diff |
| d | `cargo test -p worktree-ui-gpui -- --list` 名单比对 | 零测试丢失 |
| e | `perf_budget_report --strict` | 无新增越界（依赖 T0） |

补充纪律：
- 平台双验：本地 Windows + CI Linux（perf 腿走 Linux 专用 runner）
- 性能数字**只认固定靶子**，共享 runner 数字仅作参考不进对外表

## 决策点

| # | 决策 | 触发时机 |
|---|---|---|
| **P6-D1** | T0 结论为 C（无自持 runner）时：接受噪声门控 / 只发布不门控 / 投入自持 runner | T0 完成后立即 |
| **P6-D2** | 真实靶子仓库选谁（体积 vs 可获得性 vs 代表性） | T1 开工前 |
| **P6-D3** | T4 允许的行为变更边界——"跳过冷启动探测"会改变启动时的信息完整度 | T4 开工前 |
| **P6-D4** | T5 缓存默认开关与磁盘上限（local-first 原则：默认开还是默认关） | T5 开工前 |

## 风险与缓解

| 风险 | 缓解 |
|---|---|
| P5 与 T4/T5/T6 同改 UI crate 引发冲突 | Wave 1 先行；Wave 2 等 P5 收口或文件级避让 |
| 共享 runner 噪声导致门控误报 | PR 门控只跑轻量子集 + 容忍带；对外数字走固定靶子 |
| 真实靶子仓库过大导致 CI 时间不可控 | 靶子仅用于周调度/对外发布，不进 PR 门控 |
| 优化引入行为回归 | 每优化项独立 PR；退化即回滚并记入台账 |

## T0 结论（2026-09-14 已核查）

**结论：B 的极端版，且发现更上游的阻塞。**

### 核查证据

| 项 | 结果 | 命令 |
|---|---|---|
| 仓库 CI variables | **`total_count: 0`** — `PERF_RUNNER` / `PERF_REAL_REPO_ROOT` 均未配置 | `gh api repos/dulingzhi/WorkTree/actions/variables` |
| Performance workflow 运行记录 | **0 次，从未运行过** | `gh run list --workflow perf.yml` |
| 8-27 之后全部 workflow 运行数 | **0** | `gh api .../actions/runs?created=>2026-08-27` |
| 最后一次 CI 运行 | 2026-08-26，Clippy `exit 101` + Rustfmt `exit 1` **失败**；Build / 测试各腿**全绿** | `gh run view 32939482357` |
| 仓库可见性 | **`private: true`**，0 stars，0 open issues | `gh api repos/dulingzhi/WorkTree` |
| 本地 vs 远端 | 本地 dev HEAD = `1c466575` **未推送**；远端 dev = `5884caaa` | `gh api .../commits/dev` |

### 三条推论

1. **性能门控从来没接过线，不是"形同虚设"而是"从未存在"**。perf.yml 只有 `schedule` + `workflow_dispatch` 两种触发，**没有 `pull_request` 触发器**——即 PR 门控在设计上就不存在。原方案 T2「双层拆分」实为**新增 PR 触发器**，工作量口径要改。
2. **整个 CI 已停摆 19 天**（8-26 之后零运行，期间远端 dev 有几十个提交）。在 CI 全红的仓库上加性能门控是本末倒置——**CI 健康度恢复是迭代 06 的真正前置**。
3. **仓库仍是 private**（0 stars / 0 issues，README 却在引导 star、Releases、Discord）。这直接冲击迭代 08「扩展生态 + 团队场景」与决策点 D10（商业模式）——开源社区与插件生态的前提是仓库公开。此结论需与用户确认是"尚未开源"还是"有意保持私有"。

### 排期影响

原 Wave 0→1→2 顺序调整为：

```
P0 修复 CI 健康度（新增，最高优先）→ Wave 1 数字出仓 → Wave 2 体感优化
```

- **P0**：修 Clippy / Rustfmt 失败 + 查明 19 天未运行的原因（配额？额度？设置？）+ 恢复绿灯。**不修完，性能数字无 CI 可信度。**
- T2 口径修正为「新增 `pull_request` 触发器 + 轻量子集 + strict」，而非"拆分现有 job"。
- T1 / T3（靶子快照、README 实测表）不依赖 CI，可与 P0 并行。
