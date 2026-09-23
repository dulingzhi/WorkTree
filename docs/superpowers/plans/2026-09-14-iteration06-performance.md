# 迭代 06 — v0.7.0「性能可证明」实施计划

- 日期：2026-09-14（分支 `dev`，HEAD = `1c466575`）
- 上位方案：[docs/roadmap-long-term.md](../../roadmap-long-term.md) §3 迭代 06
- 前置：roadmap 迭代 01–05 全部完成；重构 P0–P4 已合入 dev，**P5 进行中**（本迭代与 P5 并行，见「并行约束」）
- 执行模式：SDD（实现者 agent + 独立评审 agent + 台账 + 修复循环），沿用 P4 纪律
- 状态：**进行中**（Wave 1 已落地，Wave 2 未开工；2026-09-21 复核后更新，见文末「2026-09-21 复核」）

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

## 并行约束（与 P5 的边界）—— 2026-09-22 作废

**P5「split remaining」已于 2026-09-22 取消，分支 `p5-split-remaining` 已删除。** 原「T4/T5/T6 会与 P5 抢文件」的前提不再成立，Wave 2 三大任务（冷启动 / history cache 纵深 / 大 diff 专项）**全面解锁**，可自由改动 `view/`、`git-gix/repo/`，无需再等收口或做文件级避让。

- T1/T2/T3（CI 与文档）已落地，无影响。
- 建议顺序（修订）：T0/T1/T2/T3/T7 已完成 → **T6（先补档位基准，再做大 diff 虚拟化）→ T4 冷启动 → T5 history cache 纵深**。三者相互独立，T6 的档位基准是 T6 自身测量的前置。
- 取舍记录：取消 P5 意味着保留当前模块结构（不再做剩余拆分降耦）。若日后耦合成为瓶颈，那是独立决策，不属于迭代 06。

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
- ~~`.git/index` 事件走 index-lane 定向（现 index 变更一律全量双道）~~ → **2026-09-21 复核：不做，见下**
- watcher pathspec 对齐大小写不敏感文件系统（macOS 默认）的路径形态 → **2026-09-21 已做**

#### 2026-09-21 复核：子项 1 证伪，子项 2 落地

**子项 1「`.git/index` 事件走 index-lane 定向」——不做。** 三条证据：

1. **动作路径已经走了定向**：app 自发的 stage/unstage 由 `repo_externally_changed` 之外的
   `repo_action_finished` 处理，成功时把 `RepoPathList` 作为 `incremental` 传给
   `dispatch_repo_change`，走 `LoadStatusForPaths`。「index 变更一律全量」不成立。
2. **剩下的口子是外部 `.git/index` 事件，而它没有路径源**：`classify_repo_event`
   经 `is_git_index_path` 只产出 `RepoExternalChange::Index`，不带任何 worktree 路径。
   没有路径就无法构造 pathspec，`LoadStatusForPaths` 无从发出。
3. **原设想的替代改法（只刷 staged lane）被回归测试明确禁止**：
   `crates/worktree-state/src/store/tests/external_and_history.rs`
   的 `external_index_change_must_not_refresh_only_the_staged_lane` 记录了它曾经的行为——
   index 变更只发 `[LoadStagedStatus]`，结果一个被移动的文件在 unstaged 区残留为陈旧条目。
   `set_staged_status` 只写 `staged_status`，`worktree_status_entries()` 命中 `Ready` 就直接返回旧值。

   想从「上一份 settled 快照的 staged ∪ unstaged 路径」反推候选集也不成立：一个此前完全干净、
   被 `git update-index --add` 直接写进索引的文件不在任何一个旧集合里，定向刷新会漏掉它。
   引入一次额外的索引读取来补这个洞，已经不是 S 号工作量。

   结论：外部 index 事件保持全量 `LoadStatus`。它本来就是**一条** `git status` 同时覆盖两条 lane，
   计划里「全量双道」的说法高估了成本。

