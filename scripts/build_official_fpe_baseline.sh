#!/usr/bin/env bash
# 构建 official-fpe-baseline（最小 File Provider 对照工程）。
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT/platforms/macos/official-fpe-baseline"

TEAM_FILE="${HOME}/.cloudreve/xcode_development_team"
if [[ -z "${DEVELOPMENT_TEAM:-}" && -f "$TEAM_FILE" ]]; then
  DEVELOPMENT_TEAM="$(tr -d ' \n\r\t' < "$TEAM_FILE")"
  export DEVELOPMENT_TEAM
fi

EXTRA=()
if [[ -n "${DEVELOPMENT_TEAM:-}" ]]; then
  EXTRA+=(DEVELOPMENT_TEAM="$DEVELOPMENT_TEAM")
  echo "==> DEVELOPMENT_TEAM=${DEVELOPMENT_TEAM}"
else
  echo "==> 未设置 DEVELOPMENT_TEAM：可写入 ~/.cloudreve/xcode_development_team 或: DEVELOPMENT_TEAM=xxx $0"
fi

xcodebuild \
  -allowProvisioningUpdates \
  -project OfficialFPEBaseline.xcodeproj \
  -scheme OfficialFPHost \
  -configuration Debug \
  -derivedDataPath ./build/DerivedData \
  build \
  "${EXTRA[@]}"

APP="./build/DerivedData/Build/Products/Debug/OfficialFPHost.app"
echo ""
echo "==> 构建完成: $APP"
codesign -dv "$APP" 2>&1 | grep -E '^(TeamIdentifier|Signature|flags)=' || true
if [[ -f "$APP/Contents/embedded.provisionprofile" ]]; then
  echo "==> 宿主已含 embedded.provisionprofile"
else
  echo "==> 警告: 无 embedded.provisionprofile，运行时易 -2014"
fi
echo "    安装: ditto \"$APP\" /Applications/OfficialFPHost.app && xattr -cr /Applications/OfficialFPHost.app"
