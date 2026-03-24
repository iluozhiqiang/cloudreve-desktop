#!/usr/bin/env bash
# 快速查看本机与 Cloudreve File Provider 相关的线索（不修改系统）。
set -euo pipefail
echo "==> IPC socket"
test -S "${HOME}/.cloudreve/macos-file-provider/xpc.sock" && echo "OK: ~/.cloudreve/macos-file-provider/xpc.sock" || echo "缺失（主程序可能未跑或未挂载）"

echo ""
echo "==> drives.json"
test -f "${HOME}/.cloudreve/drives.json" && echo "OK: ~/.cloudreve/drives.json" || echo "缺失"

echo ""
echo "==> fileproviderctl diagnose（前几行）"
fileproviderctl diagnose 2>&1 | head -n 25

echo ""
echo "==> pluginkit 中与 cloudreve / fpe / FileProvider 相关的行（若有）"
pluginkit -m -v 2>/dev/null | awk 'BEGIN{IGNORECASE=1} /cloudreve|fpehost|CloudreveFPE|fileprovider/ {print}' | head -n 20 || true

echo ""
echo "若 diagnose 里仍无 Cloudreve：请运行 CloudreveFPHost.app 完成注册，并在「系统设置 → 扩展 → 文件提供程序」中启用。详见 platforms/macos/macos-file-provider-extension/TROUBLESHOOTING.md"
