#!/usr/bin/env bash
# 干净构建 official-fpe-baseline、安装到 /Applications、打印 codesign 摘要。
# 若遇「Entitlements file was modified during the build」，先删 DerivedData 再编（本脚本已包含）。
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BL="$ROOT/platforms/macos/official-fpe-baseline"
rm -rf "$BL/build/DerivedData"
DEVELOPMENT_TEAM="${DEVELOPMENT_TEAM:-}"
if [[ -z "$DEVELOPMENT_TEAM" && -f "${HOME}/.cloudreve/xcode_development_team" ]]; then
  DEVELOPMENT_TEAM="$(tr -d ' \n\r\t' < "${HOME}/.cloudreve/xcode_development_team")"
fi
if [[ -z "$DEVELOPMENT_TEAM" ]]; then
  echo "请设置 DEVELOPMENT_TEAM 或 ~/.cloudreve/xcode_development_team" >&2
  exit 1
fi
export DEVELOPMENT_TEAM
"$ROOT/scripts/build_official_fpe_baseline.sh"
APP="$BL/build/DerivedData/Build/Products/Debug/OfficialFPHost.app"
echo ""
echo "==> 安装到 /Applications …"
ditto "$APP" /Applications/OfficialFPHost.app
xattr -cr /Applications/OfficialFPHost.app
echo "==> codesign 摘要"
codesign -dv /Applications/OfficialFPHost.app 2>&1 | grep -E '^(Identifier|TeamIdentifier|Signature|flags)=' || true
codesign -dv /Applications/OfficialFPHost.app/Contents/PlugIns/OfficialFPE.appex 2>&1 | grep -E '^(Identifier|TeamIdentifier|Signature|flags)=' || true
echo ""
echo "==> 请手动打开: open /Applications/OfficialFPHost.app"
echo "    在窗口内点「创建 App Group 容器 + 注册 smoke domain」观察是否仍 -2001/-2014。"
