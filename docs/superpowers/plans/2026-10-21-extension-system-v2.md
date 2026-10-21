# 扩展系统 v2（迭代 09-A）— 设计 / 接线文档

> 迭代 09 锚点特性。v1（08-1）已交付：全局 `extensions.toml` + 命令面板命令 + 3 个生命周期钩子
> （`RepositoryOpened` / `BeforeCommit` / `AfterCommit`）。v2 把扩展从「全局单例」升级为
> 「全局 + 每仓库叠加」，并补齐设置 UI 与内置扩展。本文档先落地 **Slice 1（每仓库 toml + 合并）**，
> 后续 Slice 2/3 在同一抽象上叠加，不引入新的 `GitRepository*` trait 方法（沿用迭代 06「零新 backend 接口」纪律）。

## 0. 目标

- 仓库可在自身工作目录放一份 `extensions.toml`，注册**只在该仓库生效**的命令与钩子，叠加在全局配置之上。
- 不破坏 v1 语义：没有每仓库配置时行为完全等同于 v1（全局单例）；容错边界不变（缺文件/不可读/不可解析 → 空配置）。
- 为 Slice 2（设置 UI：来源徽标 + 启停）与 Slice 3（内置扩展：随应用分发的 bundled 脚本）打好基础。

## 1. 范围

| Slice | 内容 | 工作量 | 状态 |
|---|---|---|---|
| **1** | 每仓库 `extensions.toml` 加载 + 与全局合并；命令面板/钩子/执行按合并集工作 | S | 本次 |
| 2 | 扩展设置面板：列出所有生效扩展、标注来源（global/repo）、运行时启停 | M | 待定 |
| 3 | 内置扩展：随应用分发的 bundled 脚本（如 conventional-commit 校验、分支命名策略） | M | 待定 |

本文档只落实 Slice 1。Slice 2/3 复用本片的 `merge_extensions` 与「来源」概念，不在本片范围。

## 2. 数据模型与 API（worktree-core，纯函数）

v1 已有：

```rust
pub struct Extensions { pub commands: Vec<ExtensionCommand>, pub hooks: Vec<ExtensionHook> }
pub fn load_extensions(path: &Path) -> Extensions  // 容错
```

新增（Slice 1）：

```rust
/// 合并全局配置与仓库本地配置。
/// - 命令：同 `id` 时仓库项覆盖全局项（仓库配置优先；若全局已存在则保留其位置，否则追加到末尾）。
/// - 钩子：直接拼接——同一事件的全局钩子与仓库钩子都会触发。
/// 两个输入各自容错（见 `load_extensions`），合并不会失败。
pub fn merge_extensions(global: &Extensions, repo: &Extensions) -> Extensions
```

每仓库配置位置：`<repo workdir>/extensions.toml`（与全局 `app_data_dir()/extensions.toml` 对称）。
用户自行决定是否提交该文件（可 `.gitignore`）。

## 3. UI 接线（worktree-ui-gpui，worktree_view.rs / mod.rs）

当前 `WorkTreeView::extensions` 是**全局单例**（构造时从 `app_data_dir` 读一次）。Slice 1 不改这个字段语义——它仍是全局集；新增一个**按需合并**的私有 Helper，在三处消费点替换为合并集：

```rust
// worktree_view.rs / mod.rs
fn merged_extensions(&self, workdir: Option<&Path>) -> Extensions {
    match workdir {
        Some(dir) => merge_extensions(
            &self.extensions,
            &load_extensions(&dir.join("extensions.toml")),
        ),
        None => self.extensions.clone(),
    }
}
```

三处改动（均为局部，无新 trait / 无 re-entrancy）：

1. `open_command_palette`（~809）：`palette.set_extensions(self.merged_extensions(active_workdir), cx)`
   —— 命令面板现在展示 global ∪ 当前仓库命令。
