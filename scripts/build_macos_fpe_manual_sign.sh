#!/usr/bin/env bash
# ⚠️ 不推荐用于 File Provider：产物通常无 embedded.provisionprofile，易仍报 -2001/-2014。
#    请优先: ./scripts/build_macos_fpe.sh（Xcode 自动签名 + -allowProvisioningUpdates）
#
# 两阶段：无签名构建 + 手动 codesign（仅作无 Xcode 账户时的退路）。
# 请在 **终端.app** 执行，以便钥匙串弹窗可点「始终允许」。
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT/platforms/macos/macos-file-provider-extension"

echo "==> 阶段 1: xcodebuild（CODE_SIGNING_ALLOWED=NO）…"
xcodebuild \
  -project CloudreveFileProvider.xcodeproj \
  -scheme CloudreveFPHost \
  -configuration Debug \
  -derivedDataPath ./build/DerivedData \
  clean build \
  CODE_SIGNING_ALLOWED=NO

APP="./build/DerivedData/Build/Products/Debug/CloudreveFPHost.app"
echo ""
echo "==> 阶段 2: 手动 codesign（若卡住，请到系统终端运行 manual_codesign_fpe_app.sh）…"
exec "$ROOT/platforms/macos/macos-file-provider-extension/scripts/manual_codesign_fpe_app.sh" "$APP"
