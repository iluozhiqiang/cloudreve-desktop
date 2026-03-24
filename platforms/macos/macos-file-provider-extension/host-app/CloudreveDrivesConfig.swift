import Foundation

/// 与 Rust `DriveState` / `DriveConfig` 写入的 `~/.cloudreve/drives.json` 对齐的最小解析（只读 domain 注册所需字段）。
enum CloudreveDrivesConfig {
    static func defaultJsonURL() -> URL {
        FileManager.default.homeDirectoryForCurrentUser
            .appendingPathComponent(".cloudreve")
            .appendingPathComponent("drives.json")
    }

    /// 从 `drives.json` 提取可注册项：`mount_id` + 本地同步路径 + 显示名。
    static func loadRegistrationEntries(from url: URL) -> [DomainRegistrationEntry] {
        guard let data = try? Data(contentsOf: url) else { return [] }
        guard
            let obj = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
            let drives = obj["drives"] as? [[String: Any]]
        else {
            return []
        }

        var out: [DomainRegistrationEntry] = []
        for d in drives {
            let enabled = (d["enabled"] as? Bool) ?? true
            guard enabled else { continue }

            let mountId =
                (d["mount_id"] as? String)?.trimmingCharacters(in: .whitespacesAndNewlines)
                ?? (d["sync_root_id"] as? String)?.trimmingCharacters(in: .whitespacesAndNewlines)
            guard let mountId, !mountId.isEmpty else { continue }

            guard let syncPath = d["sync_path"] as? String, !syncPath.isEmpty else { continue }

            let name = (d["name"] as? String).flatMap { $0.isEmpty ? nil : $0 } ?? mountId
            out.append(DomainRegistrationEntry(mountId: mountId, displayName: name, syncPath: syncPath))
        }
        return out
    }
}

struct DomainRegistrationEntry: Hashable {
    let mountId: String
    let displayName: String
    let syncPath: String
}
