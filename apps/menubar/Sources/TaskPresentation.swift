import Foundation

// Present the last observed submission state, never the model's response lifecycle.
struct TaskPresentation {
    let queue: String
    let lastAttempt: String
    let latestURL: URL?

    init(_ status: [String: Any]) {
        let active = status["active_job"] as? [String: Any]
        let count = status["active_tasks"] as? Int ?? 0
        let queued = status["queued_tasks"] as? Int ?? max(0, count - (active == nil ? 0 : 1))
        if status["status_available"] as? Bool == false {
            queue = "Service status unavailable · showing saved history"
        } else if let active, active["status"] as? String == "starting" {
            let stage = Self.phaseLabel(active["phase"] as? String)
            queue = "\(stage) · \(queued) queued"
        } else if count > 0 {
            queue = "\(count) \(count == 1 ? "task" : "tasks") queued"
        } else {
            queue = "No tasks waiting to start"
        }

        let latest = status["latest_job"] as? [String: Any]
        let finished = status["latest_finished_job"] as? [String: Any]
            ?? (Self.isTerminal(latest) ? latest : nil)
        switch finished?["status"] as? String {
        case "started":
            lastAttempt = "Last attempt: submission confirmed · response not tracked"
        case "failed":
            let code = Self.safeErrorCode(finished?["error"] as? String)
            lastAttempt = "Last attempt: not submitted\(code.map { " (\($0))" } ?? "")"
        case "uncertain":
            lastAttempt = "Last outcome uncertain · inspect ChatGPT before retrying"
        default:
            lastAttempt = "No completed submission attempts"
        }

        let confirmed = status["latest_started_job"] as? [String: Any]
            ?? (finished?["status"] as? String == "started" ? finished : nil)
        if confirmed?["status"] as? String == "started" {
            let result = confirmed?["result"] as? [String: Any]
            latestURL = Self.chatURL(result?["url"] as? String ?? confirmed?["url"] as? String)
        } else {
            latestURL = nil
        }
    }

    private static func isTerminal(_ job: [String: Any]?) -> Bool {
        ["started", "failed", "uncertain"].contains(job?["status"] as? String ?? "")
    }

    private static func safeErrorCode(_ error: String?) -> String? {
        guard let code = error?.split(whereSeparator: { $0 == ":" || $0 == ";" }).first,
              !code.isEmpty, code.count <= 64,
              code.allSatisfy({ $0.isASCII && ($0.isLowercase || $0.isNumber || $0 == "_") }) else { return nil }
        return String(code)
    }

    private static func chatURL(_ address: String?) -> URL? {
        guard let address, let components = URLComponents(string: address),
              components.scheme == "https", components.host == "chatgpt.com",
              components.user == nil, components.password == nil, components.port == nil,
              components.query == nil, components.fragment == nil,
              components.path.hasPrefix("/c/") else { return nil }
        let id = components.path.dropFirst(3)
        guard (1...128).contains(id.count),
              id.allSatisfy({ $0.isASCII && ($0.isLetter || $0.isNumber || $0 == "-") }),
              address == "https://chatgpt.com/c/\(id)" else { return nil }
        return components.url
    }

    private static func phaseLabel(_ phase: String?) -> String {
        switch phase {
        case "browser_launch": return "Launching browser"
        case "browser_retry_cleanup", "browser_retry": return "Retrying browser startup"
        case "browser_endpoint", "browser_profile_validation": return "Checking isolated browser"
        case "native_registration", "extension_loading", "native_handshake": return "Connecting browser adapter"
        case "chat_loading": return "Loading ChatGPT"
        case "chat_mode_verification": return "Verifying Chat mode"
        case "conversation_loading": return "Loading conversation"
        case "composer_ready": return "Checking composer"
        case "effort_selection": return "Verifying reasoning effort"
        case "prompt_filling", "prompt_verification": return "Preparing message"
        case "image_upload", "attachment_wait": return "Uploading attachments"
        case "send_ready": return "Ready to submit"
        case "submission_authorized": return "Confirming submission"
        case "activity_wait": return "Waiting for new ChatGPT activity"
        case "activity_confirmed": return "Submission confirmed"
        default: return "Starting task"
        }
    }
}
