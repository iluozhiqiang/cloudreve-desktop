import Foundation
import FileProvider
import UniformTypeIdentifiers

enum CloudreveSyncCategory: String {
    case cloudOnly = "CloudOnly"
    case syncing = "Syncing"
    case synced = "Synced"
    case error = "Error"
}

/// Minimal `NSFileProviderItem` + `NSFileProviderItemDecorating`（Finder 角标）。
///
/// 注意：在 App Extension 中 `typeIdentifier` 已被标记为 **unavailable**，只提供 `contentType`（UTType）。
final class CloudreveFileProviderItem: NSObject, NSFileProviderItemDecorating {
    private let category: CloudreveSyncCategory

    private let _itemIdentifier: NSFileProviderItemIdentifier
    private let _parentItemIdentifier: NSFileProviderItemIdentifier
    private let _filename: String
    private let _contentType: UTType
    private let _documentSize: Int64?

    private let _isDirectory: Bool
    private let _fileSystemFlags: NSFileProviderFileSystemFlags

    /// 由文件名/目录推断 UTType（扩展里不要用 `typeIdentifier` 字符串）。
    static func utType(filename: String, isDirectory: Bool) -> UTType {
        if isDirectory { return .folder }
        let ext = (filename as NSString).pathExtension
        if ext.isEmpty { return .data }
        return UTType(filenameExtension: ext) ?? .data
    }

    init(
        itemIdentifier: NSFileProviderItemIdentifier,
        parentItemIdentifier: NSFileProviderItemIdentifier,
        filename: String,
        contentType: UTType,
        documentSize: Int64?,
        isDirectory: Bool,
        category: CloudreveSyncCategory,
        fileSystemFlags: NSFileProviderFileSystemFlags = []
    ) {
        self._itemIdentifier = itemIdentifier
        self._parentItemIdentifier = parentItemIdentifier
        self._filename = filename
        self._contentType = contentType
        self._documentSize = documentSize
        self._isDirectory = isDirectory
        self.category = category
        self._fileSystemFlags = fileSystemFlags
    }

    var itemIdentifier: NSFileProviderItemIdentifier { _itemIdentifier }
    var parentItemIdentifier: NSFileProviderItemIdentifier { _parentItemIdentifier }
    var filename: String { _filename }

    var contentType: UTType { _contentType }

    var capabilities: NSFileProviderItemCapabilities {
        var c: NSFileProviderItemCapabilities = [.allowsReading]
        if _isDirectory {
            c.insert(.allowsDeleting)
        } else {
            c.insert(.allowsReading)
        }
        return c
    }

    var fileSystemFlags: NSFileProviderFileSystemFlags { _fileSystemFlags }

    /// `NSFileProviderItem` 在 ObjC 侧为 `NSNumber?`；用 `Int64?` 会触发 “nearly matches optional requirement” 警告。
    @objc var documentSize: NSNumber? {
        guard let s = _documentSize else { return nil }
        return NSNumber(value: s)
    }

    // MARK: - NSFileProviderItemDecorating（必须是 **属性**）

    var decorations: [NSFileProviderItemDecorationIdentifier]? {
        let decorationBase = (Bundle.main.bundleIdentifier ?? "com.cloudreve.sync").appending(".decoration")
        let identifier: String
        switch category {
        case .cloudOnly: identifier = "\(decorationBase).cloudOnly"
        case .syncing: identifier = "\(decorationBase).syncing"
        case .synced: identifier = "\(decorationBase).synced"
        case .error: identifier = "\(decorationBase).error"
        }
        return [NSFileProviderItemDecorationIdentifier(identifier)]
    }

    // MARK: - Transfer hints

    var isDownloaded: Bool {
        switch category {
        case .cloudOnly: return false
        case .syncing: return false
        case .synced: return true
        case .error: return false
        }
    }

    var isDownloading: Bool {
        category == .syncing
    }
}
