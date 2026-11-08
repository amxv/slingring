import Foundation

// Parse the CLI's macOS state; do not maintain a separate menu bar preference.
struct LoginPresentation {
    struct Entry {
        let enabled: Bool?
        let tooltip: String?

        init(_ object: [String: Any]?) {
            enabled = object?["enabled"] as? Bool
            if let error = object?["error"] as? String {
                tooltip = error
            } else if object?["state"] as? String == "requires_approval" {
                tooltip = "macOS requires approval in System Settings → General → Login Items & Extensions."
            } else if enabled == nil {
                tooltip = "Login item status unavailable; reopen Configure to retry."
            } else {
                tooltip = nil
            }
        }
    }

    let menuBar: Entry
    let service: Entry

    init?(_ output: String) {
        guard let data = output.data(using: .utf8),
              let value = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else {
            return nil
        }
        menuBar = Entry(value["menu_bar"] as? [String: Any])
        service = Entry(value["service"] as? [String: Any])
    }
}
