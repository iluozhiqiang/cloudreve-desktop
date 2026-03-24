import Foundation
import FileProvider
import os.log

/// 在宿主侧调用 `NSFileProviderManager.add`，把 domain 注册进系统，Finder 才会加载嵌入的 FPE。
enum FileProviderDomainRegistration {
    /// 与扩展 Info.plist 中 `NSExtensionFileProviderDocumentGroup`、entitlements 中 App Groups 一致。
    static let appGroupIdentifier = "group.com.cloudreve.desktop.fpe"

    private static let log = Logger(subsystem: "com.cloudreve.desktop.fpehost", category: "FileProviderDomain")

    /// 若从未创建过 App Group 容器，系统有时无法完成 File Provider 扩展校验（表现为持续 -2014）。
    static func ensureAppGroupContainerExists() -> String {
        guard let url = FileManager.default.containerURL(forSecurityApplicationGroupIdentifier: appGroupIdentifier) else {
            let msg = "无法解析 App Group 容器 URL（\(appGroupIdentifier)）。请在 Xcode 为宿主与扩展勾选同一 App Groups。"
            appendDebugLog([msg])
            return msg
        }
        do {
            try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
            let ok = "App Group 容器已就绪: \(url.path)"
            appendDebugLog([ok])
            return ok
        } catch {
            let msg = "创建 App Group 容器失败: \(error.localizedDescription)"
            appendDebugLog([msg])
            return msg
        }
    }

    /// 持久化到 ~/.cloudreve/fpe-host-debug.log（界面可能被截断时仍可查）。
    static func appendDebugLog(_ lines: [String]) {
        let home = FileManager.default.homeDirectoryForCurrentUser
        let dir = home.appendingPathComponent(".cloudreve", isDirectory: true)
        do {
            try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        } catch {
            // 目录创建失败时不再写日志，避免递归
        }
        let url = dir.appendingPathComponent("fpe-host-debug.log", isDirectory: false)
        let stamp = ISO8601DateFormatter().string(from: Date())
        let body = "--- \(stamp) ---\n" + lines.joined(separator: "\n") + "\n\n"
        guard let data = body.data(using: .utf8) else { return }
        if FileManager.default.fileExists(atPath: url.path) {
            do {
                let fh = try FileHandle(forWritingTo: url)
                defer {
                    do { try fh.close() } catch { /* best-effort */ }
                }
                try fh.seekToEnd()
                fh.write(data)
            } catch {
                do {
                    try data.write(to: url, options: .atomic)
                } catch {
                    // 追加与覆盖均失败则放弃
                }
            }
        } else {
            do {
                try data.write(to: url, options: .atomic)
            } catch {
                // 首次写入失败则放弃
            }
        }
    }

    /// 打印完整 NSError，便于对照 `NSFileProviderError`（例如 -2001 ProviderNotFound、-2002 ProviderTranslocated）。
    /// 底层常见 `NSFileProviderErrorApplicationExtensionNotFound`（-2014）：系统无法加载嵌入的 `.appex`，多为签名问题。
    static func describeError(_ error: Error) -> String {
        let e = error as NSError
        let fpDomain = NSFileProviderErrorDomain
        var parts: [String] = [
            e.localizedDescription,
            "[\(e.domain) code=\(e.code)]",
            "NSError: \(String(describing: error))"
        ]
        if !e.userInfo.isEmpty {
            parts.append("userInfo=\(e.userInfo)")
        }
        if let underlying = e.userInfo[NSUnderlyingErrorKey] as? NSError {
            parts.append("NSUnderlyingError: [\(underlying.domain) code=\(underlying.code)] \(underlying.localizedDescription)")
            if underlying.domain == fpDomain, underlying.code == -2014 {
                parts.append("说明: ApplicationExtensionNotFound(-2014) — 系统仍无法把 File Provider 扩展当作合法插件（即使已签名也可能缺配置）。")
                parts.append("请确认: ① 扩展已链接 FileProvider.framework；② 宿主/扩展同一 App Group + Info.plist 含 NSExtensionFileProviderDocumentGroup；③ 使用 ./scripts/build_macos_fpe.sh（勿用无签名+手动 codesign）使 .app 内含 embedded.provisionprofile；④ 再 install 到 /Applications。")
            }
        }
        if e.domain == fpDomain {
            switch e.code {
            case -2001:
                parts.append("说明: ProviderNotFound(-2001) — 与上条底层错误一起看；若含 -2014，本质是扩展未被系统接受。")
                parts.append("修复步骤: ① Team + embedded.provisionprofile（./scripts/build_macos_fpe.sh）；② App Group 与 NSExtensionFileProviderDocumentGroup；③ 宿主启动时会创建 App Group 容器，若仍失败见 TROUBLESHOOTING.md（含 spctl/未公证说明）。")
            case -2002:
                parts.append("说明: ProviderTranslocated — 因「应用转位」(常发生在从下载目录/压缩包直接运行) 已禁用提供程序。请把 CloudreveFPHost.app 拷到 /Applications，并执行: xattr -cr /Applications/CloudreveFPHost.app")
            case -2003, -2004:
                parts.append("说明: 扩展版本与系统已注册版本不一致，可尝试退出宿主、killall Finder 后重试。")
            default:
                break
            }
        }
        let text = parts.joined(separator: "\n")
        Self.log.error("\(text, privacy: .public)")
        return text
    }

