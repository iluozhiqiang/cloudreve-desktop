#!/usr/bin/env bash
# 编译 macOS File Provider 宿主 + 扩展（不打开 Xcode）。
# 若未设置 DEVELOPMENT_TEAM，会尝试读取 ~/.cloudreve/xcode_development_team（单行 10 位 Team ID）。
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT/platforms/macos/macos-file-provider-extension"

TEAM_FILE="${HOME}/.cloudreve/xcode_development_team"
if [[ -z "${DEVELOPMENT_TEAM:-}" && -f "$TEAM_FILE" ]]; then
  DEVELOPMENT_TEAM="$(tr -d ' \n\r\t' < "$TEAM_FILE")"
  export DEVELOPMENT_TEAM
fi

EXTRA=()
if [[ -n "${DEVELOPMENT_TEAM:-}" ]]; then
  EXTRA+=(DEVELOPMENT_TEAM="$DEVELOPMENT_TEAM")
  echo "==> 使用 DEVELOPMENT_TEAM=${DEVELOPMENT_TEAM} (来源: 环境变量或 ${TEAM_FILE})"
else
  echo "==> 未设置 DEVELOPMENT_TEAM：将使用 ad-hoc 签名，File Provider 常会报 -2001/-2014。"
  echo "    请在 Xcode Accounts 登录 Apple ID 后，把 Team ID 写入: $TEAM_FILE"
  echo "    或: DEVELOPMENT_TEAM=你的TeamID $0"
fi

# -allowProvisioningUpdates：拉取/更新描述文件；对 App Groups + File Provider 通常 **必须**（否则无 embedded.provisionprofile，运行时易 -2001/-2014）
xcodebuild \
  -allowProvisioningUpdates \
  -project CloudreveFileProvider.xcodeproj \
  -scheme CloudreveFPHost \
  -configuration Debug \
  -derivedDataPath ./build/DerivedData \
  build \
  "${EXTRA[@]}"

APP="./build/DerivedData/Build/Products/Debug/CloudreveFPHost.app"
echo ""
echo "==> 构建完成: $APP"
codesign -dv "$APP" 2>&1 | grep -E '^(TeamIdentifier|Signature|flags)=' || true
if [[ -f "$APP/Contents/embedded.provisionprofile" ]]; then
  echo "==> 已嵌入 embedded.provisionprofile（宿主）；File Provider 需要此文件，勿再用「无签名构建+手动 codesign」覆盖安装。"
else
  echo "==> 警告: 未找到 Contents/embedded.provisionprofile。请在 Xcode 登录账户并为两 target 开启 Signing；勿使用 build_macos_fpe_manual_sign.sh 作为主流程。"
fi
echo "    运行: open \"$APP\""
echo "    安装到系统目录: platforms/macos/macos-file-provider-extension/scripts/install_cloudreve_fpe_to_applications.sh"
echo "    （应用会读取 ~/.cloudreve/drives.json 并尝试注册 File Provider domain）"
