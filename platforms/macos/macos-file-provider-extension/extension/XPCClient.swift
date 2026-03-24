import Foundation
import Darwin

final class XPCClient {
    enum RPCType: String {
        case getItemState = "get_item_state"
        case fetchPlaceholders = "fetch_placeholders"
        case fetchData = "fetch_data"
    }

    struct GetItemStateResponse: Decodable {
        let category: String
    }

    struct FetchPlaceholdersEntry: Decodable {
        let relative_path: String
        let is_directory: Bool
        let size: UInt64
        let created_unix: Int64
        let modified_unix: Int64
        let blob_base64: String
        let mark_in_sync: Bool
        let overwrite: Bool
    }

    struct FetchPlaceholdersResponse: Decodable {
        let placeholders: [FetchPlaceholdersEntry]
    }

    struct FetchDataResponse: Decodable {
        let data_base64: String
    }

    private let socketPath: String

    init(socketPath: String = XPCClient.defaultSocketPath()) {
        self.socketPath = socketPath
    }

    private static func defaultSocketPath() -> String {
        let home = FileManager.default.homeDirectoryForCurrentUser
        return home.appendingPathComponent(".cloudreve")
            .appendingPathComponent("macos-file-provider")
            .appendingPathComponent("xpc.sock")
            .path
    }

    private func sendRequest(_ payload: [String: Any]) throws -> Data {
        let fd = socket(AF_UNIX, SOCK_STREAM, 0)
        if fd < 0 {
            throw NSError(domain: "XPCClient", code: Int(errno), userInfo: [NSLocalizedDescriptionKey: "socket() failed"])
        }
        defer { _ = close(fd) }

        var addr = sockaddr_un()
        addr.sun_family = sa_family_t(AF_UNIX)
        // 先写入临时缓冲区再 memcpy，避免 Swift 对 `sockaddr_un` 的互斥访问/重叠写分析报错。
        var pathBuf = [CChar](repeating: 0, count: MemoryLayout.size(ofValue: addr.sun_path))
        socketPath.withCString { src in
            _ = strlcpy(&pathBuf, src, pathBuf.count)
        }
        withUnsafeMutableBytes(of: &addr) { raw in
            precondition(raw.count >= 2 + pathBuf.count)
            memcpy(raw.baseAddress!.advanced(by: 2), pathBuf, pathBuf.count)
        }

        let connectRes = withUnsafePointer(to: &addr) { ptr -> Int32 in
            ptr.withMemoryRebound(to: sockaddr.self, capacity: 1) { sockPtr in
                Darwin.connect(fd, sockPtr, socklen_t(MemoryLayout<sockaddr_un>.size))
            }
        }
        if connectRes != 0 {
            throw NSError(domain: "XPCClient", code: Int(errno), userInfo: [NSLocalizedDescriptionKey: "connect() failed"])
        }

        let json = try JSONSerialization.data(withJSONObject: payload, options: [])
        var packet = Data()
        packet.append(json)
        packet.append(0x0A)

        try packet.withUnsafeBytes { buf in
            guard let base = buf.baseAddress else { return }
            let res = Darwin.write(fd, base, buf.count)
            if res < 0 {
                throw NSError(domain: "XPCClient", code: Int(errno), userInfo: [NSLocalizedDescriptionKey: "write() failed"])
            }
        }

        var response = Data()
        var byte: UInt8 = 0
        while true {
            let res = Darwin.read(fd, &byte, 1)
            if res <= 0 { break }
            response.append(byte)
            if byte == 0x0A { break }
        }
        return response
    }

    private func jsonString(from response: Data) throws -> String {
        let s = String(decoding: response, as: UTF8.self).trimmingCharacters(in: .whitespacesAndNewlines)
        if s.isEmpty {
            throw NSError(domain: "XPCClient", code: -1, userInfo: [NSLocalizedDescriptionKey: "Empty IPC response"])
        }
        return s
    }

    func getItemState(mountId: String, path: String) throws -> String {
        let req: [String: Any] = [
            "type": RPCType.getItemState.rawValue,
            "mount_id": mountId,
            "path": path
        ]
        let data = try sendRequest(req)
        let trimmed = try jsonString(from: data)
        let decoded = try JSONDecoder().decode(GetItemStateResponse.self, from: Data(trimmed.utf8))
        return decoded.category
    }

    func fetchPlaceholders(mountId: String, path: String) throws -> [FetchPlaceholdersEntry] {
        let req: [String: Any] = [
            "type": RPCType.fetchPlaceholders.rawValue,
            "mount_id": mountId,
            "path": path
        ]
        let data = try sendRequest(req)
        let trimmed = try jsonString(from: data)
        let decoded = try JSONDecoder().decode(FetchPlaceholdersResponse.self, from: Data(trimmed.utf8))
        return decoded.placeholders
    }

    func fetchData(mountId: String, path: String, rangeStart: UInt64, rangeEnd: UInt64) throws -> Data {
        let req: [String: Any] = [
            "type": RPCType.fetchData.rawValue,
            "mount_id": mountId,
            "path": path,
            "range_start": rangeStart,
            "range_end": rangeEnd
        ]
        let data = try sendRequest(req)
        let trimmed = try jsonString(from: data)
        let decoded = try JSONDecoder().decode(FetchDataResponse.self, from: Data(trimmed.utf8))
        guard let bytes = Data(base64Encoded: decoded.data_base64) else {
            throw NSError(domain: "XPCClient", code: -2, userInfo: [NSLocalizedDescriptionKey: "Invalid base64 in fetchData response"])
        }
        return bytes
    }
}
