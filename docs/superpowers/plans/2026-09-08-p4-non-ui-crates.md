# P4 非 UI crate + UI 尾巴 — 实施计划

- Spec：docs/superpowers/specs/2026-08-31-rust-codebase-refactor-design.md（P4 节 133-156 行权威；本计划为其执行化）
- 前置：P0/P1/P2/P3 已合入 dev 并推送（dev = 1381a05e）。P3 终审 PHASE APPROVED-WITH-NOTES。
- 分支策略：每任务一个分支，均自 dev 切出；按波次串行（SDD 禁并行实现者）。
- 执行模式：SDD（实现者 agent + 独立评审 agent + 台账 + 修复循环）。

## 继承约束（含 2026-09-07 用户授权的节奏变更）

- **验证节奏（新）**：**任务级四腿**——提交级便宜腿（`cargo check -p <触及包> --all-targets` + 定向包测试 + rustfmt，~1-3 分钟），任务边界完整四腿后评审。**例外：高风险批量语义变换保持提交级完整四腿**（T10 委托壳宏化、T8 reducer 分派化）。
- **四腿矩阵**：(a) `cargo test --workspace --no-default-features --features gix`（基线 49 行/0/5,935）；(b) `cargo test --workspace`（50/0/6,020）；(c) live clippy（touch lib.rs + JSON + stderr 含 Checking；p3t9fix-extract.py first-arrow 提取 + `tr -d '\r'`；与 clippy-baseline.txt 逐字节 diff）；(d) `cargo test -p worktree-ui-gpui -- --list` 名单比对（P4 触及非 UI crate 时须同步对比对应包的 --list 快照）。
- **W2**：具名 import；禁新增 `use xxx::*`（特许：迁移文件文件头 `use super::*;`）。
- **G1**：末次验证后零源码改动。**受众保持**：可见性升级须编译器证据，逐名记录消费方。
- **rustfmt**：`rustfmt --edition 2024 --config skip_children=true` 仅触及文件。
- **串行 cargo**；LNK2019 → `cargo clean -p <包>` 重试。**flake 注册表 5 件**（裁定不修；单目标全路径 --exact ×3 + 全量重跑复核）。
- **Shell**：Windows Git Bash；grep -E 不用 -P；管道会被 rtk hook 改写——文件重定向 + awk/grep 后处理。
- 提交：英文祈使句 + Co-Authored-By；仅任务文件；不 push。
- 编译基建：rust-lld 已生效（d2e9f8b5，腿周期约减半）；cmd wrapper 在 cargo 活动时禁编辑。

## P3 遗留登记册（P4 内消解点已标注）

1. T3 死 re-export 剪除、date_time.rs SystemTime —— **P3-cleanup 已先行处置**（refactor/p3-cleanup）。
2. `rows/conflict_resolver.rs`（3,307 renderer）改名消歧 —— **Wave 1 T3**。
3. components→panels `ContextMenuAction` 反向 import（context_menu_model.rs:2）—— **Wave 1 T4**（定叶子层纪律）。
4. UI 巨型文件残余池 —— **Wave 5**（范围裁定见下）。
5. 编译基建尾巴（debug=1、sccache）—— 独立基建轨，不占任务波次。

## 重锚定后的目标文件现态（2026-09-07 侦察快照）

