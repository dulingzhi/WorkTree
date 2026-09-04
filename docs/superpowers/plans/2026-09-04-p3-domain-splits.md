# P3 巨型文件域拆分 — 实施计划

- Spec：docs/superpowers/specs/2026-08-31-rust-codebase-refactor-design.md（P3 节 114-131 行为权威；本计划为其执行化）
- 前置：P0/P1/P2 已合入 dev 并推送（dev = d4dce497）。P2 终审 PHASE APPROVED。
- 分支策略：每任务一个分支（spec：每文件一个 PR），均自 dev 切出；任务互相独立，按本计划的性价比波次串行（SDD 禁并行实现者）。
- 执行模式：SDD（与 P0/P1/P2 相同——实现者 agent + 独立评审 agent + 台账 + 修复循环）。

## 继承约束（与 P2 Global Constraints 全同，此处锁定）

- **四腿验证矩阵**（每任务必跑，全绿才可提交）：(a) `cargo test --workspace --no-default-features --features gix`；(b) `cargo test --workspace`；(c) `rtk proxy cargo clippy --workspace --no-default-features --features gix -- -D warnings` → message|location 对集与仓库根 `clippy-baseline.txt`（31 行）逐字节比对（基线行站点漂移时按 T2/T3 先例更新该行并记录）；(d) `rtk proxy cargo test -p worktree-ui-gpui -- --list` 名单比对——纯搬移任务须空 diff；**测试外迁/拆模块任务允许纯模块路径改名**（裸名集全等，T2/T5 裁定类）。
- **W2**：具名 import/re-export 为默认接线；禁止新增 `use xxx::*` 行（既有 `use super::*;` 文化保留；迁入新文件的文件头 `use super::*;` 为 T2 特许惯例）。
- **G1**：末次全量验证后零源码改动；任何改动（含注释）→ 全量重跑四腿。
- **受众保持**：可见性升级须有编译器证据（E0603/E0425/E0616/E0451/E0446），逐名记录消费方；禁止预防性放宽。
- **rustfmt**：`rustfmt --edition 2024 --config skip_children=true <file>` 仅触及文件，四腿前完成。
- **串行 cargo**；LNK2019 stale-artifact 风暴解法：`cargo clean -p worktree-ui-gpui` 后重试（pristine 基线/list 捕获前建议先 clean——本阶段已 4 次）。
- **已知 flake**（裁定不修）：worktree `standalone_tool_mode_integration`；gpui visual `rebase_onto_picker_*`；worktree-state `dropping_receiver…`（P2 中 4 次，复核法 = 单目标重跑 ×3 + 全量重跑）。
- **Shell**：Windows Git Bash；grep -E 不用 -P；管道会被 hook 改写——一律文件重定向 + awk/grep 后处理。

## P2 遗留登记册（P3 内消解点已标注）

1. components→panels 的 `ContextMenuAction` 反向 import（components/context_menu_model.rs:2）——若 P3 将 components 定为叶子层，需在设置/菜单相关任务中回访。**本计划不定叶子层纪律，登记保留。**
2. `panes/main/conflict_actions.rs:8` 与 `core_impl.rs:1` 的既有 `use super::helpers::*`——随 **Wave 1 T3（helpers.rs 拆分）** 消解：helpers 切分为域模块后，两文件改具名 import。
3. `view/conflict_resolver.rs` 逻辑层最终改名——**Wave 2 T8**（conflict_resolver 拆分）中完成（拆分后 mod.rs 即逻辑层本位，按既有 tests/ 命名切 10 子模块后名实自洽）。
4. spec P3 表路径重锚定——**本计划已完成**（见下表"现态"列）。
5. 环境登记：3 flake + LNK2019 解法（已入上方约束）。

## 重锚定后的目标文件现态（2026-09-04 快照）

