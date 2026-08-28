# RepositoryTree（Rust）功能迭代路线

> 参照系：C# 版 RepositoryTree（Sourcegit fork，2020 年起 1,073 commits，活跃至 2026-08）——同一作者 5 年日常使用沉淀的功能广度。
> 约束：**不合并 `feature/jj-vcs` 及任何历史滞留分支**，jj 保持现状；本路线只排功能迭代；**subtree 与 git-flow 经决策不做**，不在计划内。
> 依据：两版全量功能盘点 + Rust 侧逐项 grep 核实（GPG / LFS / 统计 / per-repo SSH key / archive / assume-unchanged / WIP 节点均确认缺失；标签会话恢复初判缺失系误报，实为已实现，见迭代 02-5）。工作量 S/M/L 按单人周粗估。

---

## 两版能力对照

**Rust 版领先（不必动）**：行级/hunk 级暂存、交互式 rebase 编辑器、三方冲突编辑器（autosolve/手动对齐/zdiff3）、worktree 管理、blame（含 worktree 增量）、内联文件编辑、跨平台、大仓库性能基建、tree-sitter 高亮、命令面板、内嵌终端、AI commit（HTTP 双 provider）。

**C# 版领先 → 移植池（已核实 Rust 缺失）**：

| 功能 | C# 版程度 | 证据 |
|---|---|---|
| GPG 签名 | Preference 独立页签 + gpgsig 解析跳过 | `ConfigureVM.cs`、`Preference.xaml` |
| Git LFS | 检测、LFS 变更 diff 视图、对象查看、prune | `Commands/LFS.cs`、DiffViewer SetLFSChange |
| AI provider 矩阵 | HTTP×2 + CLI×4(claude/codex/gemini/ollama) + Copilot(gh token→GitHub Models) + Codex auth.json/TOML + Claude settings.json + env + 手动，全链自动发现 | `Services/*ConfigResolver.cs`、`CliSpecRegistry.cs` |
| AI commit 历史格式示例 | prompt 附最近 10 条 subject 作风格样例 + 输出清洗 | `CommitPromptBuilder.cs`（spec/plan 文档先行） |
| 统计窗口 | 周/月/年贡献者排行 + 自绘图表 | `Statistics.xaml`、`Chart.cs` |
| per-repo SSH key | clone/pull/push/fetch 注入 `core.sshCommand` | `Commands/*.cs` |
| 跨历史搜索 | 内存快筛 + `git log --all --grep/--author`（上限 100） | `CommitSearchService.cs` |
| 按路径 stash | `--pathspec-from-file` | `Commands/Stashes.cs` |
| GitLab 建 MR | 推送弹窗 4 个 push-option | `Commands/Push.cs`、`PushView.xaml` |
| archive 导出 | zip 打包任意提交 | `Commands/Archive.cs` |
| assume-unchanged | 专用管理对话框 | `Views/AssumeUnchanged.xaml` |
| 标签页会话恢复 | RestoreTabs 启动恢复 + 4 连性能优化 | `Preference.RestoreTabs` |
| WIP 虚拟节点 | 图谱顶部虚线工作区提交 | `WipCommitFactory.cs` |
| 插件系统 | IPlugin + 命令面板注入 + 反射加载（无示例插件） | `Plugins/IPlugin.cs`、`PluginLoader.cs` |

**双方都缺 → 新功能池**：bisect、操作级 undo、GitHub PR/CI 集成、AI hunk 解释 / PR 描述、覆盖率 overlay。

（两边等价、不列项：命令日志面板、外部编辑器/工具探测、升级自检、JSON 外置主题、i18n en/zh-CN。）

---

## 迭代 01 — v0.2.x「高频入口 + AI 对齐」（2–3 周）

**目标**：关掉所有半接线入口，把 C# 版最近一年最重要的 AI 子系统对齐过来，搜索补上日常最高频的缺口。

1. **命令面板接线** `S` ✅（2026-08-27）
   10 个空 handler 接已就绪的后端与弹窗（checkout-remote-branch、delete-remote-branch、merge、delete-tag、remove-remote、edit-remote-url、remove-submodule、remove-worktree、discard-all）。验收：palette 可发现并执行全部条目 + handler 注册测试。
2. **跨历史提交搜索** `M` ✅（2026-08-27）
   按 C# `CommitSearchService` 的两级设计移植：已加载列表内存快筛（subject/作者/SHA + 高亮）→ 全历史 `git log --all --grep/--author`（上限 2000）；入口 palette `search-commits` + Ctrl+F。实测走 subprocess `git log`（与 blame 同通道，复用 pretty-record 解析器），两级 UI 为 CommitSearchPicker：本地命中即时过滤、尾部动作行触发深搜、结果独立分组且不被输入过滤误删（`match_any_query`）。2026-08-28 对齐 C# 语义：Ctrl+F **默认开提交搜索**（C# 无条件开 histories 搜索；本工程 diff 视图会整块替换提交列表，若「有 diff 可见即归 diff 搜索」则提交列表实际永远够不到快捷键）——diff 侧仅在自身上下文保留：搜索浮层已开（翻页中）或 diff 面板持有焦点；已打开的 CommitSearchPicker 吞掉按键，不会在弹窗背后激活 diff 搜索。
