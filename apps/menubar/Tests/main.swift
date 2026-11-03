import Foundation

func check(_ condition: @autoclosure () -> Bool, _ message: String) {
    guard condition() else { fatalError(message) }
}

let address = "https://chatgpt.com/c/confirmed-chat"
let confirmed: [String: Any] = ["status": "started", "result": ["url": address]]
let historical = TaskPresentation(["active_tasks": 0, "latest_job": confirmed])
check(historical.queue == "No tasks waiting to start", "Historical starts are not active tasks")
check(historical.lastAttempt.contains("submission confirmed"), "History describes submission evidence")
check(!historical.lastAttempt.contains("complete"), "Do not claim response completion")
check(historical.latestURL?.absoluteString == address, "Confirmed links remain accessible")

let pending = TaskPresentation([
    "active_tasks": 3, "queued_tasks": 2,
    "active_job": ["status": "starting", "phase": "chat_mode_verification"],
    "latest_job": ["status": "queued"], "latest_finished_job": confirmed,
    "latest_started_job": confirmed,
])
check(pending.queue == "Verifying Chat mode · 2 queued", "Show active phase and queue separately")
check(pending.latestURL?.absoluteString == address, "Pending jobs must not hide prior confirmed chat")

let failed = TaskPresentation([
    "active_tasks": 0,
    "latest_job": ["status": "failed", "error": "prompt_changed; nothing was submitted", "url": "https://chatgpt.com/c/not-submitted"],
])
check(failed.lastAttempt == "Last attempt: not submitted (prompt_changed)", "Show actionable pre-submit failure")
check(failed.latestURL == nil, "An unsent target is not proof of submission")

let uncertain = TaskPresentation(["latest_finished_job": ["status": "uncertain"]])
check(uncertain.lastAttempt.contains("inspect ChatGPT before retrying"), "Uncertainty requires explicit inspection")
let unavailable = TaskPresentation(["status_available": false, "active_tasks": 0])
check(unavailable.queue.contains("unavailable"), "An unresponsive service is not an empty queue")

for invalid in ["https://chatgpt.com.evil/c/id", "https://chatgpt.com/c/id?x=1", "https://chatgpt.com/c/id#x", "https://chatgpt.com/c/id/", "https://chatgpt.com/c/%69d"] {
    let presentation = TaskPresentation(["latest_started_job": ["status": "started", "url": invalid]])
    check(presentation.latestURL == nil, "Reject noncanonical latest-chat URLs")
}
print("Menu status tests passed")
