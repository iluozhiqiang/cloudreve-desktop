import Foundation
import FileProvider

/// 将相对路径稳定地映射到 NSFileProviderItemIdentifier。
/// 规则尽量简单：前缀 `rel:` + 逐字编码后的相对路径。
enum CloudrevePathCodec {
    private static let prefix = "rel:"

    static func encode(relativePath: String) -> NSFileProviderItemIdentifier {
        // 相对路径允许包含 `/`，Finder/系统只把 identifier 当作 token。
        return NSFileProviderItemIdentifier(prefix + relativePath)
    }

    static func decode(_ identifier: NSFileProviderItemIdentifier) -> String? {
        let raw = identifier.rawValue
        guard raw.hasPrefix(prefix) else { return nil }
        return String(raw.dropFirst(prefix.count))
    }
}

