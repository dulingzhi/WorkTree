# WorkTree 全库精简重构方案（分层蚕食）

- 日期：2026-08-31
- 状态：待评审
- 范围：crates/ 下全部 7 个 workspace 成员（约 59 万行 Rust，643 个 .rs 文件）
- 基线：dev 分支 HEAD（457c6277），`cargo test --workspace --features benchmarks --no-run` 通过

## 1. 背景与证据

crate 间分层健康（core → git 抽象 → gix 后端 → state → ui → bin），代码卫生极好（真实
TODO 仅 5 处、`#[allow(dead_code)]` 仅 22 处）。问题集中在 `worktree-ui-gpui/src/view/`
（316 文件 / 34.6 万行），属于"缺抽象"型代码库而非"烂尾"型——重构以**补结构、消样板**
为主，不是清烂账。

四个根因（其余问题均为其衍生）：

| # | 根因 | 代表证据 |
|---|---|---|
| R1 | 两个 god struct 的 impl 无边界散布 | `MainPaneView`（约 350 字段）定义在 `panes/main/helpers.rs:2999`，impl 散布 27 处、横跨 `panes/main/` + `panels/main/` + `rows/` 三棵树、合计 2 万+ 行；`WorkTreeView`（145 字段）定义在 `view/mod_helpers.rs:5728` 而 impl 主体在 `view/mod.rs:641`（3,310 行），另 11 个文件持有碎片 |
| R2 | glob 导入放大器 | 79% 的 view 文件首行 `use super::*`，最坏 5 层链：`panels/popover/context_menu/branch.rs` → … → `view/mod.rs:251` 的 `use mod_helpers::*`，把 5,877 行符号注入每个文件 |
| R3 | "杂物抽屉"文件家族 | `view/mod_helpers.rs`（9 个互不相干的域）、`panes/main/core_impl.rs`（名不副实：设置+语法+滚动+换行）、`panes/main/helpers.rs`（8 个域）、共 10 个 `*_helpers/*_impl/*_actions` 文件 |
| R4 | 样板手写不收敛 | gix 侧 960 行委托壳 + 168 个 `_impl` 后缀（根因：`worktree-core/services.rs:373` 约 200 方法的巨型 trait）；`run_git` 全库定义 35 次；`session.rs` 8+ 对 `persist_X/_to_path`；settings 35 个同构 setter + 19 个同构 option_rows；6 对 `setter/_and_persist` + 4 个 mergetool 变体；4 个 pane 逐字重复的 popover 委托三件套 |

有利条件：巨型文件中 40-95% 常为内联测试（`rows/diff_text/syntax.rs` 9,899 行中
9,450 行是测试）；多个纯逻辑模块几乎不依赖 GPUI（`view/conflict_resolver.rs`、
`view/markdown_preview.rs`）放错了层；行为测试覆盖厚（`panels/tests/` 5.4 万行、
rows 层 824 测试、冲突域 245 测试）。

## 2. 目标与非目标

目标（按用户确认的优先级排序——按性价比兼顾以下三者）：

1. 可维护性：大文件拆为职责单一的模块，理顺 panels/panes 结构，消灭杂物抽屉命名。
2. 代码量：去重、宏化、表驱动，净删样板。
3. 编译/迭代速度：主要通过"改一个域不再惊动 2 万行文件"的局部性获益；实质的
   crate 级并行收益列为可选尾巴（P4.5）。

非目标：不改变任何用户可见行为；不重写 Git/合并算法核心；不动 CI/发布流水线；
不翻旧账做全量 glob 显式化。

## 3. 已确认的决策

| 决策 | 结论 |
|---|---|
| 风险边界 | 允许局部重写，但仅限测试覆盖厚的热点（如 `sync_conflict_resolver`），且每处独立成 PR |
| 执行路径 | A：分层蚕食（叶子 → 主干，五阶段，每步独立可合并可验证） |
| glob 策略 | 掐根不翻旧账：消灭注入根与非叶子 glob 再导出；保留存量 `use super::*` 文化；新模块必须显式导入 |
| crate 拆分 | 列为可选尾巴（P4.5），做完 P0-P4 后按实测编译痛感决定 |