| spec 行 | spec 锚点 | 现态 | 漂移 |
|---|---|---|---|
| 136 | reducer.rs reduce_inner :873（1,770 行/265 臂） | :873-2641（1,768 行/**262 臂**）；全文件 3,802 行；reducer/ 既有 7 域子模块 | ≈0 |
| 138 | effects 四重撞名 | reducer/effects.rs **5,579**（含 2 内联 tests mod）/ store/effects.rs 2,941 / msg/effect.rs 744 / store/tests/effects.rs 6,138；"第二同名测试文件"实为内联 mod | 口径勘误 |
| 140 | session.rs persist 8+ 对（:570-862） | **实测 10 对**（:570-:1464）+ 3 个仅 _to_path；内联测试恰 2,941 行（:1972-4912） | +2 对 |
| 142 | reducer/util.rs 双 API 族 | 文件 3,436 行，锚点需按符号重定位 | — |
| 145 | repo/mod.rs 委托壳 960 行/168 `_impl` | 1,252 行；**调用点 168 / 定义 167**，分布 19 子模块（remotes 40、porcelain 21、log 19、history 19 …） | 口径勘误 |
| 147 | services.rs 200 方法 trait | GitRepository :373-1671 共 **176 方法**；GitBackend :1672 起 56 | −24 |
| 149 | worktree-git 384 行 no-op | 精确吻合（lib.rs 44 + noop_backend.rs 340） | 0 |
| 151 | run_git 35 处 | **36 处精确名**（+90 处含变体名/39 文件）；is_git_shell_startup_failure **12 处**；set_fixed_mtime/hash_blob/make_executable 各 2 处 | +1/+1 |
| 153 | status_integration.rs 10,605/176/零子模块 | 精确吻合 | 0 |
| 155 | file_diff.rs 4,266 六边界 | 精确吻合；六段锚点已实测（line_text :52 / rows_anchors :498 / plan :545 / levenshtein :1235 / align :1389 / benchmark cfg 块） | 0 |

## 任务波次

### Wave 1 — 小件快赢 + 撞名消解

**T1 reducer/effects.rs → loaded_results.rs 改名**（spec:138）
- 单文件改名 + 引用点机械改指；撞名四重奏拆解的第一步（改名后 "effects" 剩余三处语义各自成立）。list 腿：零测试移动须空 diff。

**T2 worktree-git crate 归并**（spec:149；**裁定：归并**，2026-09-08）
- 384 行 no-op 后备归并入 worktree-core（`default_backend()` 单公开项）；Cargo.toml 依赖图收敛，workspace 成员 −1。

**T3 UI 撞名消歧三件套**（台账 #2 + 验收度量 `*_impl`/`*_helpers` 文件名清零）
- `rows/conflict_resolver.rs`（3,307 renderer）改名（建议 `conflict_renderer.rs`，消费方 2 公开项 + 引用点改指）；
- `panes/main/conflict_resolver_render.rs`（2,681）同组处置（三文件撞名：rows renderer / panes render / view 逻辑层壳）；
- `diff_view_helpers.rs`（443）随 diff 域改名归位；`render_impl.rs`（3,159）/ `core_impl.rs` / `actions_impl.rs` 文件名处置（改名或登记保留并说明）。

**T4 ContextMenuAction 类型下沉 components**（台账 #3；**裁定：类型下沉**，2026-09-08）
- `ContextMenuAction` 从 panels 下沉到 components（context_menu_model.rs 同层），panels 侧改具名 re-export 保消费方零改动；components 自此定为叶子层（不反向依赖 panels）。

### Wave 2 — test-support 收编（验收度量硬指标）

**T5 test-support crate 收编第一批**：精确名 `run_git(dir, args)` 36 处 → 1；`is_git_shell_startup_failure` 12 处 → 1；set_fixed_mtime/hash_blob/make_executable 各 2 处 → 1。
- 逐处签名比对（参数序/返回值/环境处理差异）；非同构者保留并记录。test-support crate 若不存在则新建（workspace 成员+Cargo.toml 接线）。

**T6 run_git 变体族第二批**：90 处含变体名（run_git_capture/output/with_env/at/command/expect_failure 等 30+ 变体）归并为 Options 结构或具名族。**按 T5 落地的实际痛感决定批次大小。**

**T7 status_integration.rs（10,605 行/176 测试）按域拆 tests/status/ 目录**
- 纯测试搬移；helper 移入共享模块；list 腿注意这是 integration tests（target 口径名单含 bin 测试——T14 评审口径登记 #14c 适用）。

### Wave 3 — worktree-state

**T8 session.rs**：persist 10 对收敛为 path-override 私函数 + 薄封装（机械收敛，展开等价逐对证明）；内联测试 2,941 行外迁 tests/。可分两个提交。

**T9 reducer/util.rs 双 API 家族统一**：`X`/`append_X` → 单一 `&mut impl EffectAccumulator` 入口（spec:142）。调用点改指量先实测再定提交切分。

**T10 reduce_inner 262 臂按 reducer/ 既有 7 域分派化**（spec:136）——**高风险件，提交级完整四腿**。按域分批提取（每域一提交），臂序保持 + 守卫完备性逐域证明。

**T11 reducer/effects.rs（5,579）拆分**：改名后的域拆分 + 2 个内联 tests mod 外迁。

### Wave 4 — worktree-git-gix + worktree-core

**T12 repo/mod.rs 委托壳宏化 + 167 `_impl` 改回本名**（spec:145）——**高风险件，提交级完整四腿**。`delegate!` 声明宏或 `#[trace_op]` 属性宏（选型先试 3 个子模块做样板）；按 19 子模块分批（remotes 40 最大）。168 调用点与 167 定义的一一对应表先行建档。

**T13 services.rs 176 方法 trait 拆域**（**裁定：按 T12 落地痛感后定**，2026-09-08——届时以可读性证据呈主会话裁定，不预排）：log/history/remotes/status/diff 分组与 repo/ 子模块分布同构。

**T14 file_diff.rs（4,266）六边界拆**（spec:155）：line_text / rows_anchors / plan / levenshtein / align / benchmark；物理序即切分序，:1354 prepare_replacement_lines 为天然接缝。

### Wave 5 — UI 巨型文件残余池（**裁定：只收挂号件**，2026-09-08）

- 本阶段收 P3 评审挂号的 7 件：popover/host.rs 2,788、diff_search.rs 2,726、context_menu.rs 2,799、rows/diff/rows.rs 2,419、markdown_preview.rs(rows) 2,239、resolved_output_syntax.rs 2,195、bootstrap.rs 2,148。
- 未挂号大件（view/mod.rs 4,787、terminal_panel 4,578、sidebar×2、diff_cache、theme、app 等 45 个）**另立 P5**。验收度量 ">2,000 行 <10 个" 在 P4 后仍不达标属预期，P5 收口。

## Self-Review 结论（草稿）

1. spec P4 四域全覆盖（T1/T5-T14），P3 遗留 4 项挂到 T3/T4/Wave 5；验收度量硬指标（run_git 36→1、`*_impl`/`_impl` 方法名清零、glob 根零残留维持）各有归属。
2. 风险序：Wave 1/2 机械 → Wave 3 中件（T10 高风险）→ Wave 4 大件（T12 最高风险）→ Wave 5 范围待定。
3. 节奏纪律：T10/T12 提交级四腿，其余任务级。
4. 开放裁定（2026-09-08 已全部闭合）：① T4 类型下沉 components；② T2 归并；③ Wave 5 只收挂号件（未挂号大件另立 P5）；④ T13 按痛感后定。

## 执行门槛

本计划经用户批准后，自 dev 切 `refactor/p4-*` 分支按 Wave 1→5 串行 SDD 执行。
