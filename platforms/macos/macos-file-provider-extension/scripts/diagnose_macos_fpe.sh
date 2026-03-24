#!/usr/bin/env bash
# 自检 Cloudreve File Provider 宿主 + 扩展：签名、embedded.provisionprofile、entitlements、pluginkit。
# 用法: APP=/Applications/CloudreveFPHost.app ./scripts/diagnose_macos_fpe.sh
#       或从 cloudreve-desktop 根目录: ./platforms/macos/macos-file-provider-extension/scripts/diagnose_macos_fpe.sh
set -euo pipefail

APP="${APP:-/Applications/CloudreveFPHost.app}"
APPEX="${APP}/Contents/PlugIns/CloudreveFPE.appex"

echo "========== 1) 路径 =========="
echo "APP=$APP"
echo "APPEX=$APPEX"
if [[ ! -d "$APP" ]]; then
  echo "错误: 未找到宿主 .app。请先安装或设置 APP=路径" >&2
  exit 1
fi
if [[ ! -d "$APPEX" ]]; then
  echo "错误: 未找到嵌入扩展 CloudreveFPE.appex（应在 PlugIns 下）。" >&2
  exit 1
fi

echo ""
echo "========== 2) codesign（Team / Signature）=========="
echo "--- 宿主 ---"
codesign -dv "$APP" 2>&1 | head -20 || true
echo "--- 扩展 ---"
codesign -dv "$APPEX" 2>&1 | head -20 || true

echo ""
echo "========== 3) embedded.provisionprofile =========="
for p in "$APP/Contents/embedded.provisionprofile" "$APPEX/Contents/embedded.provisionprofile"; do
  if [[ -f "$p" ]]; then
    echo "存在: $p"
    echo "  TeamName / TeamIdentifier（自描述文件）:"
    security cms -D -i "$p" 2>/dev/null | plutil -extract TeamName raw - 2>/dev/null | sed 's/^/  TeamName: /' || true
    security cms -D -i "$p" 2>/dev/null | plutil -extract TeamIdentifier raw - 2>/dev/null | sed 's/^/  TeamIdentifier: /' || true
  else
    echo "缺失: $p  （无描述文件时 App Groups 等运行时易报 -2014）"
  fi
done

echo ""
echo "========== 4) 签名 entitlements（摘要）=========="
echo "--- 宿主 ---"
set +e
codesign -d --entitlements :- "$APP" 2>&1 | plutil -convert xml1 -o - - 2>/dev/null | head -40
codesign -d --entitlements :- "$APP" 2>&1 | head -20
echo "--- 扩展 ---"
codesign -d --entitlements :- "$APPEX" 2>&1 | plutil -convert xml1 -o - - 2>/dev/null | head -40
codesign -d --entitlements :- "$APPEX" 2>&1 | head -20
set -e

echo ""
echo "========== 5) Gatekeeper（未公证常为 rejected，开发阶段可接受）=========="
set +e
spctl -a -vv "$APP" 2>&1
set -e

echo ""
echo "========== 6) pluginkit（应能搜到 cloudreve / fpehost / fileprovider 相关）=========="
if command -v pluginkit >/dev/null 2>&1; then
  set +e
  PK_OUT="$(pluginkit -m -v 2>/dev/null | awk '/[Cc]loudreve|[Ff]pehost|[Ff]ile[Pp]rovider/ {print}')"
  set -e
  if [[ -n "$PK_OUT" ]]; then
    echo "$PK_OUT"
  else
    echo "（无匹配行：可先 Finder 右键「打开」宿主一次，再重试）"
  fi
else
  echo "pluginkit 不可用"
fi

echo ""
echo "========== 7) 建议 =========="
echo "若 Signature=adhoc 或 TeamIdentifier=not set → 设置 DEVELOPMENT_TEAM 后执行 ./scripts/build_macos_fpe.sh"
echo "若宿主无 embedded.provisionprofile → 勿用手动 ad-hoc 主流程；用 -allowProvisioningUpdates 构建"
echo "若 pluginkit 始终无 Cloudreve → 系统设置 → 扩展 → 文件提供程序 中开启；并确认 App Group 与 Xcode 一致"
echo "详细: platforms/macos/macos-file-provider-extension/TROUBLESHOOTING.md"