3. **AI commit 两项对齐** `M` ✅（2026-08-27）
   (a) 最近 10 条 commit subject 作格式示例进 prompt——按 C# `CommitPromptBuilder` 规格完成：system prompt / 4k 截断（补上 `[truncated]` 标记）/ 示例块不计入截断预算 / 输出清洗全套对齐，state 侧 `LoadAiCommitContext` 一并带出 subjects；
   (b) provider 矩阵扩展 ✅：`ai_commit_sources` 模块落地 C# 配置来源设计——10 种来源（manual / claude-code / codex / copilot / env / cli-claude / cli-codex / cli-gemini / cli-ollama / custom）；resolver 链（`~/.claude/settings.json` env 块、`~/.codex/auth.json` + config.toml 极简 TOML、gh hosts.yml + GH_TOKEN/GITHUB_TOKEN → GitHub Models、环境变量），凭证生成时实时解析、永不落盘；CLI 生成器走 `smol::process`（{PROMPT} 单参数 argv、60s 超时 kill_on_drop、stdout 清洗）；设置页 Source 下拉 + 非 manual 来源可用性状态行（后台检查）+ custom 命令模板输入；session 持久化 `ai_commit_source` / `ai_commit_custom_command`。
4. **修 2 个已知失败测试** `S` ✅（2026-08-27）
   `file_and_diff_context_menu_shortcuts_match_expected_actions`、`multi_cherry_pick_rejects_merge_commits_before_starting`。

---

## 迭代 02 — v0.3.0「工作流广度移植」（4–6 周）

**目标**：把 C# 版验证过的工作流广度搬到 Rust 版——LFS、GPG 签名、stash 与 SSH 是日常高频的硬缺口（subtree、git-flow 已决策不做）。

1. **Git LFS** `M` ✅（2026-08-27）
   启用检测（pre-push 钩子 / filter 配置）、diff 视图 LFS 变更分支、对象内容查看、清理命令联动 prune。实现：启用检测 = common_dir `hooks/pre-push` 含 `git lfs pre-push`（纯 IO，worktree 共享主仓钩子）；按文件 = `git check-attr -z filter` 三元组解析（值恰为 `lfs`）；指针变更 = 复用既有 unified-diff 命令解析 `-oid sha256:`/`+oid`/`size` 行（上下文 ` size ` 行回填共享尺寸）。diff 视图：worker 侧同一加载先发文本结果（保持缓存/视图门控走熟路）再发 `DiffFileLfsLoaded`，渲染端 `diff_lfs_panel` 面板（新旧 oid 短码 + 尺寸 B/KiB/MiB）优先于图片与文本（对齐 C#：所有 LFS 文件都显示指针面板，图片文件亦然）。清理 = RepoCommand 管线 `cleanup-repository` palette 命令：`git gc` 后条件接 `git lfs prune`（prune 失败折入输出 stderr 不失败整命令，离线安全）。`lfs_smudge_bytes`（`git lfs smudge` stdin→stdout）已入 trait 供后续保存/预览用。后续项：内联 submodule diff 的 LFS 分支（保持纯文本）、diff 视图图片 smudge 预览。
2. **stash 增强** `S/M` ✅（2026-08-27）
   按路径 stash（状态行右键「Stash changes…」→ 弹窗携带选中路径，`git stash push [--] <path>…` 直接 argv 传路径，多选时按选择解析）、--keep-index 与 include-untracked（弹窗内两个勾选行，默认 untracked=true/keep-index=false，每次打开重置）、stash branch（stash 右键菜单 + palette `stash-branch` → 选贮藏 → 分支名弹窗预填 `stash-<n>` → `git stash branch`）。trait 侧 `stash_create` 扩为 (message, include_untracked, keep_index, paths)，新增 `stash_branch`（默认 Unsupported，仿 cherry_pick_with_output 模式）；成功后 hook 刷新贮藏列表，HEAD 移动交给 repo monitor。
3. **GPG 签名** `S` ✅（2026-08-27）
   `commit.gpgsign`/`user.signingkey` 配置检测 + 设置页区块 + 签名状态展示。实现：设置新增「GPG signing」分类（GitExecutable 之后），UI 层直连 `git config --global`（零状态层改动）——commit 开关始终写显式 `true`/`false`（git 把裸 key 当 true），`user.signingkey`/`gpg.program` 文本行 + Apply，留空即 `--unset`，失败红色错误行；构造窗口时读三键快照（设置窗口每次打开新建）。签名状态为存在性徽章：`Commit.signed`（gpgsig/gpgsigssh 头存在即真）→ 历史行 sha 左侧绿色 `✓`（GitHub verified 惯例）。C# 的 gpgsig 原文解析跳过在 Rust 不需要——gix 分离头与消息，`%s`/`%B` 路径天然干净（仅 `git log --follow` pretty 路径恒读作未签名，已注释）。CommitDetails 徽章（约 55 处字面量）与验签（区别于存在性）为后续项。
4. **per-repo SSH key** `S` ✅（2026-08-27）
   clone/pull/push/fetch 注入 `core.sshCommand`；与现有 SSH passphrase 桥接协同。实现：key 存 repo git config `remote.<name>.sshkey`（C# RemoteVM 约定）；远程右键「Set SSH key…」→ 输入 + Save/Clear 弹窗（Save 空输入禁用，Clear 幂等清除）；fetch --all 仅当全部 remote key 一致时注入单一全局 `core.sshCommand`，pull/push/delete-remote-branch 按 upstream/目标 remote 精确注入；路径经单引号 + `'\''` 转义（git 经 shell 执行该值）、`validate_ref_like_arg` 防选项注入；与 SSH_ASKPASS 桥接正交（passphrase 仍走 askpass），`command_may_require_auth` 跳过 `-c` 值不受影响。clone 时选 key 为后续项（C# 仅弹窗内临时使用，remote 级持久化已覆盖后续操作）。