| spec 行 | spec 路径（行数） | 现路径（现行数） | 漂移 |
|---|---|---|---|
| 118 | view/settings_window.rs（12,083） | 同（12,085） | ≈0 |
| 119 | panels/popover.rs（5,527） | 同（6,075） | +548（自然增长；`open_popover` 与 `new()` 锚点需按内容重定位） |
| 120 | panes/main/core_impl.rs（6,676） | 同（6,676） | 0 |
| 121 | panes/main/helpers.rs（3,712） | 同（3,205） | −507（T1 struct 已移走，spec 预期之内） |
| 122 | panes/main/conflict_actions.rs（4,283） | 同（4,282） | ≈0 |
| 123 | view/conflict_resolver.rs（6,382） | 同（6,416） | +34 |
| 124 | view/markdown_preview.rs（5,793） | 同（5,793） | 0 |
| 125 | **panels/main/diff_view.rs**（4,283） | **panes/main/diff_view.rs**（4,283） | 路径（P2 T2）；行数巧合相同 |
| 126 | panes/history.rs（7,725） | 同（7,725） | 0 |
| 127 | panes/main/diff_search.rs（4,283） | 同（4,284） | +1 |
| 128 | rows/diff_canvas.rs（4,313） | 同（4,313） | 0 |
| 129 | rows/diff.rs（4,103） | 同（4,110） | +7 |
| 130 | details.rs:1574 等 4 处 | **panes/details/mod.rs**（P2 T3 目录化）+ history.rs/sidebar.rs/reflog.rs | 路径 |
| 131 | 跨文件小去重 4 组 | 原位（kit/text_truncation.rs、rows/diff_text/build.rs、rows/conflict_canvas.rs、rows/diff_canvas.rs） | 未动 |

所有 spec 行号锚点（如 `helpers.rs 124-790`）均为 2026-08-31 快照——**实现者一律按符号名/内容定位，行号仅作先验**（P2 既定惯例）。

## 任务波次（性价比排序：先便宜定模式、再中件、后大件、重写收尾）

### Wave 1 — 小件快赢 + 解锁件

**T1 popover 委托三件套 → 共享扩展 trait**（spec:130）
- 4 处现状：panes/history.rs、panes/sidebar.rs、panes/details/mod.rs、panes/reflog.rs 各自的 popover 委托 + `set_theme`/`apply_ui_scale` 重写。实现者先按符号（如 `open_popover`/`set_theme`/`apply_ui_scale`）定位四 pane 的同构块，抽共享扩展 trait（建议落 `panes/pane_chrome_ext.rs` 或 kit——按受众定）。
- 产出：4 pane 各删一套委托；trait 单点。list 腿：空 diff（无测试移动）。
- 风险：低。四处的字段名若不同构，trait 以 accessor 收口。

**T2 跨文件小去重**（spec:131）
- 4 组：`hash_highlights` 双版（kit/text_truncation.rs:379 vs rows/diff_text/build.rs:1015）；tab 展开双版；`line_metrics`/`px_2`/`center_text_y` 双版（rows/conflict_canvas.rs:727 vs rows/diff_canvas.rs:3333）。逐组裁定真源（按语义完整性/受众），下沉 kit 或保留一方具名复用；**逐字相同的直接删一方**，语义有差的记录后不强行合并（局部差异即设计）。
- 风险：低。注意 hash_highlights 两组若有 salt/顺序差异须逐字节比对输出语义。

**T3 helpers.rs 域拆分（3,205 行）**（spec:121）——解锁遗留 #2
- 切分面（按内容重锚定）：resolved_output_text / conflict_blocks / diff_metrics / scroll_reveal 四域 + T6 清单时代的 15+17 树外消费名为接线面基准。
- 接线：panes/main.rs 的两个具名清单按域改指新模块路径（消费方零改动为力争目标——若域名变了路径段，消费方批量机械改指，单 PR 内完成）。
- **消解点**：conflict_actions.rs:8 与 core_impl.rs:1 的 `use super::helpers::*` 改具名 import（W2 终于全域成立）。
- 风险：中。这是 P1 mod_helpers 拆分的同型任务——P1 的 14 模块先例即模板。

### Wave 2 — 中件域拆分

**T4 diff_search.rs（4,284）**：测试外迁（:3098 起）+ needle/matcher 双轨 trait 收敛（含两处逐字相同嵌套 `fn line_text`）+ 消除 rows/diff_text/{build,prepared}.rs 对 panes 的反向依赖（DiffSearchMatcher 下沉）。list 腿：允许纯路径改名。

**T5 rows/diff.rs（4,110）**：→ `diff/{collapsed_hunk,blame,rows}`。

