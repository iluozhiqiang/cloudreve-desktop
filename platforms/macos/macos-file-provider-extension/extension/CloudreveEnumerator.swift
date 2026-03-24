import Foundation
import FileProvider
import UniformTypeIdentifiers

final class CloudreveEnumerator: NSObject, NSFileProviderEnumerator {
    private let mountId: String
    private let syncRoot: String
    private let containerRelativePath: String
    private let xpc: XPCClient

    private let maxFetchChunkBytes: UInt64 = 8 * 1024 * 1024 // Must match Rust MAX_FETCH_DATA_BYTES

    init(mountId: String, syncRoot: String, containerRelativePath: String, xpc: XPCClient) {
        self.mountId = mountId
        self.syncRoot = syncRoot
        self.containerRelativePath = containerRelativePath
        self.xpc = xpc
    }

    func enumerateItems(for observer: any NSFileProviderEnumerationObserver, startingAt page: NSFileProviderPage) {
        // 文件枚举走异步，避免阻塞系统线程。
        Task.detached {
            do {
                let containerAbs = self.absolutePath(containerRelativePath: self.containerRelativePath)
                let placeholders = try self.xpc.fetchPlaceholders(mountId: self.mountId, path: containerAbs)

                let containerComps = self.containerRelativePath
                    .split(separator: "/", omittingEmptySubsequences: true)
                    .map(String.init)
                let wantDepth = containerComps.count + 1 // immediate children depth

                var items: [NSFileProviderItem] = []
                for p in placeholders {
                    // placeholders.relative_path 是相对 syncRoot 的路径（例如 "a/b.txt"）
                    let entryComps = p.relative_path.split(separator: "/", omittingEmptySubsequences: true).map(String.init)
                    if entryComps.count != wantDepth { continue }
                    if containerComps.count > 0 {
                        if Array(entryComps.prefix(containerComps.count)) != containerComps { continue }
                    }

                    let relPath = p.relative_path
                    let isDir = p.is_directory
                    let filename = entryComps.last ?? relPath

                    let itemAbs = self.absolutePath(containerRelativePath: relPath)
                    let categoryStr = try self.xpc.getItemState(mountId: self.mountId, path: itemAbs)
                    let category = CloudreveSyncCategory(rawValue: categoryStr) ?? .error

                    let itemIdentifier = CloudrevePathCodec.encode(relativePath: relPath)
                    let parentIdentifier: NSFileProviderItemIdentifier = self.containerRelativePath.isEmpty
                        ? .rootContainer
                        : CloudrevePathCodec.encode(relativePath: self.containerRelativePath)

                    let ut = CloudreveFileProviderItem.utType(filename: String(filename), isDirectory: isDir)
                    let item = CloudreveFileProviderItem(
                        itemIdentifier: itemIdentifier,
                        parentItemIdentifier: parentIdentifier,
                        filename: String(filename),
                        contentType: ut,
                        documentSize: Int64(p.size),
                        isDirectory: isDir,
                        category: category
                    )
                    items.append(item)
                }

                observer.didEnumerate(items)
                observer.finishEnumerating(upTo: nil)
            } catch {
                // 这里尽量返回“空枚举”以避免 Finder 卡死；你也可以根据错误选择 noSuchItem。
                observer.didEnumerate([])
                observer.finishEnumerating(upTo: nil)
            }
        }
    }

    // 工作集/变更目前先不做（MVP 只要首轮枚举 + 角标状态）。
    func enumerateChanges(for observer: any NSFileProviderChangeObserver, from syncAnchor: NSFileProviderSyncAnchor) {
        // no-op
    }

    func currentSyncAnchor(completionHandler: @escaping @Sendable (NSFileProviderSyncAnchor?) -> Void) {
        completionHandler(nil)
    }

    func invalidate() {
        // no-op
    }

    private func absolutePath(containerRelativePath rel: String) -> String {
        if rel.isEmpty { return syncRoot }
        return syncRoot + "/" + rel
    }
}