5. **桌面体验小件包** `M`（四件合计）
   archive zip 导出 ✅（2026-08-27）、assume-unchanged 管理对话框 ✅（2026-08-27：后端 trait `assume_unchanged_list`/`set_assume_unchanged`（gix 走 `ls-files -v` 小写 h 标签解析 + `update-index --[no-]assume-unchanged`）；`SetAssumeUnchanged` 走 RepoAction 管线（busy 门控 + 完成后若列表已加载则自动重载）；管理弹窗 `PopoverKind::AssumeUnchangedManager`（palette「Assume-Unchanged Files…」打开，开弹窗即 `LoadAssumeUnchanged`，行 = 等宽路径 + 「Remove flag」按钮，弹窗常开、行随重载消失）；状态文件右键「Assume unchanged」仅未暂存且已跟踪文件出现（Untracked 无索引项可标记）；指纹侧 discriminant 110 + `assume_unchanged_rev`/loadable 哈希）、多标签会话恢复 ✅（2026-08-27 审计勘误：已存在——启动 `Msg::RestoreSession` 恢复 tab 与激活项（view/mod.rs，`should_auto_restore` 门控：startup_probe 未禁用、非 FocusedMergetool、`auto_restores_session()` 仅 Live 模式、store 未预载）；open/restore/close/close_repos/activate/reorder 各 reducer 均发 `Effect::PersistSession` 落盘；始于 `256496e8 open last repositories by default`，session_integration 16 测试全绿）、历史图 WIP 虚拟提交节点 ✅（2026-08-27：该行本就存在但点击是死路——现在它是可选的一等虚拟节点。`CommitId::uncommitted()`（40 个 0，双哈希长度皆满足 `is_uncommitted_commit_id`）作哨兵只存于 `selected_commit`、永不进 multi_selection；专用 `Msg::SelectWorkingTreeSummary`（幂等 + 已选中时仅 resync），选中时清 multi_selection/范围比较、合成 `working_tree_details`（staged 后 unstaged 映射 CommitFileChange，parent_ids=[HEAD]），三处 status 回复（status/worktree_status/staged_status loaded）后 `resync_working_tree_details_if_selected`（内容不变则不 bump rev）；详情面板在哨兵下渲染 `working_tree_review_view`（标题 + 关闭（ClearCommitSelection）+ 虚拟化文件行，行点击 = 状态区同款 `DiffTarget::WorkingTree{path, area}`，staged→staged patch / unstaged→worktree patch；干净树显示空态）；历史行 `selected` 认哨兵、点击改派 SelectWorkingTreeSummary；键盘 ↑↓ 走到行 0 同样选中（`resolve_history_selected_list_index` 把哨兵解析为 list 0）；选中链高亮在哨兵下锚定 HEAD；图谱节点从硬编码列 0 改为 HEAD 即可见行 0 时用 HEAD 自己的 lane（`history_worktree_node_placement`，否则保持列 0 回退），节点列下传 `worktree_band_connect_from_top_col` 使下方行的向上短接线与之无缝；侧栏 worktree「reveal」落到本行时也真选中。导航栈天然回放（快照存 selected_commit，`select_commit_multi` 顶部把哨兵路由回本 handler）；log 替换不冲掉（multi 为空不触发 reconcile）。测试：state +5（选中/位移、resync、log 替换存活、sentinel 路由、导航步进）、ui 单测 +3（placement 列/色、HEAD 非首行回退、band 列透传）、UI 测试 +3（接管详情面板且 staged 先序、干净树空态、点击行落 WorkingTree diff target））。
   archive：commit/tag/branch 右键「Archive to ZIP…」→ 平台保存对话框（`prompt_for_new_path` 预填 `archive-<短ref>.zip`，即确认步骤）→ `git archive --format=zip --output=<dest> <rev>`；走 ExportPatch 同款 RepoCommand 管线（命令日志 + 成功 toast「归档已导出 → 路径」），revision 经 `validate_ref_like_arg` 防注入，缺扩展名时补 `.zip`。
6. **统计窗口** `M` ✅（2026-08-27）
   周/月/年贡献者排行 + 图表，GPUI 自绘。实现：数据层走 assume-unchanged 同款懒加载管线——trait `contributor_commits_since(since)`（gix = 单次 `git log --branches --remotes --since=<unix> +0000 --pretty=%an%x1f%ct%x1e` 子进程 + 客户端精确复滤，对齐 C# `--since` 语义与 author_email_map 模式），`statistics: Loadable<Arc<Vec<ContributorCommit>>>` + `statistics_rev`，`Msg::LoadRepoStatistics`（门控 `open == Ready`，窗口 400 天覆盖任意时区一整年），开弹窗即请求、失败重开重试。分桶为纯函数 `view/statistics.rs::build_statistics_model`：全部整数历法运算（Hinnant civil_from_days/days_from_civil、周日开头 weekday、闰月规则），经既有 `Timezone::offset_seconds_at` 逐提交解析显示时区（SystemLocal 走 jiff 保持 DST 正确），窗口外（含未来时钟偏移）提交不入图不入排行；贡献者按次数降序、同名按字典序稳定。弹窗 `PopoverKind::Statistics`（指纹 discriminant 111 + rev/loadable 哈希；palette「Statistics…」居中打开）：周期页签 Week/Month/Year（host 本地字段、开弹窗重置 Week、点击不重开弹窗）、摘要行、canvas 自绘柱状图（零桶画基线短线保持轴可读，图容器 debug selector 携带周期名）、桶标签行（周=星期、月=日期数字、年=短月名）、贡献者排行（名次 + 截断名 + 相对第一名比例条 + 等宽计数，240px 滚动上限）。测试：state +1（加载/回复全状态机）、模型单测 +8（历法往返、周/月/年分桶、时区跨日、排序稳定性、空轴）、UI +5（渲染、页签点击换图、Loading 隐藏、NotLoaded 触发加载、空窗保轴）；基线 state 752 / ui-gpui 3342。

