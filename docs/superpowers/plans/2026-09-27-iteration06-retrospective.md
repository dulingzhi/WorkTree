# 迭代 06 性能专项 — 收尾复盘 / 成果流转

> 范围：GitComet 大型仓库打开速度与用户体感链路（冷启动 / history cache / 大 diff）。
> 状态：Wave 2（T4/T5/T6）实质性收口，T0–T3/T7 早前落地。本报告用于把成果流转给研发跟进 / 测试验收 / 上线复盘环节。

## 1. 目标回顾

迭代 06 要解决三件「有测量无优化」的事：性能门控从未真正生效、数字未出仓、冷启动与大 diff 两条用户体感最强链路无改善。最终交付：可证明的性能门控（CI PR 触发器 + 预算报告）、可观测的 history cache 命中率、大 diff 首窗口分页不卡顿。

## 2. 交付清单（T0–T7）

| 任务 | 状态 | 关键 commit / 结论 |
|---|---|---|
| T0 核查 strict 门控 | 完成 | perf.yml 原无 `pull_request` 触发器，门控从未生效 |
| T1 真实靶子快照 | 完成 | `rust-lang/rust` @ `a8a1e6fd`（340,056 commits / 1.4 GB），`0ef47dfd` `82374c82` |
| T2 perf.yml 双层拆分 | 完成 | 新增 `pull_request` 触发器 + PR 子集 / 周全量两个 job，`0cb41933` |
| T3 README 实测表 | 完成 | 中英双版实测表填数，`7764cb7c` |
| T4 冷启动四连 | 部分完成 / 收口 | 仅 ① 跳过探测被否→改为②离线程+乐观回填（`24fb8168`，−200~380ms）；③作废（冷启动与仓库数无关）；④审计后定夺**不做** |
| T5 history cache 纵深 | 完成 | 扩展域 blame/reflog/search + 护栏（256MiB/32MiB/7d TTL/真 LRU）+ Storage 设置页 + 命中率预算；`a8fd15f5` `767aef8c` `d9ecc752` `9076e56f` `1c487965` `b8ad0d4b` `ff3e47b4` |
| T6 大文件 / 大 diff | 完成 | 合成档位 >10MB 单文件 / >50k 行 + 虚拟化 + 增量解码 + 预算；`f3075f7a` |
| T7 增量 status 收尾 | 完成 | `dcec618d` |

## 3. 关键决策与结论

- **P6-D1（自托管 runner）**：决定用 GitHub Actions hosted runner（仓库已 public，免费），不投入自托管。代价：`real_repo/*` 组在 hosted runner 按缺失跳过，命中率预算需专用 runner + 真仓库才严格评估。
- **P6-D3（T4 行为边界）**：不跳过探测，改为「离线程 + 乐观回填」。唯一真同步探测 `git --version` 后台化，主视图零信息损失。
- **P6-D4（T5 默认开关 + 磁盘上限）**：默认开（即现状，无 enable 判定可改）+ 256 MiB 全局上限 + 32 MiB 每仓 + 7 天 TTL + 真 LRU + 设置页可清。**开关不做**（加开关=新增状态+持久化，且会让命中率指标因「用户关了」读 0%）。
- **T4 ④**：审计后归档。09-24 审计已覆盖全部启动期同步重活，唯一瓶颈已离线程；其余全异步，且冷启动与仓库数无关。额外优化属高成本、收益不可证伪的 speculative 工作。
- **T6 ②**：虚拟化（按页渲染、0 全量物化）与增量解码（`from_unified` 单次扫描构建行描述符、不复制原文、非逐窗口重解）已由现有分页架构满足，合成档位预算证毕，不另做流式解析重构。

## 4. 量化指标

| 指标 | 改前 | 改后 | 预算 |
|---|---|---|---|
| `app_launch/cold_*` first_paint | 1374ms（假数，新二进制首启+残留夹具） | 846ms（稳态） | ≤8000ms ✅ |
| `app_launch/cold_*` first_interactive | — | 979ms | ≤20000ms ✅ |
| git --version 离线程 | first_paint 824–883 / interactive 965–993 | 577–676 / 582–683 | — |
| paint→interactive 间隔 | ~140ms | ~7ms | — |
| history_cache.hit_rate.log | — | ≥20%（预算下限） | AtLeast 20 ✅ |
| history_cache.hit_rate.search | — | ≥3%（盈亏平衡下限） | AtLeast 3 ✅ |
| 大 diff 首窗口（>10MB 单文件） | — | ~123µs，rows_painted=200，full_text_materializations=0 | ✅ |
| 大 diff 首窗口（>50k 行纯新增） | — | ~4.88ms，rows_painted=200，full_text_materializations=0 | ✅ |

## 5. 遗留与风险

1. **严格门控评估依赖专用自托管 runner**（P6-D1）。hosted runner 不 checkout 巨型仓库，`real_repo/*` 组按缺失跳过；命中率预算在专用 runner 上观测分布后才可收紧阈值（当前为盈亏平衡硬下限，非目标值）。
2. **本机 Windows 跑 bench 环境坑**：criterion 把位置参数当 Regex → 所有 target 被调用 → `bench_git_ops` spawn git 辅助进程触发 os error 231（Stdio::piped stdin 已知 Windows 坑）整包 panic。修法：传 `--exact <完整 bench id>` 隔离跑单 bench。release 档首次编译偶发 `target/release` 指纹文件「拒绝访问 (os error 5)」，重试即过。
3. **T4 ④ / T6 ② 深层优化**：已审计归档 / 确认满足，不立项。

## 6. 成果流转建议

- **研发跟进**：命中率预算阈值当前为盈亏平衡硬下限（search 3% / log 20%），待专用 runner 长期观测分布后收紧到目标值；reflog 与 blame 命中率预算已于成果流转阶段提升为生效预算（reflog 经 `GitRepositoryLog::reflog_head`、blame 经 `GitRepositoryDiff::blame_file`，二者均接磁盘缓存且 `run_cache_repeat` 已驱动 cold+hot 读取）；四个域（log/search/reflog/blame）现已全部纳入 `STRUCTURAL_BUDGETS`，`DEFERRED` 项已清零。
- **测试验收**：冷启动（`app_launch/cold_*`）、大 diff（`diff_open_patch_large_tiers/*`）、命中率（`real_repo/monorepo_open_and_history_load_repeat`）基线条已进 `perf_budget_report`，可作为回归门；hosted CI 跑 PR 子集 + alerting，专用 runner 跑 strict。
- **上线复盘**：「性能可证明」核心数字来自固定靶子 `rust-lang/rust` 的本地 / 周调度测量；对外口径应明确 hosted runner 仅做 alerting、严格门控待专用 runner。
- **不在范围**：T-F（directory-diff）显式顺延，属 directory-diff 轨道，非性能迭代。
