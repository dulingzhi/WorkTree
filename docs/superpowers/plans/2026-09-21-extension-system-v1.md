# 扩展系统 v1（迭代 08-1）— 设计草案

> 状态：草案，待用户确认两个范围分叉（见 §6）后再落地。
> 来源：`docs/roadmap-long-term.md` §「迭代 08」#1 + 决策点 D8（① 重启，声明式 v1，仅迭代 08）。

## 1. 目标与范围（来自 roadmap）

- **声明式优先**：外置配置文件注册**命令面板条目** + **脚本命令** + 生命周期**钩子**（`RepositoryOpened` / `BeforeCommit` / `AfterCommit`）。
- **定位**：自用取向——自动化本人/本团队的重复操作；不面向第三方生态。
- **明确不做（留二期）**：进程内扩展、WASM（Rust ABI 不稳定，不早投）。不引入插件 ABI / 不开放内部 trait。

## 2. 配置模型（v1 草案）

全局配置文件，落在现有 `worktree_state::session::app_data_dir()` 下（与 themes 同目录，跨平台已支持）：

- Windows：`%LOCALAPPDATA%/worktree/extensions.toml`
- Linux：`$XDG_DATA_HOME/worktree/extensions.toml` 或 `~/.local/share/worktree/extensions.toml`
- macOS：`~/Library/Application Support/worktree/extensions.toml`

```toml
# 用户声明的脚本命令
[[command]]
id = "fmt-cargo"              # 唯一标识；重复 id 后者覆盖前者并记 warning
label = "Run cargo fmt"       # 面板里显示的名字（也是 fuzzy 匹配目标）
command = "cargo fmt"         # 在仓库 workdir 里执行的 shell 命令
requires_repo = true          # true=仅当存在激活仓库时出现；false=全局可用
context = ["palette"]         # 出现位置：palette（v1）；commit-menu/repo-menu 留二期
confirm = false               # true=执行前弹确认（破坏性命令建议开）

# 生命周期钩子
[[hook]]
event = "AfterCommit"         # RepositoryOpened | BeforeCommit | AfterCommit
command = "git push"          # 与 [[command]] 同样的执行管线
```

解析策略：**容忍式**——单条坏条目跳过并 `applog_warn!`，不让整个文件崩溃；无法解析整个文件时记 error 并回退到「无扩展」。

## 3. 集成点（已勘察代码）

- **命令面板**：`crates/worktree-ui-gpui/src/view/command_palette.rs:821` `filtered_commands()` 现遍历静态 `COMMANDS` 数组。需改为「静态 + 动态（来自配置）」合并；动态条目的 `CommandEntry` 携带「运行用户脚本」的分派标识。
  - **测试护栏**：`command_palette.rs:1486` `every_palette_command_has_a_registered_handler` 要求每个 palette 命令都有 `execute_command` arm —— 用户命令必须满足（新增一个 arm：按 id 取配置、用现有仓库命令管线执行）。
- **执行管线**：复用 `worktree-state/src/store/effects/repo_actions.rs` 的 `spawn_with_repo` 外部命令执行（已有超时/取消/忙碌门控）；脚本在仓库 `workdir` 运行，stdout/stderr 进 toast / 日志。
- **钩子触发点**：
  - `RepositoryOpened`：仓库打开路径（reducer 加载完成处）触发。
  - `BeforeCommit` / `AfterCommit`：`Msg::Commit` → `crates/worktree-state/src/store/reducer/actions_emit_effects.rs:287` `commit()` 发 `Effect::Commit`。`BeforeCommit` 在发 effect 前、`AfterCommit` 在 commit effect **成功返回**后触发（失败不触发 AfterCommit）。
- **配置加载**：新增 `worktree-core`（纯 `std::fs` + `toml` 解析，不碰任何 trait）或 `worktree-state::session` 的 `load_extensions()`；启动时与 repo 打开时各加载一次（配置可被外部改动，沿用「每次打开无条件重扫」的 hooks UI 思路，而非缓存 `Ready`）。

## 4. 落地步骤（建议分期，全部属 08-1）

- **Stage 1 — 配置 + 面板命令**：
  1. 配置 schema（`serde` + `toml`）与 `load_extensions()`（全局 `extensions.toml`）。
  2. `filtered_commands()` 合并动态条目；`execute_command` 增加用户脚本 arm。
  3. 单元/集成测试：解析容错、面板合并（`filtered_commands` 含用户命令）、dispatch arm 执行 mock 命令。
- **Stage 2 — 钩子**：
  1. `RepositoryOpened` / `BeforeCommit` / `AfterCommit` 触发点接线（复用 Stage 1 的执行管线）。
  2. 测试：三个事件分别在正确时机触发、AfterCommit 在失败后不触发。
- （**Stage 3 — 可选，v1 可暂缓**：配置管理 UI，用手编 TOML 即可。）

## 5. 风险与约束

- **安全**：用户脚本以 GitComet 进程权限在仓库目录运行。v1 **不自签安全承诺**；文档明确「这是用户自己负责的本地自动化」，不解析/不联网/不隔离。
- **取消/超时**：用户脚本走现有 `git_command_timeout()` / 取消机制；不另造。
- **P5 model freeze**：不新增 `GitRepositoryDiff` trait 必需方法；扩展系统全部走 `worktree-core` 自由函数 + 现有 effect 管线，与 特性 4（native hooks UI）同路线。
- **CI 门禁**：UI 改动必须真跑 `cargo test -p worktree-ui-gpui --lib`（跨平台 `ui-lib-tests` 硬门禁）；本地用 `CARGO_TARGET_DIR=C 盘` 跑。

## 6. 待确认的两个范围分叉

1. **配置范围**：仅全局 `extensions.toml`（推荐，v1 最小闭环；自用场景 80% 覆盖）还是 全局 + 仓库级 `.gitcomet/extensions.toml` 合并（更灵活但需定义合并/优先级语义，工作量 +）。
2. **命令出现位置（v1）**：仅命令面板（推荐，与 roadmap「注册命令面板条目」字面一致）还是 面板 + 提交/仓库右键菜单（体验更顺，但需定义菜单注入点与可见性条件）。

钩子（RepositoryOpened / BeforeCommit / AfterCommit）按 roadmap 属 v1，默认包含在 Stage 2，不另问。

## 7. 验收

- `cargo fmt` / `cargo clippy -D warnings`（各 crate）/ `cargo test`（含跨平台 `ui-lib-tests`）全绿。
- 配置解析单测（容错）；面板合并单测；三个钩子触发单测（mock 命令，断言触发时机）。
- 无新增 `unsafe`、无新增 `GitRepositoryDiff` 必需方法、无破坏公开 API / 存储格式。