**T6 rows/diff_canvas.rs（4,313）**：→ `diff_canvas/{blame,stage_gutter,streamed,geometry}`；`diff_text_paint_payload`（1,146 行）按阶段拆。

**T7 markdown_preview.rs（5,793）**：→ `view/markdown/{model,wrap,flatten,html,tables,inline,diff,parse}` + tests/；三对"带/不带 spans"双版本合并（spec:124 行号按符号重锚定）；`flatten_to_rows`（758 行）按事件类型拆。

**T8 conflict_resolver.rs（6,416）**：按 tests/ 既有命名切 10 子模块（text/parse/resolve/output_projection/block_diff/three_way_map/visibility/minimap/fold/provenance）；扁平 `pub use` 保住 869 处引用零改动；**同时闭合遗留 #3**（逻辑层名实归位）。869 处引用零改动是本任务的黄金验收标准（list 腿 + 引用计数前后比对）。

### Wave 3 — 大件

**T9 history.rs（7,725）**：测试外迁（约 60%，:3073 起）+ columns/reveal/cache_build 三域。测试迁移量大但机械——性价比实际偏高，故在大件中排首。

**T10 popover.rs（6,075）**：→ `popover/{geometry,dialog,submit,open,dispatch,host}`；211 字段按 picker 分组子结构；`new()`（880 行）按 prompt 域拆构造块。保留 `this: &mut PopoverHost` 子模块惯例。**注意 +548 行漂移——spec 的 selected_index 连排重置锚点按符号重定位。**

**T11 core_impl.rs（6,676）**：六域拆 impl（resolved_output_syntax/settings/scroll_sync/target_query/context_menus/diff_wrap）；6 对 setter+_and_persist 与 4 个 mergetool 变体宏化；`new()`（575 行）按域拆。

**T12 diff_view.rs（4,283）**：diff_shortcuts（按键表表驱动化）/submodule_summary/search_overlay 三域；`diff_view` 本体（1,698 行）降级为意图计算+分发。

### Wave 4 — 最大件 + 授权重写

**T13 settings_window.rs（12,085）**：全库最大文件。目标形态见 spec:118（settings/{window_chrome,categories,widgets,option_rows,git_runtime,external_editor,ai_commit} + domain/ 9 域 + tests/）。**收敛动作：35 个同构 setter + 19 个同构 option_rows → 泛型/宏（净删约 1,500 行）；render()（3,045 行）按域拆 card 函数。**建议再拆两个 PR：13a 纯拆分（零删除）、13b 收敛（泛型/宏，可独立评审）。

**T14 conflict_actions.rs（4,282）+ `sync_conflict_resolver` 分阶段函数化**（spec:122）——**局部重写授权的首选对象，独立 PR**。五刀拆分（conflict_nav/bootstrap/pick/output_edit/alignment）先行；1,210 行单体函数的函数化在其后、同 PR 内分提交。**这是 P3 唯一允许改变内部结构的任务——行为不变仍由四腿兜底，但评审透镜升级为"逐段等价性"。**

## Self-Review 结论

1. **Spec 覆盖**：P3 表 15 行全部映射（T1-T14，settings_window 拆 13a/13b 两 PR）；遗留 #2/#3 的消解点分别挂到 T3/T8；遗留 #1/#4/#5 已登记。
2. **性价比序**：Wave 1（T1/T2 小件 + T3 解锁件）→ Wave 2 中件 → Wave 3 大件（history 测试迁移实际便宜故排首）→ Wave 4 最大件 + 唯一授权重写。
3. **验收一致性**：四腿矩阵、W2/G1、flake 登记全部继承；测试外迁任务的 list 腿裁定标准已预载（纯路径改名类）。
4. **风险登记**：T13 的 −1,500 行收敛是全阶段最大净删收益但非纯搬移（宏语义等价性评审）；T14 是行为保持边界最薄的任务，排最后（管道最热时执行）。

## 执行门槛

本计划经用户批准后，自 dev（d4dce497）切 `refactor/p3-domain-splits` 分支，按 Wave 1→4 串行 SDD 执行。每任务：简报派发 → 实现者 → 四腿 → 提交 → 独立评审 → 修复循环 → 台账。Wave 内顺序可按实现者反馈微调（任务互相独立）；Wave 间顺序不变。
