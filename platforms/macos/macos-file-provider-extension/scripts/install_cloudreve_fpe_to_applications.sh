#!/usr/bin/env bash
# 将 CloudreveFPHost.app 安装到 /Applications 并清除隔离属性，缓解
# NSFileProviderError ProviderTranslocated (-2002)（从下载目录/DerivedData 直接运行时常见）。
set -euo pipefail

# 本脚本位于 …/macos-file-provider-extension/scripts/ —— FPE 工程根目录为上一级
FPE="$(cd "$(dirname "$0")/.." && pwd)"
# 常见 xcodebuild -derivedDataPath 输出；可通过 SRC= 覆盖
SRC="${SRC:-}"
if [[ -z "$SRC" || ! -d "$SRC" ]]; then
  for cand in \
    "$FPE/build/Build/Products/Debug/CloudreveFPHost.app" \
    "$FPE/build/DerivedData/Build/Products/Debug/CloudreveFPHost.app"; do
    if [[ -d "$cand" ]]; then
      SRC="$cand"
      break
    fi
  done
fi
DST="/Applications/CloudreveFPHost.app"

if [[ -z "${SRC:-}" || ! -d "$SRC" ]]; then
  echo "未找到 CloudreveFPHost.app（已尝试 build/Build 与 build/DerivedData）。" >&2
  echo "请先按 platforms/macos/macos-file-provider-extension/BUILD.md 构建，或: SRC=/path/to/CloudreveFPHost.app $0" >&2
  exit 1
fi

echo "安装: $SRC -> $DST"
ditto "$SRC" "$DST"
xattr -cr "$DST"
echo "已执行 xattr -cr $DST"

LSREG="/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister"
if [[ -x "$LSREG" ]]; then
  echo "正在向 Launch Services 注册 App（有助于系统发现嵌入扩展）…"
  "$LSREG" -f -R -trusted "$DST" 2>/dev/null || "$LSREG" -f -R "$DST" || true
fi

echo "请从启动台或 open 打开后再在窗口内点「注册」。"
open "$DST" || true
