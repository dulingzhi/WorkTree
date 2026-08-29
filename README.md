# <img alt="WorkTree logo" src="assets/worktree_logo.svg" width="28" /> WorkTree

**English** | [简体中文](README.zh-CN.md)

[![Build Status](https://github.com/dulingzhi/WorkTree/actions/workflows/rust.yml/badge.svg?branch=main)](https://github.com/dulingzhi/WorkTree/actions/workflows/rust.yml)
[![license](https://img.shields.io/github/license/dulingzhi/WorkTree.svg)](LICENSE-AGPL-3.0)
[![latest](https://img.shields.io/github/v/release/dulingzhi/WorkTree.svg)](https://github.com/dulingzhi/WorkTree/releases/latest)
[![downloads](https://img.shields.io/github/downloads/dulingzhi/WorkTree/total)](https://github.com/dulingzhi/WorkTree/releases)

**The fastest open-source Git GUI — local-first, cross-platform, built in Rust.**

WorkTree is a free, open-source Git client for Linux, Windows, and macOS. It is written in Rust on the [GPUI](https://github.com/zed-industries/gpui) UI framework with [gix](https://github.com/GitoxideLabs/gitoxide) (gitoxide) underneath. It exists because its author could not find a tool that stayed fast and usable on codebases the size of Chromium — responsiveness on huge repositories is the bar every feature has to clear.

WorkTree is local-first: your repositories, credentials, and AI configuration stay on your machine. Nothing leaves it unless you run a remote operation or explicitly invoke an integration.

<img alt="WorkTree demo" src="assets/worktree_screenshot.png"/>

## Features

### Complete Git workflows

- Stage, unstage, and discard changes at file, hunk, or single-line level
- Commits, branches, tags, remotes, and submodules
- Fetch, pull, and push — including GitLab merge-request push options
- Stashes: by path, with keep-index / include-untracked options, and branch-from-stash
- Interactive rebase editor and cherry-pick
- Operation-level undo built on the reflog — undo resets, merges, rebases, and pulls with a safety preview
- Full bisect flow, with good / bad / skip verdicts shown right in the history graph
- First-class `git worktree` management
- Blame with incremental working-tree updates
- History graph with first-parent mode and ref / tag filters, plus cross-history commit search
- GPG signing configuration and per-commit signature badges
- Git LFS: pointer diffs, object viewing, image smudge previews, and prune
- Per-remote SSH keys, ZIP archive export of any commit, and an assume-unchanged manager
- A virtual WIP node that puts your uncommitted work inside the history graph

### Diff and merge

- Inline and side-by-side diffs with word-level highlighting
- Syntax highlighting for dozens of languages via tree-sitter
- Image diffs, LFS object previews, and inline file editing
- A real three-way merge editor with conflict-style alignment (including zdiff3) and auto-solve

### Works as your difftool / mergetool

- One command — `worktree setup` — registers WorkTree with `git difftool` / `git mergetool`
- Interactive GPUI windows when a display is available; headless, algorithm-only mode when it is not
- KDiff3- and Meld-compatible invocation forms, so it drops right in as a replacement

### GitHub & GitLab

- Pull-request list in the sidebar with CI status chips; checkout PR refs locally
- Create pull requests and merge requests via prefilled compare URLs (GitHub) or push options (GitLab)

### AI assistance — bring your own model

- Generate commit messages in the style of your recent history
- Ask for a plain-language explanation of any hunk
- Draft merge-request / pull-request descriptions, edit them, and copy them into your forge
- Providers: Claude Code, Codex, GitHub Copilot (`gh` token → GitHub Models), Gemini CLI, Ollama, generic HTTP endpoints, environment variables, or a custom command
- Credentials are discovered locally and resolved on demand — never written to disk

### Agent workbench

- Run Claude Code or Codex sessions inside WorkTree's embedded terminal
- Every session gets its own isolated git worktree, so agents never touch your working tree
- "What did the agent change?" diffs the session against its baseline — accept changes path by path, or restore files from the baseline

### A polished desktop app

- Command palette covering every action
- Embedded terminal powered by Alacritty's core
- Coverage overlay: import an lcov tracefile (as emitted by `llvm-cov` / `cargo llvm-cov`) and see covered and missed lines in your diffs
- Contribution statistics with week / month / year rankings and charts
- Multiple repositories in tabs, with full session restore
- Light and dark themes, plus custom themes from JSON bundles
- English and 简体中文 interface
- Keyboard-first: every shortcut is shown inline in its context menu
- Crash logging with next-launch recovery and prefilled issue reports

### Built for huge repositories

- Incremental status updates driven by filesystem events — only changed paths are rescanned, with full-scan fallback
- Native performance throughout: Rust, GPUI rendering, gix for Git operations, mimalloc allocation
- Developed and benchmarked against very large real-world repositories

## Download

Get the latest prebuilt binaries and installers from [GitHub Releases](https://github.com/dulingzhi/WorkTree/releases).

<details>
<summary>Windows</summary>

Download the latest installer or portable ZIP from [GitHub Releases](https://github.com/dulingzhi/WorkTree/releases), or install from the Microsoft Store:

<a href="https://apps.microsoft.com/detail/XPFD182V1H793R?referrer=appbadge&mode=full" target="_blank"  rel="noopener noreferrer">
  <img src="https://get.microsoft.com/images/en-us%20dark.svg" width="200"/>
</a>

</details>

<details>
<summary>Homebrew (macOS / Linux)</summary>

```bash
brew install --cask worktree
```

On Linux, the cask installs the AppImage build. If your system cannot launch AppImages, use the APT repo, AUR package, release tarball, or `.deb` instead.

</details>

<details>
<summary>AUR (Arch Linux)</summary>

```bash
git clone https://aur.archlinux.org/worktree.git
cd worktree && makepkg -si
```

</details>

<details>
<summary>GURU (Gentoo Linux)</summary>

```bash
emerge --ask dev-vcs/worktree
```

</details>

<details>
<summary>apt (Debian / Ubuntu)</summary>

```bash
curl -fsSL https://apt.worktree.dev/worktree-archive-keyring.gpg | sudo tee /usr/share/keyrings/worktree-archive-keyring.gpg >/dev/null
curl -fsSL https://apt.worktree.dev/worktree.sources | sudo tee /etc/apt/sources.list.d/worktree.sources >/dev/null
sudo apt update
sudo apt install worktree
```

If you install a Linux tarball or Homebrew binary on Debian, Ubuntu, or WSLg instead of the official `apt` package, install the GUI runtime libraries separately:

```bash
sudo apt install libxcb1 libxkbcommon0 libxkbcommon-x11-0
```

</details>

## Requirements

WorkTree requires a local Git installation of **2.50 or newer**.

## Build from source

```bash
git clone https://github.com/dulingzhi/WorkTree.git
cd WorkTree
cargo build -p worktree --features ui-gpui,gix
cargo run -p worktree --features ui-gpui,gix -- /path/to/repo
```

Run the CI-equivalent test suite and lints:

```bash
cargo test --workspace --no-default-features --features gix
cargo clippy --workspace --no-default-features --features gix -- -D warnings
```

Workspace layout, packaging, coverage tooling, and release processes are documented in [CONTRIBUTING.md](CONTRIBUTING.md).

## Using WorkTree as a Git difftool / mergetool

WorkTree runs standalone as a diff and merge tool invoked by `git difftool` and `git mergetool`. It supports both headless (algorithm-only) and GUI (interactive GPUI window) modes.

```bash
# Configure Git globally to use WorkTree for both difftool + mergetool
worktree setup

# Remove WorkTree integration safely
worktree uninstall
```

- `--local` targets only the current repository; `--dry-run` prints the changes without applying them.
- `setup` registers both headless and GUI variants with `guiDefault=auto`, so Git picks the GUI when a display is available and falls back to headless otherwise.
- Both commands are idempotent. `uninstall` backs up and restores any user values it would otherwise overwrite.
- KDiff3 and Meld invocation forms are supported, so WorkTree is a drop-in replacement.

## Documentation

- [Keyboard shortcuts](docs/shortcuts.md) — full shortcut reference per surface
- [Themes](docs/themes.md) — file locations, schema, and example bundles
- [Contributing](CONTRIBUTING.md) — workspace layout, build, test, coverage, and release packaging

## Crash logs

WorkTree writes panic logs and abnormal-exit recovery state to:

- Linux: `$XDG_STATE_HOME/worktree/crashes/` (fallback: `~/.local/state/worktree/crashes/`)
- macOS: `~/Library/Logs/worktree/crashes/`
- Windows: `%LOCALAPPDATA%\worktree\crashes\` (fallback: `%APPDATA%\worktree\crashes\`)

On the next launch, WorkTree shows the recovered report — app version, platform, structured failure details, and a trimmed backtrace — and prints a prefilled GitHub issue URL and log path to the launching terminal. You choose whether to report or dismiss.

## Community

- [Discord](https://discord.gg/2ufDGP8RnA) — ask questions, share feedback, follow development
- [worktree.dev](https://worktree.dev) — website

## Acknowledgments

WorkTree's design and implementation draw on ideas from SourceTree, GitKraken, Zed, GPUI, KDiff3, Meld, GitHub Desktop, Git, Gix, Rust, Smol, and many more.

This project has been created with the help of AI tools, including OpenAI Codex and Claude Code.

## License

WorkTree is licensed under the GNU Affero General Public License Version 3 (AGPL-3.0-only). See [LICENSE-AGPL-3.0](LICENSE-AGPL-3.0).

Copyright (C) 2026 AutoExplore Oy
Contact: info@autoexplore.ai
