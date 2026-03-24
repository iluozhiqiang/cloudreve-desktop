import SwiftUI

/// 最小宿主：仅注册固定 smoke domain，用于验证系统是否接受嵌入的 File Provider 扩展。
@main
struct OfficialFPHostApp: App {
    var body: some Scene {
        WindowGroup {
            OfficialFPHostRootView()
        }
    }
}

private struct OfficialFPHostRootView: View {
    @State private var logText = "启动中…"
    @State private var busy = false

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Official FPE Baseline（对照工程）")
                .font(.headline)
            Text("与 Cloudreve 生产扩展独立：Bundle ID、App Group、domain id 均不同。若本工程仍报 -2001/-2014，问题在环境/账号/系统侧，而非 Cloudreve 业务代码。")
                .font(.caption)
                .foregroundStyle(.secondary)

            ScrollView {
                Text(logText)
                    .font(.system(.caption, design: .monospaced))
                    .textSelection(.enabled)
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
            .padding(8)
            .background(Color(nsColor: .textBackgroundColor))
            .cornerRadius(8)

            HStack {
                Button("创建 App Group 容器 + 注册 smoke domain") {
                    run()
                }
                .disabled(busy)

                Button("仅查询 getDomains") {
                    query()
                }
                .disabled(busy)

                Spacer()
            }
        }
        .frame(minWidth: 480, minHeight: 240)
        .padding()
    }

    private func run() {
        busy = true
        let pre = OfficialFPHostDomain.ensureAppGroupContainerExists()
        OfficialFPHostDomain.registerSmokeDomain { result in
            switch result {
            case .success:
                logText = "\(pre)\n\n✓ 已注册 domain=\(OfficialFPHostDomain.smokeDomainId)"
            case .failure(let err):
                logText = "\(pre)\n\n✗ 注册失败:\n\(OfficialFPHostDomain.describeError(err))"
            }
            busy = false
        }
    }

    private func query() {
        busy = true
        let pre = OfficialFPHostDomain.ensureAppGroupContainerExists()
        OfficialFPHostDomain.queryDomains { lines in
            logText = "\(pre)\n\n" + lines.joined(separator: "\n")
            busy = false
        }
    }
}