2. `run_extension_hooks(event, workdir, cx)`（~1972）：
   `select_extension_hooks(&self.merged_extensions(workdir.as_deref()), event, workdir.as_deref())`
   —— 钩子按「全局 + 该仓库本地」集合触发；`workdir` 已是具体仓库路径（来自 `state_apply.rs`）。
3. `run_extension_command(ext_id, ...)`（~1864）：命令查找从 `self.extensions.commands` 改为
   `self.merged_extensions(active_workdir).commands`，使仓库命令可被按 `ext:<id>` 派发执行。

命令面板本身（`command_palette.rs`）无需改动：`set_extensions` 已接收合并集，`filtered_commands` 的
`requires_repo`/active-repo 过滤逻辑照常工作；`run_extension_command` 侧补上合并查找即闭环。

## 4. 合并语义示例

全局 `extensions.toml`：

```toml
[[command]]
id = "lint"
command = "cargo clippy"
[[hook]]
event = "AfterCommit"
command = "git push"
```

仓库 `extensions.toml`：

```toml
[[command]]
id = "lint"        # 覆盖全局：仓库用自己的 lint
command = "cargo clippy --features repo"
[[command]]
id = "bump"
command = "./bump.sh"
[[hook]]
event = "BeforeCommit"
command = "cargo fmt --check"
```

合并结果（命令面板 / 钩子实际生效集）：

- 命令：`lint`（仓库版，覆盖全局）、`bump`（仓库新增）、—— 全局 `lint` 被仓库覆盖。
- 钩子：`AfterCommit: git push`（全局）+ `BeforeCommit: cargo fmt --check`（仓库），两个事件都会触发。

## 5. 决策点

| # | 决策 | 结论 |
|---|---|---|
| **D-E1** | 每仓库配置位置 | `<workdir>/extensions.toml`（仓库根，非 `.git/`）；与全局对称，用户自决是否提交 |
| **D-E2** | 命令 id 冲突 | 仓库覆盖全局（仓库配置优先），保留全局位置或追加 |
| **D-E3** | 钩子冲突 | 不去重直接拼接（同事件全局+仓库都触发）；钩子无 id，覆盖无意义 |
| **D-E4** | 加载时机 | Slice 1 按需懒加载（每次消费点读 toml，文件小且容错）；后续 Slice 2/3 如需可加 `RepoId` 缓存 |
| **D-E5** | `requires_repo=false` 的仓库命令无活动仓库时 | 回退到 `app_data_dir`（同 v1 全局命令语义） |

## 6. 验证（沿用迭代 06/08 四腿基线）

| 腿 | 命令 / 判据 |
|---|---|
| a | `cargo test -p worktree-core --lib extensions` —— 新增 `merge_extensions` 单测（追加 / 同 id 覆盖 / 钩子拼接） |
| b | `cargo +stable fmt --all` 干净 |
| c | `cargo +stable check -p worktree-core -p worktree-ui-gpui`（UI crate 用 C 盘 `CARGO_TARGET_DIR`，见项目 MEMORY） |
| d | `cargo +stable clippy -p worktree-core --lib -- -D warnings`；UI crate clippy 在 C 盘 target 跑（全量门禁） |

回归：无每仓库 `extensions.toml` 时行为与 v1 逐字节一致（合并退化为全局单例）。

## 7. Slice 2/3 前瞻（不在本片）

- **Slice 2 设置 UI**：复用 `merge_extensions` 的结果，加「来源」字段（`Source::Global | Source::Repo`）展示；
  运行时启停用 `disabled_extension_ids: FxHashSet<String>` 在合并/过滤时剔除（不回写 toml，避免篡改用户文件）。
- **Slice 3 内置扩展**：随应用分发的 bundled 脚本路径作为 `command` 注入合并集（来源标记为 `Builtin`）；
  校验类扩展（如 conventional-commit）若需「拦截提交」而非「跑脚本」，需在 v1 的 shell-command 钩子模型外
  另开 `BuiltinHook` 种类——本片不引入，留待 Slice 3 设计。