    /// 注册单个 domain（`identifier` 必须与 Rust IPC 使用的 `mount_id` 一致）。
    static func register(entry: DomainRegistrationEntry, completion: @escaping (Result<Void, Error>) -> Void) {
        let domainId = NSFileProviderDomainIdentifier(rawValue: entry.mountId)
        let domain = NSFileProviderDomain(identifier: domainId, displayName: entry.displayName)
        if #available(macOS 15.0, *) {
            domain.userInfo = ["sync_path": entry.syncPath]
        }

        NSFileProviderManager.add(domain) { error in
            if let error {
                completion(.failure(error))
            } else {
                completion(.success(()))
            }
        }
    }

    /// 读取 `drives.json` 并逐个注册；已存在的 domain 可能返回错误，会记入 `lines`。
    static func registerAllFromDefaultDrivesJson(completion: @escaping ([String]) -> Void) {
        let url = CloudreveDrivesConfig.defaultJsonURL()
        let entries = CloudreveDrivesConfig.loadRegistrationEntries(from: url)

        guard !entries.isEmpty else {
            completion(["未找到可注册项：请确认 \(url.path) 存在，且盘已启用且含 mount_id。"])
            return
        }

        var lines: [String] = ["使用配置: \(url.path)", "待注册 \(entries.count) 个 domain…"]
        let group = DispatchGroup()
        let lock = NSLock()

        for e in entries {
            group.enter()
            register(entry: e) { result in
                lock.lock()
                defer { lock.unlock() }
                switch result {
                case .success:
                    lines.append("✓ 已注册 domain=\(e.mountId) (\(e.displayName))")
                case .failure(let err):
                    let detail = Self.describeError(err)
                    lines.append("✗ domain=\(e.mountId) 失败:\n\(detail)")
                    Self.appendDebugLog(["register failure mountId=\(e.mountId)", detail])
                }
                group.leave()
            }
        }

        group.notify(queue: .main) {
            completion(lines)
        }
    }

    /// 查询当前系统里 **本应用 File Provider** 已注册的 domain（用于排查 `fileproviderctl` 里看不到 Cloudreve 的原因）。
    static func describeRegisteredDomains(completion: @escaping ([String]) -> Void) {
        NSFileProviderManager.getDomainsWithCompletionHandler { domains, error in
            var lines: [String] = ["—— 系统返回的已注册 domain（getDomains）——"]
            if let error {
                let detail = Self.describeError(error)
                lines.append("查询失败:\n\(detail)")
                Self.appendDebugLog(["getDomains failure", detail])
                completion(lines)
                return
            }
            let list = domains
            guard !list.isEmpty else {
                lines.append("（空）没有任何 File Provider domain。请确认已点击「注册」，并在 系统设置 中启用本扩展。")
                completion(lines)
                return
            }
            for d in list {
                let id = d.identifier.rawValue
                let name = d.displayName
                var extra = ""
                if #available(macOS 15.0, *), let info = d.userInfo, !info.isEmpty {
                    extra = " userInfo=\(info)"
                }
                lines.append("• id=\(id)  displayName=\(name)\(extra)")
            }
            lines.append("共 \(list.count) 个。")
            completion(lines)
        }
    }
}
