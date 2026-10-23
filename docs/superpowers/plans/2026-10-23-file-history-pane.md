# 文件历史停靠化（file-history pane）— 设计 / 接线文档

> 迭代 09 追加项。目标：把「文件历史」从**一次性弹层**升级为**常驻、全高、可操作**的停靠视图。

## 0. 背景与问题

现状（`PopoverKind::FileHistory`，`view/panels/popover/file_history.rs`）：

| 维度 | 现状 |
|---|---|
| 入口 | 文件浏览器 / 状态文件 / 提交文件**右键「Show file history」**；diff 标题栏历史按钮；`Ctrl+H` |
| 呈现 | 弹层，包在 `components::context_menu` 里，宽 `LARGE_PICKER_WIDTH`，**列表高上限仅 340px**（超限窗口化） |
| 行为 | 点行 → `Msg::SelectCommit`（跳主提交列表并高亮）→ **立即 `close_popover`** |

痛点（用户反馈「不太好使用」）：

1. **易逝**：失焦/点行即关，无法与 diff 并排对照一个文件怎样一步步变化。
2. **高度受限**：340px 小窗扫长历史很憋屈。
3. **行无操作**：只能「跳主列表」，不能就地看该提交 diff、与上一版对比、看该 commit 的文件内容、作为 blame 起点。
4. **入口隐蔽**：藏在右键菜单里。

## 1. 关键事实（调研结论，别重开）

| # | 事实 | 影响 |
|---|---|---|
| F1 | `history_state.log` 与 `history_state.file_history` **同为 `Loadable<Shared<LogPage>>`** | 文件历史与整仓历史是**同一数据类型** → History 面板现有列/图/选择逻辑可直接复用 |
| F2 | `LogScope` = `HistoryMode`，只有 `FullReachable`/`FirstParent`/`NoMerges`/`MergesOnly`/`AllBranches`，**无 path 变体** | 现成 History 面板**不能**靠 `LogScope` 按文件过滤；需要独立的「文件作用域」标志 |
| F3 | `HistoryView` 在 `panes/main/core_impl.rs:633` 内创建，**位于 Main 面板内部**（不是顶层可停靠 pane） | 新建顶层停靠 pane 需动布局/缩放装配；改为「给现有 History 面板加文件作用域」复用最多、风险最低 |
| F4 | per-file 历史是独立数据通道（`LoadFileHistory` / `schedule_load_file_history` / `file_history`）且已有 `SelectedHistoryReload::FileHistory` 重载 | 加载/重载基础设施已存在，不需要新 trait |
| F5 | 弹层用 `limit: 200` | 面板沿用同一窗口（常量 `FILE_HISTORY_LIMIT = 200`） |

## 2. 方案

**给现有全高 History 面板加「文件作用域」**，而不是新建 dockable pane。

- 新增 `HistoryState::history_file_scope: Option<PathBuf>`。
- `Some(path)` 时：History 面板渲染 `file_history` 而非 `log`；标题区显示「File: <path>」作用域条 + 清除按钮。
- `None` 时：与今日行为逐字节一致（整仓 walk）。
- 行操作**完全复用**现有 History 面板能力（选择提交、对比、打开 diff、blame 等）。

收益：一次性解决易逝、高度、无操作三个痛点；复用 F1 的同构数据与现有行操作；不动布局/缩放装配。

## 3. 切片

| Slice | 内容 | 状态 |
|---|---|---|
| **S1 — 状态层** | `history_file_scope` 字段 + `Msg::SetHistoryFileScope` + `set_history_file_scope`（设作用域并加载）+ 单测 | ✅ 已完成 |
| S2 — 面板渲染 | History 面板按作用域在 `log` / `file_history` 间切换；作用域条（路径 + 清除） | 待做 |
| S3 — 入口改接 | 右键「Show file history」/ diff 标题栏 / `Ctrl+H` 改为派发 `SetHistoryFileScope` 并切到 History 面板，不再开弹层 | 待做 |
| S4 — 收尾 | i18n（作用域条文案）、弹层去留决策、全量门禁 + CI | 待做 |

## 4. 决策点

| # | 决策 | 结论 |
|---|---|---|
| D-F1 | 停靠化实现方式 | 给现有 History 面板加文件作用域（**不新建顶层 pane**）——依据 F1/F3，复用最多、不动布局 |
| D-F2 | 作用域是否复用 `file_history_path` | **否**，另设 `history_file_scope`：`file_history_path` 是「最近加载的路径」语义，与「面板当前是否处于文件模式」是两件事，混用会让弹层与面板互相踩数据 |
| D-F3 | 清除作用域是否清空已加载页 | **否**：清除只置 `None`，保留 `file_history`，回到同一文件时瞬时 |
| D-F4 | 首屏窗口大小 | `FILE_HISTORY_LIMIT = 200`，与弹层一致（F5） |
| D-F5 | 原 `FileHistory` 弹层 | S1–S3 期间保留（编译面不变）；S4 再决定退役或保留为「快速浏览」 |

## 5. 验证

| 腿 | 命令 / 判据 |
|---|---|
| a | `cargo +stable test -p worktree-state --lib` —— S1 实测 **800 passed / 0 failed** |
| b | `cargo +stable fmt --all --check` 干净 |
| c | `cargo +stable clippy -p worktree-core -p worktree-state -p worktree-git-gix -- -D warnings` 干净 |
| d | UI 门禁（S2/S3 起）：`cargo +stable clippy -p worktree-ui-gpui --all-targets -- -D warnings`（C 盘 `CARGO_TARGET_DIR`） |

回归：未设作用域时行为与改动前完全一致（`history_file_scope` 默认 `None`）。