## 4. 总体原则

- **每阶段 = 一串独立可合并的小 PR**。每步验收（CI 三件套 + 按需补充）：
  - `cargo clippy --workspace --no-default-features --features gix -- -D warnings`
  - `cargo test --workspace --no-default-features --features gix`
  - `cargo build -p worktree --features ui-gpui,gix`
  - 涉及 benchmark 符号时：`cargo test --workspace --features benchmarks --no-run`
- **纯搬移优先于改写**：先让文件变小、命名变准，再做同文件内的函数级重构。
- **兼容壳策略**：搬移符号时在原路径留 `pub use` 再导出（如 869 处引用的
  `conflict_resolver::` 前缀），消费方零改动；待该域消费方自然更新后再删壳。
- **平台双验**：本地 Windows + CI Linux（存在 `panels/mod.rs` linux 变体、
  `linux_desktop_integration.rs`、终端进程组 unix/windows 双实现）。
- **可见性规范**（随搬移逐步落实，不做专项）：跨模块用 `pub(crate)`，模块内用
  `pub(super)`；禁止新增 `pub(in super::super::super)` —— 深路径是模块层级错误的信号。
- **测试安置政策**：内联 `mod tests` 超过约 1,500 行即外迁至同级 `tests/` 目录并按
  域分文件（既有范本：`conflict_resolver/tests/`、`panels/popover/tests/`）。

## 5. 阶段计划

### P0 快赢（3-5 个 PR，零行为变化）

| 动作 | 位置 | 效果 |
|---|---|---|
| 内联测试外迁 | `rows/diff_text/syntax.rs`（测试占 9,450/9,899 行） | → `syntax/tests/`，主文件剩 ~450 行 |
| 拆出 markdown 预览渲染器 | `rows/history.rs:667-2890`（约 2,200 行自成体系） | 文件 5,134 → ~2,900 |
| 拆出第二个视图 | `rows/sidebar.rs:2646-3125`（`impl DetailsPaneView`） | 消除"一文件两视图" |
| 删测试专用拷贝 | `rows/conflict_resolver.rs:2973`（`whitespace_visible_text_and_highlights`）改调 `rows/diff_text.rs:430` 版本 | 净删重复 |
| 镜像函数参数化 | `panels/main/actions_impl.rs:483-528`（`diff_jump_prev/next`）、`panes/main/conflict_actions.rs:633-714`（conflict_jump 六件套） | 方向枚举参数，净删约 150 行 |
| 注册表表驱动 | `rows/diff_text/syntax/language.rs:246-510`（`tree_sitter_grammar`，265 行 match） | 265 → ~75 行，保留特例注释 |
| 删空壳 | `panels/main/history.rs`（2 行） | 消除导航陷阱 |

### P1 解散 `view/mod_helpers.rs`（5,877 行 → 0，每域一个 PR）

| 域（行号段） | 去向 | 依据 |
|---|---|---|
| ConflictResolverUiState（1386-4347，含约 2,643 行 impl+测试，占文件 45%） | `view/conflict_resolver/` 树内独立模块 | 已有 245 测试 |
| 终端类型族（4381-4438、5166-5372，14 类型） | terminal 模块旁 | 仅 2 个使用方 |
| PopoverKind 词汇表（4439-4955，417 行 enum + 子 Kind） | `panels/popover.rs`，view 根留再导出壳 | 99 个文件引用 |
| 拖拽分隔条状态机（70-101、460-543） | `view/resize_state.rs` | 广泛共享的真公共项 |
| StatusSection 多选状态机（728-1075） | 独立模块 | 10 文件共享 |
| Toast 族（9-21、686-727） | 并入 `toast_host.rs` | 仅 1 个使用方 |
| 预览类型判定（125-358） | `view/preview_kind.rs` | 7 文件使用 |
| 滚动/命中几何（103-124、567-685） | rows 层 | 消费方所在 |
| WrapCache/Markdown 预览状态（1076-1385） | panes/main 使用方 | fan-out 低 |
| view_mode/bootstrap/主题/diff 偏好（4956-5727） | `view/view_mode.rs` + `view/diff_prefs.rs` | 广泛共享 |
| `struct WorkTreeView`（5728-5872，145 字段） | `view/worktree_view.rs`（P1 末步执行，保证 mod_helpers.rs 清零） | 定义与 impl 异地问题在 P1 收口 |

