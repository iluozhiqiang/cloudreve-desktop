#!/usr/bin/env bash
# 在「无签名 / ad-hoc」构建产物上，用钥匙串里的 Apple Development 证书重新签名（绕过 xcodebuild 强找「Mac Development」）。
# 需在 **macOS 终端.app** 运行，以便批准钥匙串访问；若在 Cursor 内卡住，请改用终端。
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
APP="${1:-$ROOT/build/DerivedData/Build/Products/Debug/CloudreveFPHost.app}"
APPEX="$APP/Contents/PlugIns/CloudreveFPE.appex"

if [[ ! -d "$APP" || ! -d "$APPEX" ]]; then
  echo "未找到: $APP 或嵌入扩展，请先构建。" >&2
  exit 1
fi

# 取第一个 Apple Development 身份（与 Team 一致）
IDENT="$(security find-identity -v -p codesigning 2>/dev/null | sed -n 's/.*"\(Apple Development:.*\)".*/\1/p' | head -1)"
if [[ -z "$IDENT" ]]; then
  echo "钥匙串中未找到 Apple Development 证书。请在 Xcode → Settings → Accounts → Manage Certificates 创建。" >&2
  exit 1
fi
echo "==> 使用签名身份: $IDENT"

codesign --force --sign "$IDENT" --timestamp=none \
  --entitlements "$ROOT/fpe/CloudreveFPE.entitlements" \
  "$APPEX"

codesign --force --sign "$IDENT" --timestamp=none \
  --entitlements "$ROOT/host-app/CloudreveFPHost.entitlements" \
  "$APP"

codesign --verify --verbose=2 "$APPEX"
codesign --verify --verbose=2 "$APP"
echo ""
echo "==> 验证 Team / 签名:"
codesign -dv "$APPEX" 2>&1 | grep -E '^(TeamIdentifier|Signature|Identifier)=' || true
codesign -dv "$APP" 2>&1 | grep -E '^(TeamIdentifier|Signature|Identifier)=' || true
echo "==> 完成。可执行: platforms/macos/macos-file-provider-extension/scripts/install_cloudreve_fpe_to_applications.sh"
