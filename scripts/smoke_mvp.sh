#!/usr/bin/env bash
# MVP 冒烟：核心 crate 单元/集成测试 + 桌面端编译检查。
# 在 macOS 上可额外跑 platforms/macos；在其它系统上跳过（该 crate 仍可交叉编译，但此处不强求）。
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

echo "==> cloudreve-app-config"
cargo test -p cloudreve-app-config

echo "==> cloudreve-sync (含 tests/mvp_smoke.rs)"
cargo test -p cloudreve-sync

if [[ "$(uname -s)" == "Darwin" ]]; then
  echo "==> cloudreve-platforms-macos"
  cargo test -p cloudreve-platforms-macos
else
  echo "==> cloudreve-platforms-macos (skip, not macOS)"
fi

echo "==> cloudreve-desktop (check)"
cargo check -p cloudreve-desktop

echo "smoke_mvp: OK"