收尾：删除 `view/mod.rs:251` 的 `use mod_helpers::*`，编译器逐文件暴露真实依赖，
显式补 import（一次性付清，之后命名空间不再被污染）。

### P2 结构归位（3-4 个 PR）

1. `struct MainPaneView`（`panes/main/helpers.rs:2999-3517`）→ 新建 `panes/main/state.rs`
   （`WorkTreeView` 的同类迁移已在 P1 末步完成）。定义与 impl 不再异地。
2. **`panels/main/`（9 文件约 1 万行，渲染半边）并入 `panes/main/`（行为半边）**：
   同一个 `MainPaneView` 的 impl 收回一棵树；`panels/` 回归窗口级 chrome 本位
   （popover/action_bar/bottom_status_bar/repo_tabs_bar/layout 骨架）。
3. `panels/layout.rs:447-3514` 的 `impl DetailsPaneView`（约 3,000 行）→ `panes/details/`；
   `panels/mod.rs:8-731` 的 ContextMenuModel 族（约 700 行）→ `components/`。
4. 命名去歧义：`reflog_panel.rs` 与 `panes/reflog.rs` 归位；三个 `conflict_resolver*`
   文件借 P3 拆分改名（逻辑层/渲染层各得其名）。
5. 撤销 `panes/main.rs:27` 的 `pub(crate) use helpers::*` 匿名泄漏（与 P3 的 helpers
   拆分联动，先降级为显式 re-export 清单）。

### P3 巨型文件域拆分（每文件一个 PR，互相独立、顺序任意）

| 文件（行数） | 目标形态 | 附加动作 |
|---|---|---|
| `view/settings_window.rs`（12,083） | `view/settings/{mod,window_chrome,categories,widgets,option_rows,git_runtime,external_editor,ai_commit}` + `domain/{general,terminal,diff,git_log,tags,git_executable,gpg_signing,merge_tool,environment,links}` + `tests/`；mod 的 Render 只做按 `SettingsCategory` 分发 | 35 个同构 setter 与 19 个同构 option_rows 收敛为泛型/宏（净删约 1,500 行）；拆 `render()`（3,045 行）为各域 card 函数 |
| `panels/popover.rs`（5,527） | `popover/{geometry,dialog,submit,open,dispatch,host}` | 保留子模块 `this: &mut PopoverHost` 惯例（后代模块零成本续拆）；211 字段按 picker 分组为子结构（`open_popover` 3647-3674 的 11 个 selected_index 连排重置即分组清单）；`new()`（880 行）按 prompt 域拆构造块 |
| `panes/main/core_impl.rs`（6,676） | 按六域拆 impl：resolved_output_syntax（2091-3230）/ settings（3772-4650）/ scroll_sync（544-905、6284-6480）/ target_query（4165-4530）/ context_menus（4657-5090）/ diff_wrap（5305-6010） | 6 对 `setter+_and_persist` + 4 个 mergetool 变体宏化；`new()`（575 行）按域拆构造块 |
| `panes/main/helpers.rs`（3,712） | struct 移走后切：resolved_output_text（124-790）/ conflict_blocks（790-2658）/ diff_metrics（2838-2997）/ scroll_reveal（1-120） | — |
| `panes/main/conflict_actions.rs`（4,283） | 五刀：conflict_nav（220-760）/ conflict_bootstrap（763-1984）/ conflict_pick（2207-3160）/ conflict_output_edit（3154-3550）/ conflict_alignment（3550-4112） | **`sync_conflict_resolver`（约 1,210 行，全库最大单体函数）分阶段函数化——局部重写授权的首选对象** |
| `view/conflict_resolver.rs`（6,382） | 按 `tests/` 既有命名切 10 子模块：text/parse/resolve/output_projection/block_diff/three_way_map/visibility/minimap/fold/provenance | 扁平 `pub use` 保住 869 处引用零改动 |
| `view/markdown_preview.rs`（5,793） | `view/markdown/{model,wrap,flatten,html,tables,inline,diff,parse}` + `tests/` | 三对"带/不带 spans"双版本合并（2917/2934、2686/2717、2826/2881）；`flatten_to_rows`（758 行）按事件类型拆 |
| `panels/main/diff_view.rs`（4,283） | 拆 diff_shortcuts（275-1147，按键表表驱动化）/ submodule_summary（72-157、1789-2407）/ search_overlay（11-70、1148-1766） | `diff_view` 本体（1,698 行）降级为"意图计算 + 分发"，body 分支下沉既有域文件 |
| `panes/history.rs`（7,725） | 测试外迁（3073-7725，占 60%）+ columns（1-548）/ reveal（549-1097）/ cache_build（2580-3072） | — |
| `panes/main/diff_search.rs`（4,283） | 测试外迁（3098-4283）；needle/matcher 双轨 trait 收敛（含两处逐字相同的嵌套 `fn line_text`） | 消除 `rows/diff_text/{build,prepared}.rs` 对 panes 的反向依赖（DiffSearchMatcher 下沉） |
| `rows/diff_canvas.rs`（4,313） | `diff_canvas/{blame,stage_gutter,streamed,geometry}` | `diff_text_paint_payload`（1,146 行）按阶段拆 |
| `rows/diff.rs`（4,103） | `diff/{collapsed_hunk,blame,rows}` | — |
| 4 个 pane 的 popover 委托三件套 | `history.rs:1842`、`sidebar.rs:2922`、`details.rs:1574`、`reflog.rs:543` → 共享扩展 trait | `set_theme`/`apply_ui_scale` 每 pane 重写一并统一 |
| 跨文件小去重 | `hash_highlights`（kit/text_truncation.rs:379 vs rows/diff_text/build.rs:1015）、tab 展开双版、`line_metrics`/`px_2`/`center_text_y` 双版（rows/conflict_canvas.rs:727 vs rows/diff_canvas.rs:3333） | 几何/度量助手下沉 kit |