**子项 2「watcher pathspec 对齐大小写不敏感文件系统」——已做。** 真实缺陷在
`repo_monitor.rs` 的 `relativize_paths`：它用 `Path::strip_prefix` 做**字节**前缀比较，
而 canonicalize 过的 `workdir` 与 watcher 报上来的路径在大小写不敏感的文件系统
（macOS APFS/HFS+ 默认、Windows NTFS）上可以只差大小写，在 Windows 上还可以差
`\\?\` verbatim 前缀。任一不匹配就让整条路径集被丢弃 → 定向刷新静默退化成全量 worktree 走查。
新增 `strip_workdir_prefix`：先走字节快路径，失败后退化为逐组件比较，按平台折叠大小写
（仅 macOS/Windows；Linux 上 `/REPO/a` 与 `/repo/a` 是两个文件，不能折叠）并忽略
verbatim 前缀差异。仍然对真正落在 workdir 之外的路径返回 `None`——那必须退回粗扫。
4 个单测锁定：字节匹配、越界拒绝（含 `repo` vs `repo2` 的前缀陷阱）、大小写折叠、verbatim 前缀。

> 注：大小写折叠只在 macOS/Windows 编译，CI 的 Linux 腿跑不到这一支；本地 Windows 已验证。

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
| **P6-D3** | T4 允许的行为变更边界——"跳过冷启动探测"会改变启动时的信息完整度 | **2026-09-23 已闭合**：不跳过，改「离线程 + 乐观回填」（见下） |
| **P6-D4** | T5 缓存默认开关与磁盘上限（local-first 原则：默认开还是默认关） | **2026-09-23 已闭合**：默认开 + 256 MiB + 7 天 TTL + 设置页可清（见下） |

## 风险与缓解

| 风险 | 缓解 |
|---|---|
| ~~P5 与 T4/T5/T6 同改 UI crate 引发冲突~~ | **P5 已取消（分支删除），此风险消除**；Wave 2 可直接开工 |
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

## T2 实施进展（2026-09-15）

已落地 `pull_request` 触发器（迭代 06 T2 口径修正：原 perf.yml 只有 `schedule`+`workflow_dispatch`，无 PR 门控）：
- `on` 新增 `pull_request: types:[opened,synchronize,reopened]`，带 `paths` 过滤（`crates/**`、`scripts/**`、`Cargo.toml`、`Cargo.lock`、`.github/workflows/perf.yml`），doc-only PR 跳过以省 runner 分钟。
- `performance-budgets` job 的 `if` 扩展为 `pull_request || (workflow_dispatch && suite==pr-subset)`；job 名改为「PR subset + manual」。
- 该 job 的预算报告步改为条件式：配了 `PERF_RUNNER` 则 `--strict`（真门控），否则 `--skip-missing`（告警/容忍带）。`continue-on-error: true` 保留（PR 容忍带；严格数字走 nightly 全量专用 runner）。
- 未推、未开 CI。生效前提：P0 开启 Actions（`gh api -X PUT repos/dulingzhi/WorkTree/actions/permissions -f enabled=true`）+ 配置 `PERF_RUNNER`/`PERF_REAL_REPO_ROOT` 变量后，PR 子集才会跑 `--strict`。
- 验证：YAML 结构按既有 `performance-budgets-full` 同名条件式对齐（本机无 pyyaml，未做机器解析，人工核对缩进一致）。

## T1 实施进展（2026-09-15）

靶子快照已落地并复现：
- 固定靶子 = `rust-lang/rust` @ `main`，钉死 commit `a8a1e6fd9df2e094d6f09c0d57991508680acc1c`，**340,056 commits**、163 tags、磁盘 **1.4 GB**（之前 `--ref master` 失败，rust 默认分支已改 `main`）。
- `scripts/generate-perf-target-manifest.sh`（v1，仓库无关、可复现）：`git clone --no-single-branch` 到 `tmp/perf-real-repo/<name>/source.git`，量 ref/commit 数 + `count-objects` 体积，写出 4 个场景 `metadata.json`（`source:"../source.git"` 相对解析，故快照可整体 relocate）；同时提交版本化 `benches/performance/real_repo_target.json`。
- `tmp/perf-real-repo/` 已 git-ignore，只提交小的 manifest，多 GB 克隆留本地。
- 修复（已进脚本）：ref 解析容错（`origin/<ref>` / `refs/tags/<ref>` 回退）；`branch_count` glob 改 `refs/remotes/origin/*`；**冲突场景需要 `conflict_merge_ref` 作为 `source.git` 的本地分支**（否则 bench clone 只带 `refs/heads/*`，worktree 里没有 `origin/stable`，`git merge origin/stable` 报 "not something we can merge"）。脚本现自动 `git branch <ref> origin/<ref>`。

## T3 实施进展（2026-09-15 落地；2026-09-21 补完后半）

### 第 1 件：README 中英双版实测表（已完成，commit `7764cb7c`）

README.md / README.zh-CN.md 均新增 `## Performance` / `## 性能` 节，4 场景表已填实测数字：

| 场景 | 实测 mean |
| --- | --- |
| `monorepo_open_and_history_load` | 8.87 s |
| `deep_history_open_and_scroll` | 532 ms |
| `mid_merge_conflict_list_and_open` | 15.30 s |
| `large_file_diff_open` | 587 ms |

- 已修正：原 `merge_ref` 写成 `origin/stable` 导致 harness 解析失败 panic；改裸分支名 `stable`，并在 `source.git` 里建本地分支。
- 本地 Windows 跑 bench 必须用 native 路径：`WORKTREE_PERF_REAL_REPO_ROOT="$(pwd -W)/tmp/perf-real-repo/rust"`，Git-Bash 的 `/mnt/d/...` 虚拟路径 Rust bench 二进制不识别。

### 第 2 件：release 性能对比（2026-09-21 已接线，未经 CI 实跑）

`release-manual-main.yml` 新增 `perf_comparison` job，把 `scripts/compare-perf-runs.sh` 接进发布流程：

1. 在自托管 perf runner 上跑 `scripts/archive-perf-run.sh --run-id release-<version> --profile full --strict`，产出本版基线。
2. 把 `benchmark-metrics.jsonl` / `benchmark-metrics.log` / `budget-report.md` / `metadata.txt` 打包成 `perf-record.tar.gz`（**不含 criterion/ 目录**，否则资产过大）。
3. 用 `gh release list` 找上一个 release，`gh release download --pattern perf-record.tar.gz` 拿它的资产，解包成扁平 run 目录当 base。
4. `scripts/compare-perf-runs.sh --sort regression <base> <candidate>` 产出 `perf-comparison-<内容>.md`。
5. `gh release upload` 把 `perf-record.tar.gz` + `perf-comparison.md` 挂到本次 release。

三条设计约束（直接针对账户 Actions 额度耗尽这一现状）：

- `if: vars.PERF_RUNNER != ''` → 只在自托管 runner 上跑，**不消耗 hosted 分钟**
- `continue-on-error: true` → 性能对比失败只告警，不阻断、不回滚发布
- `needs: [validate, create_release]`，与 `build_and_upload` 并行，不拖长发布链路

首次发布若无上一版资产，会写成「本版成为首个基线」的说明而不是报错。

**未验证**：CI 自 2026-09-16 起因额度问题所有 job 无法启动，本 job 从未实跑。已做的验证只有：
- YAML 结构解析通过（job/steps/if/runs-on 均按预期）
- 冒烟测试过 tar 打包 → 解包 → `compare-perf-runs.sh` 全链路：用两个构造的假存档跑出了 `reg 25.00%` 的对比表，确认扁平 run 目录布局能被脚本正确识别

---

## 2026-09-21 复核：看板与文档落后于代码

本次对账发现三件事，已同步到事项看板：

### 1. T0/T1/T2/T3 实际已落地，看板仍标「未开始」

| 事项 | commit | 落地内容 |
| --- | --- | --- |
| T1 | `0ef47dfd`、`82374c82` | `scripts/generate-perf-target-manifest.sh` + `benches/performance/real_repo_target.json`（rust-lang/rust @ `a8a1e6fd`，340,056 commits / 1.4 GB） |
| T2 | `0cb41933` | perf.yml 新增 `pull_request` 触发器 + 拆成 PR 子集 / 周全量两个 job |
| T3-1 | `7764cb7c` | README 中英双版实测表填数 |

### 2. P0「CI 停摆」的根因判断已作废，真实根因是额度耗尽

- `gh api repos/dulingzhi/WorkTree/actions/permissions` → `{"enabled":true,"allowed_actions":"all"}` —— **Actions 开关已开**，原「需授权开启」这条待办不存在了。
- 但 `actions/runs` 显示 **53 次运行 conclusion 全部 failure，0 次 success**。
- 2026-09-16 04:18 的运行 job 还有完整 steps；**12:18 起所有 job `steps=0`、1~3 秒内 failure**，三平台一致，`runner_name` 为空 → hosted runner 根本没起来。
- HEAD `acf6dbab` 的 commit message 本人写明「CI is down from billing」。
- 8-26 那次的 Clippy/Rustfmt 失败早已被本地 `cargo fmt --all` 与 clippy 验证修复，那两个失败现在已不存在。

修法三选一（需拍板）：等账单周期重置 / 加支付方式提额 / **转自托管 runner**（顺带解决 `PERF_RUNNER` 未配的问题）。

### 3. strict 门控至今一次都没生效过

`actions/variables` `total_count: 0` → `PERF_RUNNER`、`PERF_REAL_REPO_ROOT` 均未配置；`actions/runners` `total_count: 0`。因此 perf.yml 无论哪条路径都走 `--skip-missing` + `continue-on-error: true`。

**T2 只能算「结构完成」，不能算「门控生效」。** 在配好自托管 perf runner 之前，「性能可证明」里的「证明」二字不成立。

### 剩余工作

Wave 2 全部未开工：T4 冷启动四连、T5 history cache 纵深、T6 大文件/大 diff、T7 增量 status 收尾（T7 的 index-lane 在 `crates/` 里 grep 不到任何痕迹，确认未动）。

## 2026-09-22 复核：strict 门控在免费 hosted runner 上落地（T0 收口）

仓库已转 **public**（见 P0 复盘）→ GitHub-hosted runner 对公开仓库免费，原「额度耗尽」阻塞消失。借此把严格门控真正接上线，不再依赖用户手动去 Settings 点变量。

### 改动（commit `b61b24f3`，hosted 报告参数在 follow-up 修正 commit 中改为 `--strict --skip-missing --skip-prefix real_repo/`）

1. **`perf_budget_report` 新增 `--skip-prefix`**（可重复）。`run_report` 在评估前按 `label`（timing）/ `bench`（structural）前缀过滤，被匹配的预算**整体跳过**（不计入 Skipped/Alert，也不计入告警门禁）。单测 `parse_cli_args_collects_repeatable_skip_prefixes` 覆盖解析。
2. **`scripts/perf-bench-list.sh`**：抽出 PR subset 的合成 bench id 清单（与 perf.yml 中 PR subset 完全一致），供 hosted 回退路径复用。刻意排除 `real_repo/*`（需要 checked-out 巨型仓库）与 `app_launch/*` / `idle/*` harness（需要 compositor）。
3. **`perf.yml` 的 `performance-budgets-full` job**：
   - bench 步骤拆成两条：`vars.PERF_RUNNER == ''` 时跑合成子集（`perf-bench-list.sh`，`continue-on-error: true` 防单 bench 抖动压垮报告）；`!= ''` 时跑全量（含 real_repo）。
   - 预算报告步骤：专用 runner 仍 `--strict`（全量，含 real_repo）；hosted 回退从 `--skip-missing` **改为 `--strict --skip-missing --skip-prefix real_repo/`**。
     - 关键点：预算表共 230 条（217 timing + 162 structural，去重后），而 hosted 回退只跑 `perf-bench-list.sh` 的 **44 个**合成 bench。若只用 `--strict --skip-prefix real_repo/`（不设 `--skip-missing`），那 186 条没被这 44 个 bench 覆盖的非 `real_repo/` 预算会因「数据缺失 → Alert」导致严格门控**每次都红（exit 2）**——这是初版落地的逻辑漏洞（已修）。
     - `--skip-missing` 与 `--strict` 正交：`--skip-missing` 把「数据缺失」从 Alert 降级为 Skipped（不计入告警门禁），`--strict` 仍把「有数据但超阈值」判 Alert 并 `exit 2`。二者叠加 = **只严格约束本 runner 实际跑到的那 44 个合成 bench；其余（real_repo/*、app_launch/*、idle/* 以及未纳入 bench-list 的合成预算）按缺失跳过**，门控不再假红。

### 现在的门控语义

| 路径 | runs-on | 跑的 bench | 报告模式 | 门禁 |
|---|---|---|---|---|
| `performance-budgets`（PR subset） | ubuntu-22.04 | 合成子集 | `--skip-missing`（轻量信号） | 容忍，不阻断 |
| `performance-budgets-full` / 未配 PERF_RUNNER | ubuntu-22.04（免费） | 合成子集（`perf-bench-list.sh` 的 44 个） | `--strict --skip-missing --skip-prefix real_repo/` | **真严格，但只针对本 runner 实际跑到的 44 个合成 bench；其余（real_repo/app_launch/idle 及未纳入 bench-list 的合成预算）按缺失跳过** |
| `performance-budgets-full` / 配了 PERF_RUNNER+PERF_REAL_REPO_ROOT | 专用 runner | 全量含 real_repo | `--strict` | **真严格，全预算** |

### 结论更新（推翻原「B 极端版 / 门控从未生效」）

- 「strict 从未生效」**已不成立**：免费 hosted runner 上，本 runner 实际跑到的 44 个合成预算的严格门控现在每周真实跑（超阈值会真 `exit 2`）。
- `PERF_RUNNER` / `PERF_REAL_REPO_ROOT` 两个仓库变量**仍可配可不配**：不配 → 仅 44 个合成 bench 真严格 + 其余按缺失跳过；配了 → real_repo 组也纳入严格门控（需要专用 runner 能 checkout 巨型仓库）。
- **残留风险**：hosted 共享 runner 数字有噪声，44 个合成预算的夜间严格门控偶发误报；`real_repo/*` 组（对外的「性能可证明」核心数字）仍只在专用 runner 上才有。要消除这两项，仍需接入自托管 perf runner 并配置 `PERF_REAL_REPO_ROOT`——但那已从「门控前提」降级为「数字精度增强」。
- **2026-09-22 修正（follow-up commit）**：初版 hosted 回退用了 `--strict --skip-prefix real_repo/`（漏了 `--skip-missing`），会使 186 条未被 44 bench 覆盖的非 real_repo 预算全部 Alert → 门控假红。已改为 `--strict --skip-missing --skip-prefix real_repo/`。本地空 criterion 根验证：旧参数 `exit=2` / 977 行 ALERT；新参数 `exit=0` / 977 行 SKIP。CI 尚未重跑（hosted runner）。
- 验证：`cargo test -p worktree-ui-gpui --bin perf_budget_report` 单测通过（`--skip-prefix` 解析 + 既有预算评估用例）；YAML 结构与既有条件式对齐。

## 2026-09-22（2）P5 取消 · Wave 2 重规划

P5 取消后，迭代 06 只剩 Wave 2（T4/T5/T6）未开工。原「剩余工作」（含 T7 未动）已过时：T7（增量 status 收尾）已于 `dcec618d` 落地（大小写不敏感文件系统路径对齐），故 T7 已从剩余清单移除。

### 修订后任务清单（Wave 2）

| 任务 | 量级 | 前置 | 范围 / 验收 |
|---|---|---|---|
| **T6 档位基准 + 大 diff 专项** | M | 无（立即开工） | ① 补合成档位：>10MB 单文件 diff、>5万行 diff（现有合成档位未覆盖，否则无法证明改善）；进 `benches/performance/` 夹具 + `budgets/` 预算规格，纯新增无冲突。② 对这两类做虚拟化 + 增量解码。验收：`perf_budget_report` 列出新预算且能产出 criterion 产物；大 diff 打开不再卡顿（与基线同表对比） |
| **T4 冷启动四连优化** | M | P6-D3 决策 | 移植 C# 已验证四项：跳过冷启动探测、重活离 UI 线程、恢复 tab 不实例化全部仓库（第四项审计后定）。基线先测 `perf-app-launch` 的 `app_launch/cold_*`，不预设目标。验收：优化后数字与基线同表对比，退化即回滚 |
| **T5 history cache 纵深** | M | P6-D4 决策 | 把已落地的 ref-fingerprint 磁盘缓存（log 域）扩展到 blame / reflog / 提交搜索；失效策略（ref 指纹 + LRU + 磁盘上限）；设置页手动清理入口（local-first：路径对用户可见可控）；缓存命中率指标进 `perf_budget_report`。验收：扩展域命中率可测、清理入口可用、越界受预算约束 |

### 待拍板的决策（原决策点更新）

- **P6-D1（自托管 runner）—— 2026-09-22 已定：用 GitHub Actions（仓库已 public，hosted runner 免费）**：不投入自托管 runner。real_repo/* 组严格门控顺延（hosted runner 不 checkout 巨型仓库，该组按缺失跳过）；T4/T5 基线靠本地 Windows 跑 `perf-app-launch` / 缓存命中率观测，或顺延到日后有专用 runner。对外「性能可证明」核心数字仍来自固定靶子 `rust-lang/rust` 的本地/周调度测量。
- **P6-D3（T4 行为边界）—— 2026-09-23 已闭合：不跳过探测，改为「离线程 + 乐观回填」。**

  审计推翻了原措辞。启动期真正算「探测」的只有 `git --version`（`worktree-core/src/process.rs:334-402`），
  且它是 **reducer 硬闸门**（`store/reducer.rs:891-893`）：git 不可用时 `Msg::RestoreSession` 被
  **静默丢弃**，用户看到的是空工作区而非「Git 未安装」错误条——这才是真实且最严重的信息损失。

  同时，它的开销不在「探测」本身而在**跑在 UI 线程**：`AppState::default()` 经 `store/mod.rs:314`
  （`app.rs:611` 的 `open_window` 闭包内）触发一次，`view/mod.rs:2219` 每次窗口激活再同步触发一次。
  因此 ①跳过探测 与 ②重活离 UI 线程 **合并为同一个改动**：

  1. `git --version` 改后台执行 + 结果缓存；
  2. `AppState::default()` 乐观当可用，后台探测完成后用 `Msg::SetGitRuntimeState`（`reducer.rs:956-959`）回填；
  3. git 真缺失时走**已有的** `DeferredRepoBootstrap`（`view/view_mode.rs:108-115`）
     → `resume_after_git_runtime_recovery`（`view/state_apply.rs:176-177`），有转圈态兜底。

  代价仅「设置页 Git 版本串短暂为空」，主视图零信息损失。

- **P6-D4（T5 默认开关 + 磁盘上限）—— 2026-09-23 已闭合：默认开 + 256 MiB 全局上限 + 7 天 TTL + 设置页可清。**

  审计发现「默认开」**就是现状**：`log.rs:1707` / `log.rs:1882` 无条件调用缓存，全仓不存在任何
  enable 判定（无 feature flag、无 env、无 setting 字段）。改成「默认关」反而是新增工作量 + 行为回退，
  且会让 T5 交付物「缓存命中率进 `perf_budget_report`」恒为 0、指标失去意义。

  真正缺的是**护栏**：无字节上限、无 TTL、LRU 是假的（`prune_old_generations` 按 mtime 排序但
  `load_log_page` 只读不刷 mtime，实为 FIFO）、无生产可达的清理入口（`clear_repo_cache` 仅 `#[cfg(test)]`）。

  采纳值（对齐仓库既有先例 `view/panes/main/diff_cache/image_cache.rs:8-11`）：

  | 项 | 值 | 依据 |
  |---|---|---|
  | 全局字节上限 | 256 MiB | 与 `IMAGE_DIFF_CACHE_MAX_TOTAL_BYTES` 一致；实测 254 B/commit，≈2000 个满快照 |
  | TTL | 7 天 | 与 `IMAGE_DIFF_CACHE_MAX_AGE` 一致；实测 7 天前文件仍在盘（当前永不过期） |
  | 每仓库子上限 | 32 MiB | 新增，防 blame（单条目可达 ~2 MB）一家吃满 |
  | 真 LRU | 命中时刷 mtime | 修 FIFO 误删热快照的问题 |

  另需：新增设置页 **Storage** 分类（显示缓存路径 / 体积 / 条目数 + 开关 + Clear 按钮）；
  缓存根目录建议从 `std::env::temp_dir()` 迁到 `app_data_dir()`（`session.rs:1972`），
  因 Linux `temp_dir` 可能是 tmpfs 且被系统清理，与 local-first「用户可见可控」相悖。
  跨 crate 调用走 `worktree_core` 中转（`worktree-git-gix` 是 optional 依赖，先例 `process.rs:273`）。

- **执行顺序（2026-09-23 定）**：T4 先修 `perf-app-launch` harness 语义并取基线（当前 5/20-repo 三个 case
  必然失败、且从未取过基线），再做 D3 的离线程改动；T5 先做护栏 + 命中率接线，**再**按
  reflog(S) → commit-search(M) → blame(M–L，最后且单独 PR) 的顺序扩展。

### 执行顺序建议

T6 档位基准（纯新增、零冲突、解锁测量）→ T4（用户体感最强，但基线依赖能跑 `perf-app-launch`，见 D1/D3）→ T5（深度最大）。三者可并行开发，但每项独立 PR、退化即回滚（沿用 P4 纪律）。

### 不在迭代 06 范围

- **T-F（directory-diff）**：计划显式顺延（deferred），属 directory-diff 轨道，非性能迭代。P5 冻结 GitRepositoryDiff trait 的约束随 P5 取消而失效；如想重启 T-F 另行规划。
- P5 自身不再有任务。
