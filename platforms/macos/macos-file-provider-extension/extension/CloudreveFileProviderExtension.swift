import Foundation
import FileProvider
import UniformTypeIdentifiers

/// `NSFileProviderReplicatedExtension` 在 Swift 里是 **protocol**，实现方式：`NSObject` + 协议（不要 subclass 不存在的基类）。
final class CloudreveFileProviderExtension: NSObject, NSFileProviderReplicatedExtension {
    private let fpDomain: NSFileProviderDomain
    private let mountId: String
    private let syncRoot: String
    private let xpc: XPCClient

    required init(domain: NSFileProviderDomain) {
        self.fpDomain = domain
        self.mountId = domain.identifier.rawValue
        // `NSFileProviderDomain.userInfo` 仅在较新系统可用；注册 domain 时写入的 sync_path 由此读取。
        if #available(macOSApplicationExtension 15.0, *) {
            if let syncPath = domain.userInfo?["sync_path"] as? String {
                self.syncRoot = syncPath
            } else {
                self.syncRoot = ""
            }
        } else {
            self.syncRoot = ""
        }
        self.xpc = XPCClient()
        super.init()
    }

    func invalidate() {
        // 无长生命周期资源；后续可在此取消进行中的任务。
    }

    func enumerator(for containerItemIdentifier: NSFileProviderItemIdentifier, request: NSFileProviderRequest) throws -> any NSFileProviderEnumerator {
        let containerRel: String
        if containerItemIdentifier == .rootContainer {
            containerRel = ""
        } else {
            containerRel = CloudrevePathCodec.decode(containerItemIdentifier) ?? ""
        }
        return CloudreveEnumerator(mountId: mountId, syncRoot: syncRoot, containerRelativePath: containerRel, xpc: xpc)
    }

    func item(for identifier: NSFileProviderItemIdentifier, request: NSFileProviderRequest, completionHandler: @escaping (NSFileProviderItem?, (any Error)?) -> Void) -> Progress {
        let progress = Progress(totalUnitCount: 1)

        Task.detached {
            do {
                if identifier == .rootContainer {
                    let rootItem = CloudreveFileProviderItem(
                        itemIdentifier: .rootContainer,
                        parentItemIdentifier: .rootContainer,
                        filename: "root",
                        contentType: .folder,
                        documentSize: nil,
                        isDirectory: true,
                        category: .synced
                    )
                    completionHandler(rootItem, nil)
                    return
                }

                guard let relPath = CloudrevePathCodec.decode(identifier) else {
                    completionHandler(nil, NSError(domain: "cloudreve.fileprovider", code: 404, userInfo: nil))
                    return
                }

                let parentRel: String
                if relPath.contains("/") {
                    parentRel = String(relPath.split(separator: "/").dropLast().joined(separator: "/"))
                } else {
                    parentRel = ""
                }

                let parentAbs = self.absolutePath(parentRel)
                let placeholders = try self.xpc.fetchPlaceholders(mountId: self.mountId, path: parentAbs)
                guard let entry = placeholders.first(where: { $0.relative_path == relPath }) else {
                    completionHandler(nil, NSError(domain: "cloudreve.fileprovider", code: 404, userInfo: nil))
                    return
                }

                let itemAbs = self.absolutePath(relPath)
                let categoryStr = try self.xpc.getItemState(mountId: self.mountId, path: itemAbs)
                let category = CloudreveSyncCategory(rawValue: categoryStr) ?? .error

                let filename = relPath.split(separator: "/").last.map(String.init) ?? relPath

                let parentIdentifier: NSFileProviderItemIdentifier = parentRel.isEmpty
                    ? .rootContainer
                    : CloudrevePathCodec.encode(relativePath: parentRel)

                let ut = CloudreveFileProviderItem.utType(filename: filename, isDirectory: entry.is_directory)
                let item = CloudreveFileProviderItem(
                    itemIdentifier: identifier,
                    parentItemIdentifier: parentIdentifier,
                    filename: filename,
                    contentType: ut,
                    documentSize: Int64(entry.size),
                    isDirectory: entry.is_directory,
                    category: category
                )

                completionHandler(item, nil)
            } catch {
                completionHandler(nil, error)
            }
        }

        return progress
    }

    func fetchContents(for itemIdentifier: NSFileProviderItemIdentifier, version requestedVersion: NSFileProviderItemVersion?, request: NSFileProviderRequest, completionHandler: @escaping (URL?, NSFileProviderItem?, (any Error)?) -> Void) -> Progress {
        let progress = Progress(totalUnitCount: 1)

        Task.detached {
            do {
                guard let relPath = CloudrevePathCodec.decode(itemIdentifier) else {
                    completionHandler(nil, nil, NSError(domain: "cloudreve.fileprovider", code: 404, userInfo: nil))
                    return
                }

                let parentRel: String
                if relPath.contains("/") {
                    parentRel = String(relPath.split(separator: "/").dropLast().joined(separator: "/"))
                } else {
                    parentRel = ""
                }
                let parentAbs = self.absolutePath(parentRel)
                let placeholders = try self.xpc.fetchPlaceholders(mountId: self.mountId, path: parentAbs)
                guard let entry = placeholders.first(where: { $0.relative_path == relPath }) else {
                    completionHandler(nil, nil, NSError(domain: "cloudreve.fileprovider", code: 404, userInfo: nil))
                    return
                }
                if entry.is_directory {
                    completionHandler(nil, nil, NSError(domain: "cloudreve.fileprovider", code: 400, userInfo: nil))
                    return
                }

                let size = entry.size
                guard let mgr = NSFileProviderManager(for: self.fpDomain) else {
                    completionHandler(nil, nil, NSError(domain: "cloudreve.fileprovider", code: 500, userInfo: [NSLocalizedDescriptionKey: "NSFileProviderManager unavailable"]))
                    return
                }
                let tempDir = try mgr.temporaryDirectoryURL()
                let tempURL = tempDir.appendingPathComponent("cloudreve-fetch-\(UUID().uuidString)")

                FileManager.default.createFile(atPath: tempURL.path, contents: nil)
                let handle = try FileHandle(forWritingTo: tempURL)
                defer { try? handle.close() }

                let chunk: UInt64 = 8 * 1024 * 1024
                var offset: UInt64 = 0
                while offset < size {
                    let end = min(size, offset + chunk)
                    let bytes = try self.xpc.fetchData(mountId: self.mountId, path: self.absolutePath(relPath), rangeStart: offset, rangeEnd: end)
                    try handle.seek(toOffset: offset)
                    try handle.write(contentsOf: bytes)
                    offset = end
                }

                let filename = relPath.split(separator: "/").last.map(String.init) ?? relPath
                let item = CloudreveFileProviderItem(
                    itemIdentifier: itemIdentifier,
                    parentItemIdentifier: parentRel.isEmpty ? .rootContainer : CloudrevePathCodec.encode(relativePath: parentRel),
                    filename: filename,
                    contentType: CloudreveFileProviderItem.utType(filename: filename, isDirectory: false),
                    documentSize: Int64(size),
                    isDirectory: false,
                    category: .synced
                )

                completionHandler(tempURL, item, nil)
            } catch {
                completionHandler(nil, nil, error)
            }
        }

        return progress
    }

    func createItem(basedOn itemTemplate: NSFileProviderItem, fields: NSFileProviderItemFields, contents url: URL?, options: NSFileProviderCreateItemOptions, request: NSFileProviderRequest, completionHandler: @escaping (NSFileProviderItem?, NSFileProviderItemFields, Bool, (any Error)?) -> Void) -> Progress {
        completionHandler(nil, [], false, CocoaError(.featureUnsupported))
        return Progress()
    }

    func modifyItem(_ item: NSFileProviderItem, baseVersion version: NSFileProviderItemVersion, changedFields: NSFileProviderItemFields, contents newContents: URL?, options: NSFileProviderModifyItemOptions, request: NSFileProviderRequest, completionHandler: @escaping (NSFileProviderItem?, NSFileProviderItemFields, Bool, (any Error)?) -> Void) -> Progress {
        completionHandler(nil, [], false, CocoaError(.featureUnsupported))
        return Progress()
    }

    func deleteItem(identifier: NSFileProviderItemIdentifier, baseVersion version: NSFileProviderItemVersion, options: NSFileProviderDeleteItemOptions, request: NSFileProviderRequest, completionHandler: @escaping ((any Error)?) -> Void) -> Progress {
        completionHandler(CocoaError(.featureUnsupported))
        return Progress()
    }

    private func absolutePath(_ rel: String) -> String {
        if rel.isEmpty { return syncRoot }
        return syncRoot + "/" + rel
    }
}