---

## 迭代 03 — v0.4.0「发现力 + 平台联动」（6–8 周）

**目标**：历史发现力补齐竞品标配，平台联动从 C# 验证过的最小形态起步。

1. **GitLab 建 MR / GitHub 建 PR** `S/M` ✅（2026-08-27，GitLab 侧）
   推送弹窗加 push-option 区：GitLab 四选项（目标分支、流水线成功即合并、删源分支、建 MR 分支）直接照 C# 交互；GitHub 侧走 gh CLI 或 API 对应最小闭环。
   实现于 `MergeRequestPushOptions`（core）→ `push_merge_request_with_output`（gix：`git push -o merge_request.create [-o merge_request.target=…] [-o merge_request.merge_when_pipeline_succeeds] [-o merge_request.remove_source_branch] remote HEAD:refs/heads/<branch|MR/branch>`，远程/分支按上游→首选远程解析，目标分支过 `validate_ref_like_arg`）→ `RepoCommandKind::PushMergeRequest` 完整管线（不接 PullAndRetry 重试状态机——重试会丢 options）→ Push 菜单「Push with merge request…」开 `PopoverKind::MergeRequestPushPrompt`（目标分支输入 + 三开关，defaults 照 C#：删源分支默认开）。GitHub 侧（gh CLI/API 最小闭环）顺延与迭代 03-6 机动项合并评估。测试：state 753 / ui-gpui 3345 / gix remote_management +2（bare 远程需 `receive.advertisePushOptions=true` 才收 push options；`ls-remote` 裸 pattern 是尾匹配，断言要用全限定 `refs/heads/…`）。
2. **bisect** `M` ✅（2026-08-27）
   start / good / bad / skip / reset 全流程；脏工作区防护复用现有 busy-gating；历史图高亮候选区间。
   实现：core `BisectState`（original_branch/bad/good/skipped/current）+ `BisectVerdict`；gix 解析经实测校准——`git bisect log` 的 `# good:`/`# bad:` 摘要行携带 RESOLVED sha（replay 行重复用户原始 term，不可解析），`.git/BISECT_START` 首行 = 原分支（跨 mark 稳定）；三条命令 `bisect_start_with_output`/`_mark_`/`_reset_` 走 RepoCommand 全管线（无 auth 槽位——本地命令且从不签名；不在 force-push-lease 清除列表——从不移动分支 ref；在 diff 失效列表——移动 HEAD 检出）。状态加载走独立 `BISECT_STATE` 位（1<<19），主刷新快路径与逐 flag 回退两处都请求+发射。UI：动作栏 bisect 条（会话中显示；`current == bad` 即无候选——good 为空 = 等待 good 锚点（提示语），非空 = 收敛（显示首个坏提交 sha）并禁用标记按钮；Bad/Good/Skip 标 HEAD、Reset 回原分支）+ 提交右键（无会话：「从这里开始二分排查（标记为坏）…」仅给坏端——实测 bare start 后只标 bad 时 git 不检出任何候选（"waiting for good commit(s)"，HEAD 停在坏 tip），good 锚点须从另一提交菜单补标；会话中：good/bad/skip 三项标任意提交；条目排在「Open diff」之后，保住 Enter 默认动作）+ 历史行 sha 左侧 verdict 徽章（✓/✗/⊘）与候选 ◆ 标记。测试：state 755 / ui-gpui 3349 / gix bisect_integration +3（收敛循环须先补显式 good 锚点再迭代标候选）。
3. **操作级 Undo** `M` ✅（2026-08-27）
   reflog 面板加「撤销上次操作」：reset / merge / rebase / pull 的反向操作 + 安全预览。
   实现：纯组合而非新命令管线——core 新增纯分类器 `undo.rs::classify_undo(&[ReflogEntry]) -> Option<UndoPlan>`（最新条目起判：`commit: `/`commit (merge): `→Soft 回退、`merge `/`pull: `→Mixed、`reset: moving to `→Hard；`rebase` 开头须走完整 run——跳过全部 rebase 系条目取首个非 rebase 条目（run 结束前一条是新链最后一个 pick，错目标），窗口耗尽→不可撤销；两条目以下、checkout/stash/`commits: batch` 等一律 None），UI 层 `resolve_undo` 加进行中优先级：merge 待结论（`merge_commit_message Ready(Some)`）→ AbortMerge、sequencer/rebase 在飞 → AbortRebase（`Msg::RebaseAbort` 兼走 cherry-pick abort），否则才看 reflog 分类。弹窗 `PopoverKind::UndoLastActionPrompt`（指纹 discriminant 113 + reflog/merge/rebase 状态哈希；懒加载：reflog NotLoaded/Error 时开弹窗即 `Msg::LoadReflog`）三态：Abort 确认卡（直接发既有 `Msg::MergeAbort`/`Msg::RebaseAbort`）/ ResetBack 安全预览（操作描述 + 回到 sha + 三枚模式 chip（statistics 页签样式，默认=按操作类型的建议模式，可切换）+ 模式说明 + 完整 `git reset --{mode} <sha>` 命令预览，Hard 红钮）/ Nothing 说明卡。入口：reflog 面板头部 Undo 常驻按钮（无物可撤销时禁用+tooltip，不闪没）+ 命令面板「Undo Last Action…」居中打开。模块可见性：`panels/mod.rs` 以 `pub(in crate::view) use` 把 resolver 从私有 popover 树递出给 panes（照 benchmark 先例）。测试：core +7（分类表、rebase run 目标、窗口耗尽）、ui-gpui +10（resolver 单测 5、弹窗四态 + 模式切换 + 默认 Mixed 落库、面板头部按钮位次）；基线 state 755 / ui-gpui 3359 / core 524。
