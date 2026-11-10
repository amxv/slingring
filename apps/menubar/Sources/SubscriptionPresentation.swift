import Foundation

struct SubscriptionPresentation {
    let subscription: String
    let effort: String
    init(_ config: [String: Any]) {
        effort = config["default_effort"] as? String ?? "site"
        subscription = config["subscription"] as? String
            ?? (["Extra High", "Pro"].contains(effort) ? "pro" : "plus")
    }
    func allows(_ effort: String) -> Bool {
        subscription == "pro" || ["Instant", "Medium", "High", "site"].contains(effort)
    }
}
