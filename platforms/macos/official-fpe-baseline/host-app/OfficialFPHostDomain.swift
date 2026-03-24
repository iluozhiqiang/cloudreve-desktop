import Foundation
import FileProvider
import os.log

/// 与生产版 `FileProviderDomainRegistration` 同思路，但固定 **独立** App Group / domain，用于对照试验。
enum OfficialFPHostDomain {
    static let appGroupIdentifier = "group.com.cloudreve.desktop.fperef"
    /// 固定测试用 domain id（与 drives.json 无关）。
    static let smokeDomainId = "fperef-smoke-domain"

    private static let log = Logger(subsystem: "com.cloudreve.desktop.fperef.host", category: "baseline")

    static func ensureAppGroupContainerExists() -> String {
        guard let url = FileManager.default.containerURL(forSecurityApplicationGroupIdentifier: appGroupIdentifier) else {
            return "无法解析 App Group 容器 URL（\(appGroupIdentifier)）。请在 Apple Developer 创建该 Group，并在 Xcode 为宿主与扩展勾选。"
        }
        do {
            try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
            return "App Group 容器已就绪: \(url.path)"
        } catch {
            return "创建 App Group 容器失败: \(error.localizedDescription)"
        }
    }

    static func registerSmokeDomain(completion: @escaping (Result<Void, Error>) -> Void) {
        let domainId = NSFileProviderDomainIdentifier(rawValue: smokeDomainId)
        let domain = NSFileProviderDomain(identifier: domainId, displayName: "Official FPE Baseline (smoke)")
        NSFileProviderManager.add(domain) { error in
            if let error {
                completion(.failure(error))
            } else {
                completion(.success(()))
            }
        }
    }

    static func describeError(_ error: Error) -> String {
        let e = error as NSError
        var parts: [String] = [e.localizedDescription, "[\(e.domain) code=\(e.code)]"]
        if let u = e.userInfo[NSUnderlyingErrorKey] as? NSError {
            parts.append("underlying: [\(u.domain) \(u.code)] \(u.localizedDescription)")
        }
        Self.log.error("\(parts.joined(separator: " | "), privacy: .public)")
        return parts.joined(separator: "\n")
    }

    static func queryDomains(completion: @escaping ([String]) -> Void) {
        NSFileProviderManager.getDomainsWithCompletionHandler { domains, error in
            if let error {
                completion(["getDomains 失败: \(describeError(error))"])
                return
            }
            let list = domains ?? []
            if list.isEmpty {
                completion(["（空）无已注册 domain"])
                return
            }
            completion(list.map { "• \($0.identifier.rawValue) — \($0.displayName)" })
        }
    }
}