4. **历史视图 ref/tag 过滤器** `S/M` ✅（2026-08-27）
   按分支/tag 过滤提交列表（C# `Repository.Filters` 模式），补足现有 5 种 HistoryMode。
   实现语义刻意取「限制」而非 C# 的并集——历史 = 仅从所选 ref 可达的提交（多选为并集可达），与 5 种 HistoryMode 正交组合（FirstParent + refs 同样生效）。统一全限定名（`refs/heads/…`、`refs/remotes/<remote>/…`、`refs/tags/…`；C# 用裸名，这里与 walk 解析口径一致）。不可解析的 ref 使整个 walk 报错（同 `git log <gone>`），不自动清理——陈旧过滤器在弹窗里显示为 warning 色「已不存在」行，可单击移除或 Clear 清空。数据层：`Effect::LoadLog { refs }` 贯穿 `log_history_mode_refs_page_streaming`（gix，refs 先解析成 tip 集再走既有排序/分页/取消管线）；`Msg::SetHistoryRefFilters` reducer 规范化（排序 + 去重，集合不变则 no-op）并重启 walk；会话持久化 `persist_repo_history_ref_filters_to_path`（空集即删条目，不留墓碑）。UI：历史列头分支格内 mode 下拉旁的 funnel 图标（新 `filter.svg`，激活时 accent 色 + 计数徽章 `history_ref_filter_count_<n>`，tooltip 带计数；无 repo 时降透明度禁点）→ `PopoverKind::HistoryRefFilter`（discriminant 114；指纹哈希 filters + 三列 ref 表 rev，toggle 即重绘）：本地/远程/tag 三节（节内按标签排序，空节不渲染标题）、16px 复选行（等宽截断路径 + 全文 tooltip，弹窗常开逐击刷新——C# 侧栏 toggle 语义）、240px 封顶滚动列表、活动时头部 Clear、加载/错误态走三列 Loadable。测试：gix log_integration +4（可达性/远程+tag ref/不可解析报错/FirstParent 组合）、state +3（in-flight 切换重启 walk、规范化+no-op、会话往返）、ui-gpui +9（rows 组合/missing/toggle 单测 3 + 弹窗渲染/切换/missing 行 GPUI 3 + 列头计数/无徽章/常亮 3）；基线 state 758 / ui-gpui 3368 / core 524。
5. **AI hunk 解释** `M` ✅（2026-08-27）
   diff 右键「解释这段改动」，复用迭代 01 的 provider 矩阵。
   实现：hunk 右键菜单尾追加「Explain this change」（sparkle.svg，永远可用——未配置源在点击时 toast `misc.ai_commit.not_configured`，与 ✨ 按钮同一「点击时守卫、不禁用按钮」哲学）→ `ContextMenuAction::ExplainHunk { repo_id, src_ix }` → `start_hunk_explanation`：先捕获该 hunk 的 unified patch 快照（`build_unified_patch_for_hunk_src_ix`）再开 `PopoverKind::HunkExplanation { repo_id, src_ix }`（discriminant 115，DIALOG_540_WIDTH）于菜单锚点并发请求。快照语义：diff 之后可重载（stage 切换会移动所有行），答案解释的是开窗时的快照，Retry 重发存量快照而非从活 diff 重派生——否则会悄悄解释另一个 hunk。状态宿主持有（`hunk_explanation: Option<HunkExplanation{patch, phase}>`，不进 kind；打开即 reset，同 statistics_period 先例；无指纹状态依赖，cx.notify() 重绘即可）。三态：Generating 占位 / Ready 逐行 div 渲染 + 280px 封顶滚动 / Error danger 色 + Retry；头部 mono 摘要行 = `patch_summary`（`+++ b/` 文件名、缺失时 `diff --git a/… b/…` 回退 + 首个 `@@` 行）。生成侧抽出共享 `generate_from_source(settings, cli_prompt, http_request)`（CLI/HTTP 两分支原样迁移），`generate` 与新 `generate_explanation` 成薄包装；`EXPLAIN_SYSTEM_PROMPT` 要求解释意图而非逐行复述，locale（`rust_i18n::locale()`）写进 user content 强制本地语言，复用 truncate_diff 4000 上限与 parse_response/sanitize。零 state 层改动（无新 Msg/Effect——patch 在 UI 层从既有 diff 状态构建，AI 设置进程全局）。测试：ai_commit +3（locale+patch 携带、长补丁截断、system prompt 换装）、popover 单测 +2（summary 两种头型+空补丁）、GPUI +3（菜单条目与 action、开窗生成→落稿正文、错误态 + Retry 重发同一快照且请求计数 = 2）；基线 ui-gpui 3376（state 758 / core 524 不变）。顺延项：流式输出与请求取消（矩阵是单发，暂不做）。