### P4 非 UI crate（可与 P3 交错）

1. **worktree-state**
   - `store/reducer.rs:873` 的 `reduce_inner`（1,770 行 / 265 臂）改为按 `reducer/`
     既有域子模块分派。
   - `store/reducer/effects.rs` 改名（如 `loaded_results.rs`），消除与
     `store/effects.rs`、`msg/effect.rs`、两个同名测试文件的"effects"四重撞名。
   - `session.rs` 的 8+ 对 `persist_X/persist_X_to_path`（:570-:862）收敛为带
     path-override 的内部函数 + 公开薄封装；2,941 行内联测试外迁。
   - `reducer/util.rs` 的 `X`/`append_X` 双 API 家族（:473-533、:573/:583、:724/:769）
     统一为单一 `&mut impl EffectAccumulator` 入口。
2. **worktree-git-gix + worktree-core**
   - `repo/mod.rs:266-1228` 的 960 行委托壳宏化（`#[trace_op]` 属性宏或 `delegate!`
     声明宏），168 个 `_impl` 改回本名。
   - 可选：`services.rs:373` 的 200 方法巨型 trait 拆为域 trait（log/status/remotes/
     merge…），宏化后按痛感决定。
   - 可选：归并名不副实的 `worktree-git` crate（仅 384 行 no-op 后备，抽象本体在 core）。
3. **test-support crate**
   - 收编 `run_git`（35 处定义）、`is_git_shell_startup_failure`（11 处）、
     `set_fixed_mtime`、`hash_blob`、`make_executable`。
   - `worktree-git-gix/tests/status_integration.rs`（10,605 行 / 176 测试 / 零子模块）
     按域拆 `tests/status/` 目录，helper 移入共享模块。
4. **worktree-core**：`file_diff.rs`（4,266）按六个既有边界拆：line_text / align /
   plan / rows_anchors / levenshtein / benchmark。

### P4.5 可选尾巴（按实测编译痛感决定，默认不做）

把几乎不依赖 GPUI 的纯逻辑模块拆成独立 crate，改善编译并行：conflict_resolver
（`view/conflict_resolver.rs` 纯逻辑，869 处引用）、markdown 引擎（唯一外部依赖
`view/diff_text_model.rs` 的 CachedDiffStyledText）、syntax 引擎。属接口级工作
（可见性需从 `pub(in crate::view)` 升为跨 crate pub），必须显式立项再做。

