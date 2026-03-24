import Foundation
import FileProvider

/// Domain registration helper (host-side).
///
/// This file is intended to be compiled into either:
/// - the main macOS app, or
/// - a tiny helper CLI app launched by the main app.
///
/// It registers a File Provider domain so Finder can load the File Provider Extension and display decorations.
final class DomainRegistrar {
    static func main() {
        let env = ProcessInfo.processInfo.environment
        let mountId = env["CLOUDREVE_MOUNT_ID"] ?? ""
        let displayName = env["CLOUDREVE_DISPLAY_NAME"] ?? "Cloudreve"
        let syncPath = env["CLOUDREVE_SYNC_PATH"] ?? ""

        guard !mountId.isEmpty, !syncPath.isEmpty else {
            fputs("Missing CLOUDREVE_MOUNT_ID or CLOUDREVE_SYNC_PATH\n", stderr)
            exit(2)
        }

        let domain = NSFileProviderDomain(
            identifier: NSFileProviderDomainIdentifier(rawValue: mountId),
            displayName: displayName
        )
        if #available(macOS 15.0, *) {
            domain.userInfo = ["sync_path": syncPath]
        }

        NSFileProviderManager.add(domain) { error in
            if let error = error {
                fputs("NSFileProviderManager.add failed: \(error)\n", stderr)
                exit(1)
            }
            print("NSFileProviderManager.add success for domain=\(mountId)")
        }

        // Keep process alive for the async completion handler.
        RunLoop.current.run()
    }
}

DomainRegistrar.main()