6. **（机动）GitHub 集成 v1** `L` ✅（2026-08-27）
   PR 列表 + checkout PR ref + checks/CI 状态 chip；permalink 基建可扩展为 API 客户端。
   实现：核心解耦为「store 持有、UI 驱动」——`RepoState.pull_requests: Loadable<Arc<Vec<PullRequest>>>`（`pull_requests_rev` 供指纹失效）进状态树，但加载是 UI 侧异步（`view/github.rs` API 客户端经 gpui `http_client` `get_json`，结果以 `InternalMsg::PullRequestsLoaded/PullRequestChecksLoaded` 回流公开 `store.dispatch`；state effect 线程无 HTTP，所有外呼只在 ui-gpui——与 AI CLI 同一约束）。token 只读 `read_gh_token`（gh hosts.yml → GH_TOKEN/GITHUB_TOKEN，复用 AI 源矩阵），无手动 PAT（顺延）。slug 解析复用 permalink 的 `parse_remote_url`（仅 host==github.com，https/ssh 皆可，剥 `.git`），非 GitHub 远程整节不渲染（含折叠栏图标按 repo 隐藏）。侧栏 PR 节位于 Remote 与 Worktrees 之间，**默认折叠**——只有显式展开（或折叠栏 popover 打开）才发请求，隐私/限流友好；`fetch_pull_requests_now` 带 in-flight 防抖（settled 即释放，丢包不钉死 Loading），测试构建 `#[cfg(not(test))]` 剔除网络 spawn。CI chip = head sha 的 combined status（`total_count==0` → 无 chip——纯 Actions 仓库不显示），checks 逐 PR 拉取且 token 门控（匿名 60/h 共享限流）+ 20 条封顶。交互：双击开 web 链接、右键 `PopoverKind::PullRequestMenu`（discriminant 116：Checkout / Open in web browser / Copy link，无 slug 时链接项自动消失；`model_for_pull_request` 纯半拆出以绕开 PopoverHost 不可单测构造），checkout 走 Stage 1 的 `Msg::CheckoutPullRequest` 本地动作管线（gix fetch `pr/N` + checkout）。测试：state +4（Loaded/Checks reducer、会话往返）、ui-gpui +14（github 客户端 6、菜单模型 3、行构建 4、GPUI 渲染 1：折叠/展开/chip 选择器）、gix status_integration +2；基线 state 762 / ui-gpui 3390 / core 524。顺延项：手动 PAT 设置字段、GHE host、免 token statuses、check-runs 汇总 API、GitHub 侧建 PR（与 03-1 的 gh 闭环合并评估进迭代 04）。

---

## 迭代 04 — v0.5.0「扩展生态 + agent 工作台」（6–8 周）

**目标**：从「功能完备」走向「可扩展」——插件化与 agent 工作流。

1. **扩展系统 v1** `L` ⏸（2026-08-28 经决策搁置，后续迭代再启）
   借鉴 C# `IPlugin` 的接口形状（Name/OnActivate/RegisterCommand/三事件），但**不能照搬反射 DLL 模型**（Rust 跨平台不适用）：先做声明式——外置配置注册命令面板条目 + 脚本命令 + RepositoryOpened/BeforeCommit/AfterCommit 钩子；进程内/WASM 留二期。
2. **agent 工作台** `L` ✅ v1（2026-08-28，核心闭环）
   claude code / codex 会话进内嵌 alacritty：启停、会话列表、per-repo-tab 工作目录；会话期间 worktree 自动快照 diff——「agent 改了什么」以标准 diff 呈现，接行级暂存做路径级接受/拒绝。
   v1 实现核心闭环：`view/agent_workbench.rs`（`AgentKind{ClaudeCode,Codex}` + PATH 可执行发现（unix 要求可执行位、拒路径形名字）+ `resolve_agent_baseline`）+ `spawn_alacritty_terminal_with_command`（PTY 直接跑 agent 命令而非用户 shell，tab 标题预置 agent 名）+ 根视图 `agent_sessions: FxHashMap<RepoId, AgentSessionState{kind, baseline}>`。启动（palette「Agent: Start Claude Code/Codex session」）时序刻意**先取基线再拉起终端**：基线 = 会话前脏树快照 `git stash create`（只造 commit 对象不动任何东西，保留用户自己的未提交改动）→ 空则 HEAD，两者都失败拒绝启动；workdir = 当前 repo tab 的 workdir（per-repo 隔离免费获得），终端宿主进既有 terminal session（无则先开）。「Agent: Show what the agent changed」= `Msg::CompareWithWorkingTree{from: baseline}`——直接复用既有 worktree 跟踪式比较的完整 diff 视图，行级/hunk 级暂存即「路径级接受」；拒绝 = 从 baseline 恢复该路径（保护用户的会话前脏改动不被一并回滚）。会话结束 = 关终端会话/repo 关闭时同步清理记录。测试：agent_workbench +3（kind 映射、可执行位/目录/路径形名字拒斥、基线三态）、palette 快照 +3 id；基线 ui-gpui 3404。顺延（v2）：会话列表面板 UI（现启停走 palette，agent tab 在终端 tab 条里）、每 repo 多 agent 并发、diff 视图内一键「从基线恢复此文件」按钮、专用 agent worktree（隔离而非共享主 worktree）、会话自动快照节奏（现按需查看）。
