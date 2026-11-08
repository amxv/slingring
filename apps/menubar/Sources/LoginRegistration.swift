import Foundation
import ServiceManagement

// Only the bundle executable touches ServiceManagement. The visible menu bar
// invokes the Rust CLI; this command-only entrypoint makes SMAppService.mainApp
// accessible to the CLI without a second Swift settings store or login helper.
struct LoginRegistrationState {
    let name: String
    let enabled: Bool

    init(_ status: SMAppService.Status) {
        switch status {
        case .enabled: name = "enabled"; enabled = true
        case .notRegistered: name = "not_registered"; enabled = false
        case .requiresApproval: name = "requires_approval"; enabled = false
        case .notFound: name = "not_found"; enabled = false
        @unknown default: name = "unknown"; enabled = false
        }
    }

    var json: [String: Any] { ["state": name, "enabled": enabled] }
}

func runLoginItemCommand(_ argument: String) -> Int32 {
    let service = SMAppService.mainApp
    do {
        switch argument {
        case "--login-item-on":
            if service.status == .requiresApproval {
                throw NSError(domain: "Slingring", code: 1, userInfo: [
                    NSLocalizedDescriptionKey: "macOS denied the login item. Enable Slingring in System Settings → General → Login Items & Extensions."
                ])
            }
            if service.status != .enabled { try service.register() }
        case "--login-item-off":
            if service.status != .notRegistered && service.status != .notFound {
                try service.unregister()
            }
        case "--login-item-status": break
        default: return 2
        }
        let state = LoginRegistrationState(service.status)
        guard state.name != "unknown" else {
            fputs("Unknown macOS login item state\n", stderr)
            return 1
        }
        if argument == "--login-item-on" && !state.enabled {
            fputs("macOS has not enabled Slingring. Check System Settings → General → Login Items & Extensions.\n", stderr)
            return 1
        }
        if argument == "--login-item-off" && state.enabled {
            fputs("macOS did not unregister Slingring's login item\n", stderr)
            return 1
        }
        let data = try JSONSerialization.data(withJSONObject: state.json, options: [.sortedKeys])
        print(String(decoding: data, as: UTF8.self))
        return 0
    } catch {
        fputs("macOS login item operation failed: \(error.localizedDescription)\n", stderr)
        return 1
    }
}
