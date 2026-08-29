#!/usr/bin/env bash
set -euo pipefail

RUSTFLAGS="-C symbol-mangling-version=v0" cargo build -p worktree --all-targets --profile=release-with-debug