3. **AI PR/MR 描述生成** `M` ✅（2026-08-28）
   依赖迭代 03 的 MR/PR 创建；复用 provider 矩阵与 commit 格式示例能力。
   实现为 MR 推送弹窗的描述区（prepare-and-copy 语义——GitLab push options 没有可靠的多行 description 通道，推送不携带，生成→可编辑→Copy 粘贴进 GitLab）：`input.mr_push` 新增 ✨ Generate with AI（未配置源点击时 warning toast，与 commit ✨ 同一「点击时守卫」契约）→ `start_mr_description_generation` 后台 `git log --pretty=%h|%s target..HEAD`（50 条封顶）+ `git diff --stat target...HEAD`（UI 层经 `core::process::git_command`，smol::unblock；目标分支 = 输入框值，空则回退 `symbolic-ref refs/remotes/origin/HEAD` 解析远程默认分支，再不行报「先填目标」）→ `ai_commit::generate_mr_description`（第三个共享 `generate_from_source` 的生成器：`MR_DESCRIPTION_SYSTEM_PROMPT`（markdown：概述段 + ## Changes 分组 + 仅可推断时 ## Testing，禁止编造）+ locale 强制 + diffstat 走 truncate_diff 预算）→ `finish_mr_description_generation` 接缝（成功填入可编辑多行输入、失败行内红字、generating 防抖 + spinner；重开弹窗即重置）。目标分支名过 `target_branch_is_safe`（拒 `-` 开头/`..`/`:`/空白，仿 validate_ref_like_arg 意图）。测试：ai_commit +3（prompt 携带 target/locale/commits、超长 diffstat 截断、双 provider system prompt 换装）、纯函数 +3（log 解析与封顶/畸形记录跳过、目标解析优先级、安全校验）、GPUI +2（生成落稿与重开重置、失败行内展示）；基线 ui-gpui 3400。顺延：GitLab `merge_request.description` push-option 携带（多行经 push options 的传输/解析待实测验证）、GitHub 侧建 PR 后的描述直填。
