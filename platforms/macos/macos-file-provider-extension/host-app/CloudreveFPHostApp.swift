import SwiftUI

/// 宿主应用：嵌入 File Provider Extension，并在启动时尝试根据 `~/.cloudreve/drives.json` 注册 domain。
@main
struct CloudreveFPHostApp: App {
    var body: some Scene {
        WindowGroup {
            HostRootView()
        }
    }
}

private struct HostRootView: View {
    @State private var logLines: [String] = ["启动中…"]
    @State private var isBusy = false

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Cloudreve File Provider 宿主")
                .font(.headline)
            Text("Finder 角标由扩展提供。若注册报错「应用程序目前无法使用」(-2001)，说明扩展尚未被系统接受；此时「系统设置 → 文件提供程序」里可能根本找不到 Cloudreve，请先按 TROUBLESHOOTING.md 修复签名/App Group（非单纯开关问题）。注册成功后再查扩展列表。完整 NSError 另写入 ~/.cloudreve/fpe-host-debug.log。")
                .font(.caption)
                .foregroundStyle(.secondary)

            ScrollView {
                VStack(alignment: .leading, spacing: 6) {
                    ForEach(Array(logLines.enumerated()), id: \.offset) { _, block in
                        Text(block)
                            .font(.system(.caption, design: .monospaced))
                            .lineLimit(nil)
                            .fixedSize(horizontal: false, vertical: true)
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .textSelection(.enabled)
                    }
                }
            }
            .padding(8)
            .background(Color(nsColor: .textBackgroundColor))
            .cornerRadius(8)

            HStack(spacing: 12) {
                Button("读取 drives.json 并注册") {
                    runRegistration()
                }
                .disabled(isBusy)

                Button("仅查询系统已注册 domain") {
                    queryDomainsOnly()
                }
                .disabled(isBusy)

                Spacer()
            }
        }
        .frame(minWidth: 520, minHeight: 260)
        .padding()
        .onAppear { runRegistration() }
    }

    private func runRegistration() {
        isBusy = true
        let pre = FileProviderDomainRegistration.ensureAppGroupContainerExists()
        logLines = [pre, "", "正在读取 ~/.cloudreve/drives.json …"]
        FileProviderDomainRegistration.registerAllFromDefaultDrivesJson { lines in
            logLines = [pre, ""] + lines
            appendDomainSnapshot()
        }
    }

    private func queryDomainsOnly() {
        isBusy = true
        let pre = FileProviderDomainRegistration.ensureAppGroupContainerExists()
        logLines = [pre, "", "正在查询 NSFileProviderManager.getDomainsWithCompletionHandler …"]
        FileProviderDomainRegistration.describeRegisteredDomains { snap in
            logLines = [pre, ""] + snap
            isBusy = false
        }
    }

    /// 注册流程结束后追加一次系统当前 domain 列表，便于对照 `fileproviderctl`。
    private func appendDomainSnapshot() {
        FileProviderDomainRegistration.describeRegisteredDomains { snap in
            logLines.append(contentsOf: ["", "---"])
            logLines.append(contentsOf: snap)
            isBusy = false
        }
    }
}
