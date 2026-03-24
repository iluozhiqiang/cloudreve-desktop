import Foundation
import FileProvider
import UniformTypeIdentifiers

/// 最小 `NSFileProviderItem`（无角标、无装饰），与 Xcode File Provider 模板思路一致。
final class OfficialItem: NSObject, NSFileProviderItem {
    private let _itemIdentifier: NSFileProviderItemIdentifier
    private let _parentItemIdentifier: NSFileProviderItemIdentifier
    private let _filename: String
    private let _contentType: UTType
    private let _isDirectory: Bool

    init(
        itemIdentifier: NSFileProviderItemIdentifier,
        parentItemIdentifier: NSFileProviderItemIdentifier,
        filename: String,
        contentType: UTType,
        isDirectory: Bool
    ) {
        self._itemIdentifier = itemIdentifier
        self._parentItemIdentifier = parentItemIdentifier
        self._filename = filename
        self._contentType = contentType
        self._isDirectory = isDirectory
    }

    var itemIdentifier: NSFileProviderItemIdentifier { _itemIdentifier }
    var parentItemIdentifier: NSFileProviderItemIdentifier { _parentItemIdentifier }
    var filename: String { _filename }
    var contentType: UTType { _contentType }

    var capabilities: NSFileProviderItemCapabilities {
        [.allowsReading]
    }

    @objc var documentSize: NSNumber? { nil }
}