4. **（机动）覆盖率 overlay** `M` ✅（2026-08-28）
   lcov/llvm-cov 导入 + diff 行覆盖标注；仓库自身 llvm-cov 流水线经验现成。
   实现单格式覆盖双生产者：lcov tracefile 即 llvm-cov 的 `--format=lcov` 输出（cargo llvm-cov --lcov 同源）。`core/coverage.rs` 纯解析器（`SF:`/`DA:` 行表，`normalize_coverage_path` 归一 `\`→`/`、剥 `./`；截断容错、负计数钳为 missed、无 DA 的记录丢弃、全空报错提示格式；`line_status`/`summarize`），`RepoState.coverage: Option<Arc<CoverageReport>>` 会话级持有（**有意不持久化**——大且可再生），`Msg::SetCoverage/ClearCoverage` 纯存储无 effect。导入 = palette「Import coverage file…」→ `prompt_for_paths` 原生文件选择（取消即取消，archive 同契约）→ 后台读+解析 → SetCoverage + 成功 toast（文件/行数/百分比）；失败（选错文件等）Error toast；「Clear coverage overlay」清除。标注面 = **diff 行号栏着色**（new 侧行号：covered=success 绿、missed=danger 红、无数据不动），inline 与 split 双模式接入（split 仅右列——旧侧无覆盖率语义），条件 = 已导入报告 + diff_target 有具体路径（整提交无单文件时不硬标）；折叠投影、patch split 共用 `coverage_gutter_color` 纯函数。测试：core +5（解析/截断/负计数/空报告拒绝/汇总百分比）、state +1（set/clear 无 effect + 未知 repo 容错）、ui +1（gutter 颜色三态）+ palette handler 快照注册；基线 core 529 / state 763 / ui-gpui 3401。顺延：行内整行背景着色（现仅行号栏）、分支覆盖率（BRDA）、未覆盖行汇总面板、coverage 文件拖拽导入。

---

## 迭代 05 — v0.6.0「agent 深化 + 性能基建」（4–6 周）

**目标**：把 agent 工作台从 v1 的「能跑」推进到日常主力——隔离的专用 worktree、对称的接受/拒绝；清掉大仓库日常操作的性能主项（全量 status）；GitHub PR 链路补上「建」的一环。扩展系统维持 ⏸ 搁置。

1. **agent 工作台 v2** `L` ✅（2026-08-28 三切片全部落地）
   专用 agent worktree：每会话一个 linked worktree（`<repo>/worktrees/agent-<n>`，复用既有 worktree 创建/信任管线），agent 与主工作树互不干扰，用户照常在主树工作；会话结束保留，走既有 worktree 管理 UI 合并/清理。会话列表面板（启停、切换、查看改动）；diff 视图在 agent 比较模式下补「从基线恢复此文件」按钮，接齐路径级拒绝（v1 拒绝无入口，只有手动 checkout）。
   已落地：**专用 worktree 隔离**——启动时 `git worktree add <parent>/agent-<unix秒>`（无 ref：git 自动从 HEAD 建同名分支，工作树天然干净 → 基线即创建时 HEAD，不再需要 stash-create）；终端仍宿主主 repo tab（cwd = agent worktree）；侧栏 worktree 节自动刷新可见。「查看 agent 改动」= 打开/激活 agent worktree 自己的 repo tab（工作树即 agent 的未提交改动）+ 在该 tab 派发 `CompareWithWorkingTree{from: baseline}`（打开异步 → spawn 轮询 store 快照，5s 截止）；会话记录带 `worktree_path`，关终端即结束会话、worktree 保留走既有管理 UI。diff 工具栏「从基线恢复此文件」按钮（2026-08-28 第二切片）：session 记 `worktree_repo_id`（「查看改动」打开 tab 后回填，diff 视图经它反查 session），仅当活动 diff target = 本会话的 `CommitRange{from: baseline, to: None, path: Some}` 时出现（`agent_restore_context` 纯函数判定——用户自己的比较绝不长按钮），点击 `git checkout <baseline> -- <path>` 于 agent worktree + ReloadRepo，失败 toast。第三切片 = **会话列表面板**：palette「Agent: Sessions…」开 `PopoverKind::AgentSessions`（discriminant **117**；DIALOG_440；纯启动器语义——内容每次打开从根视图 session 现算、所有动作即关弹窗，故指纹归 no-deps 组不哈希 repo 状态）：运行中会话卡片（agent 名 + worktree 路径 + baseline 短 sha + View changes / Stop session）+ 每个 agent 一条 Start 入口（PATH 扫描定 disabled）；Stop = 关 repo 终端会话（连带清 session 记录，worktree 保留）。测试：GPUI +3（有会话卡片与双动作、无会话空态提示、Stop 结束会话并关弹窗）；基线 ui-gpui 3409。**顺延**：每 repo 多会话并行（数据模型现为每 repo 单会话）。
2. **增量 status** `M/L` ◐（2026-08-28 Slice A：定向重扫与合并管线落地；monitor 路径透传待接）
   取代每次全量 worktree_status：文件系统事件收集变更路径集合（防抖合并、滤 .git 噪声）→ 只重扫受影响路径并与上次结果合并；冷启动与事件丢失兜底仍走全量。先做 watcher 选型 spike（见决策点 D5）。当前每次 status/暂存/提交触发全量重扫，是 agent 会话期间（高频外部改动）与大仓库的最大卡顿源。
   Slice A（`StatusForPaths` 管线）：core 新类型 `StatusForPaths::{Lists{unstaged,staged}, NeedsFullScan}` + trait `status_for_paths(&[PathBuf])`；gix = 单次 `git status --porcelain=v2 -z --untracked-files=all --ignore-submodules=none -- <paths>`（`--no-optional-locks`），完整 v2 解析（`1` 双列分道、`u` 冲突带 FileConflictKind 入 unstaged、`?` 未跟踪；**`2` rename/copy 记录 → NeedsFullScan**——增量合并不复刻配对，全量是诚实答案）；state = `Effect::LoadStatusForPaths` → `schedule_load_status_for_paths`（repo_load 同款 spawn/取消/缺仓回退）→ `InternalMsg::StatusForPathsLoaded` → `patch_status_for_paths` 合并（只并在 Ready 快照上：覆盖路径两道整体替换、未覆盖路径原样保留、排序镜像 gix 的 path-then-kind-priority——合并结果与全量逐字节同形；内容不变不 bump rev；NotLoaded/Error/NeedsFullScan/Err 一律回落 `LoadStatus`，全量不会再路由回来故无重试环）。测试：gix +3（与全量在覆盖路径逐项等价 + 目录 pathspec 递归 + 未触碰路径不出现 / `2` 记录 NeedsFullScan / `u` 记录冲突种类入 unstaged 且 staged 清空）、state +2（补丁替换/追加/去抖 rev、不可合并三态回落全量）；基线 core 529 / state 765。**Slice B（下轮）**：repo monitor 的防抖合并器从粗粒度 flags 扩为携带 worktree 路径集（≤阈值走 LoadStatusForPaths、超限/含 .git 噪声回落全量），激活本管线。
3. **GitHub 建 PR 轻闭环** `S`
   零 API 优先（D3 local-first 边界内）：推送后 toast 动作与 PR 节右键「Create pull request」打开 prefilled compare URL（`/{o}/{r}/compare/{base}...{head}`，新建分支回落 `/pull/new/{branch}`）；`gh` 在 PATH 时可选增强 `gh pr create --web`（带上标题/描述，衔接迭代 04 的 AI 描述生成）。
4. **桌面小件包** `S/M`（合计）
   hunk 解释流式输出 + 取消；历史 ref 过滤弹窗搜索框（接 PickerPrompt 模式）；LFS 图片 smudge 预览（`lfs_smudge_bytes` 已入 trait，差 UI）；GPG CommitDetails 签名徽章（约 55 处字面量）；clone 对话框选 SSH key。
5. **（机动）diff_view.rs 测试第一期** `M`
   4,050 行零测试的分期起点：行渲染、选择、行级暂存交互的纯函数/接缝测试骨架，优先覆盖 coverage overlay 与 agent 比较新踩过的路径。

---

## 持续轨道（跨迭代，不占席位）

| 轨道 | 内容 |
|---|---|
| 性能 | 增量 status（当前每次全量 worktree_status）；启动专项——参照 C# 四连优化的清单式做法（跳过冷启动探测、重活离 UI 线程、恢复 tab 不实例化全部仓库） |
| 质量债 | `diff_view.rs`（4,050 行零测试）分期补测试；i18n key 覆盖脚本进 CI；修 flaky |
| jj | 保持 `feature/jj-vcs` 现状不合并、不删（既定决策）；需要时另行评估，不占功能迭代 |

---

## 决策点

- **D1 · AI provider 矩阵的范围**：C# 版整套（HTTP + 4 CLI + 3 种本地凭证解析 + env + 手动）一次性移植，还是先 CLI 生成器 + gh token 两条高价值路径？建议按 C# spec 文档全量移植——规格现成，风险主要在 TOML/凭证文件解析的 Rust 重写。
- **D2 · 扩展系统形态**：C# 的反射加载 DLL 在 Rust 侧不可行；声明式命令/钩子是安全起点，进程内扩展（WASM/ABI 稳定化）成本高，二期再议。
- **D3 · GitHub 集成与 local-first 的边界**：首批主动外呼 API——默认可关闭、数据最小化、请求内容设置页可见，写进产品原则。
- **D4 · 统计窗口图表**：GPUI 无现成图表库，自绘工作量 ≈ 半个迭代小项；若排序靠后可先出纯表格版。
- **D5 · 增量 status 的 watcher 方案**：notify（跨平台 crate，FSEvents/inotify/ReadDirectoryChangesW 统一封装）+ 防抖，还是 macOS 直用 FSEvents？`.git` 内部噪声、事件合并语义、大目录树 watch 开销需 spike 实测（本仓库自身就是好基准）。
