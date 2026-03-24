import Foundation
import FileProvider

/// 空目录枚举（用于验证系统能否加载扩展；不依赖 XPC / Rust）。
final class OfficialEnumerator: NSObject, NSFileProviderEnumerator {
    func enumerateItems(for observer: any NSFileProviderEnumerationObserver, startingAt page: NSFileProviderPage) {
        observer.didEnumerate([])
        observer.finishEnumerating(upTo: nil)
    }

    func enumerateChanges(for observer: any NSFileProviderChangeObserver, from syncAnchor: NSFileProviderSyncAnchor) {}

    func currentSyncAnchor(completionHandler: @escaping @Sendable (NSFileProviderSyncAnchor?) -> Void) {
        completionHandler(nil)
    }

    func invalidate() {}
}