## 6. 验收度量

| 指标 | 现状 | 目标 |
|---|---|---|
| >2,000 行的 src 文件 | 约 55 个 | < 10 个 |
| `*_helpers`/`*_impl`/`mod_helpers` 文件 | 10 个 | 0 |
| `run_git` 定义数 | 35 | 1 |
| glob 注入根（`use xxx_helpers::*`） | 2 处 | 0 |
| 净代码量 | — | 去重+宏+表驱动估计 -8K ~ -12K 行（约 2%） |

诚实预期：编译速度在 P0-P3 基本不变（单 crate 内拆文件不改编译图）；净行数收益
有限——**本方案的主要收益是结构**（改一个域不再惊动 2 万行文件、导航不再踩
panels/panes 命名陷阱、新人 onboarding 路径清晰）。

## 7. 风险与缓解

| 风险 | 缓解 |
|---|---|
| glob 链断裂引发大面积编译错误 | 兼容壳 `pub use` 过渡；编译器逐文件暴露真实依赖，属机械修复 |
| 拆分引入行为回归 | 纯搬移 PR 不混合任何改写；局部重写独立 PR 且只选测试最厚的域（冲突域 245 测试） |
| `#[cfg]` 平台分叉漏验（linux 变体、终端双实现、macOS 专属字段） | 每阶段过 CI Linux + 本地 Windows；涉及 macOS 字段时查 cross-platform-tests.yml 矩阵 |
| benchmark feature 符号被移动 | 涉及时加 `--features benchmarks` 编译检查；perf 预算 CI 的对象是行为而非文件位置 |
| 测试与实现的隐式耦合（如 `handle_diff_shortcut` 按键表 vs `shortcuts.rs` 断言） | 搬移同步更新；测试外迁时按域分文件降低耦合面 |
| 与并行开发冲突 | 每阶段拆小 PR、纯搬移优先（git 合并冲突面小）；避开活跃开发中的文件可跳过后补 |

## 8. 附录：证据索引（分析于 2026-08-31，行号为当时快照）

热点文件（非测试部分严重度排序）：settings_window.rs 12,083（含 4,250 行测试）；
panes/history.rs 7,725（60% 测试）；core_impl.rs 6,676；conflict_resolver.rs 6,382
（拆分缝隙已由 tests/ 命名预示）；mod_helpers.rs 5,877；markdown_preview.rs 5,793
（48% 测试）；popover.rs 5,527；store/reducer/effects.rs 5,579；rows/history.rs
5,134；rows/sidebar.rs 5,020；session.rs 4,912；diff_view.rs 4,283；
core/file_diff.rs 4,266；status_integration.rs 10,605（测试）；
panels/tests/file_diff.rs 18,031（测试）。

超长单体函数：settings render() 3,045（settings_window.rs:4687）；
sync_conflict_resolver 约 1,210（conflict_actions.rs:772）；diff_view 1,698
（diff_view.rs:2408）；reduce_inner 1,770（reducer.rs:873）；flatten_to_rows 758
（markdown_preview.rs:1206）；diff_text_paint_payload 1,146（diff_canvas.rs:1878）；
render_submodule_summary 619（diff_view.rs:1789）；handle_diff_shortcut 873
（diff_view.rs:275）；MainPaneView::new 575（core_impl.rs:1325）；working_tree_details
690（reducer/effects.rs:1315）。

重复清单（代表位置）：popover 委托三件套 ×4 pane（history.rs:1842 /
sidebar.rs:2922 / details.rs:1574 / reflog.rs:543）；set_theme ×4；schedule_ui_settings_persist ×3；whitespace_visible_text 双版（rows/conflict_resolver.rs:2973 vs rows/diff_text.rs:430）；hash_highlights 双版；tab 展开双版；line_metrics/px_2/center_text_y 双版；needle/matcher 双轨内嵌相同 fn line_text（diff_search.rs:2849/2923）；persist_X 家族 8+ 对（session.rs:570-862）；X/append_X 家族（reducer/util.rs）。
