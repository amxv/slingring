// Opt-in disposable GitHub-hosted runner fixture. Never run against a personal Mac.
// No signals, application installation, profile discovery or security changes.
import AppKit
import CoreGraphics
import Foundation

struct CheckError: Error { let code: String }
func require(_ value: Bool, _ code: String) throws { if !value { throw CheckError(code: code) } }
func canonical(_ url: URL) -> String { url.resolvingSymlinksInPath().standardizedFileURL.path }
func pump(_ seconds: Double = 0.05) { _ = RunLoop.current.run(mode: .default, before: Date().addingTimeInterval(seconds)) }
func until(_ seconds: Double, _ condition: () -> Bool) -> Bool {
    let end = Date().addingTimeInterval(seconds)
    while !condition() && Date() < end { pump() }
    return condition()
}
final class Box<T> {
    private let lock = NSLock()
    private var stored: T?
    func put(_ value: T) { lock.lock(); stored = value; lock.unlock() }
    func get() -> T? { lock.lock(); defer { lock.unlock() }; return stored }
}
struct Spec {
    let name: String, id: String, bundle: URL, executable: URL
    static let chrome = Spec(name: "chrome", id: "com.google.Chrome",
        bundle: URL(fileURLWithPath: "/Applications/Google Chrome.app"),
        executable: URL(fileURLWithPath: "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"))
    static let cft = Spec(name: "cft", id: "com.google.chrome.for.testing",
        bundle: URL(fileURLWithPath: "/Applications/Google Chrome for Testing.app"),
        executable: URL(fileURLWithPath: "/Applications/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing"))
}
struct Evidence: Codable {
    let bundleID: String?, bundlePath: String?, executablePath: String?, launchTime: Double?
    let pid: Int32, equalNativeObject: Bool, finishedLaunching: Bool, activationPolicy: Int
    func matches(_ spec: Spec, after: Date) -> Bool {
        bundleID == spec.id && bundlePath == spec.bundle.path && executablePath == spec.executable.path
        && pid > 1 && equalNativeObject && launchTime.map { $0 >= after.timeIntervalSince1970 - 3 && $0 <= Date().timeIntervalSince1970 + 3 } == true
    }
}
func evidence(_ app: NSRunningApplication) -> Evidence {
    let current = NSRunningApplication(processIdentifier: app.processIdentifier)
    return Evidence(bundleID: app.bundleIdentifier, bundlePath: app.bundleURL.map(canonical),
        executablePath: app.executableURL.map(canonical), launchTime: app.launchDate?.timeIntervalSince1970,
        pid: app.processIdentifier, equalNativeObject: current.map { app.isEqual($0) } ?? false,
        finishedLaunching: app.isFinishedLaunching, activationPolicy: app.activationPolicy.rawValue)
}
final class Owned {
    let spec: Spec, profile: URL, started: Date
    var process: Process?, app: NSRunningApplication?
    init(_ spec: Spec, _ profile: URL) { self.spec = spec; self.profile = profile; started = Date() }
    var pid: Int32? { process?.processIdentifier ?? app?.processIdentifier }
    var gone: Bool { if let process { return !process.isRunning }; return app?.isTerminated ?? false }
    func binding() -> Evidence? {
        if app == nil, let pid { app = NSRunningApplication(processIdentifier: pid) }
        return app.map(evidence)
    }
}
final class Report {
    var mode = "unknown", status = "unproven", code = "not_started"
    var versions: [String: String] = [:]
    var steps: [[String: Any]] = []
    var cleanup: [[String: Any]] = []
    func record(_ label: String, _ owned: Owned) -> Bool {
        guard !owned.gone, let item = owned.binding(), item.matches(owned.spec, after: owned.started) else {
            steps.append(["step": label, "native_binding": "unproven"]); return false
        }
        let data = try! JSONEncoder().encode(item)
        var value = try! JSONSerialization.jsonObject(with: data) as! [String: Any]
        value["step"] = label; value["native_binding"] = "verified"
        steps.append(value); return true
    }
    func write(_ path: String) throws {
        let value: [String: Any] = ["schema": 1, "mode": mode, "status": status, "code": code,
            "steps": steps, "cleanup": cleanup, "browser_versions": versions, "scope": "application_identity_only",
            "tree_cleanup": "unproven", "candidate_helper_integration": "not_run",
            "signature_scope": "integrity_and_system_execution_policy_only", "publisher_identity": "not_independently_pinned",
            "image_version": ProcessInfo.processInfo.environment["ImageVersion"] ?? "unknown"]
        let data = try JSONSerialization.data(withJSONObject: value, options: [.prettyPrinted, .sortedKeys])
        try data.write(to: URL(fileURLWithPath: path), options: .atomic)
    }
}
func hosted(_ env: [String: String]) -> Bool {
    env["GITHUB_ACTIONS"] == "true" && env["RUNNER_ENVIRONMENT"] == "github-hosted"
    && env["RUNNER_OS"] == "macOS" && env["RUNNER_ARCH"] == "ARM64"
    && env["GITHUB_REPOSITORY"] == "amxv/slingring" && env["GITHUB_EVENT_NAME"] == "workflow_dispatch"
    && env["SLINGRING_DISPOSABLE_BROWSER_TEST"] == "explicitly-authorized"
}
func privateDirectory(_ url: URL) throws {
    try FileManager.default.createDirectory(at: url, withIntermediateDirectories: false, attributes: [.posixPermissions: 0o700])
}
func checkInstalled(_ spec: Spec) throws {
    try require(canonical(spec.bundle) == spec.bundle.path && canonical(spec.executable) == spec.executable.path, "unexpected_bundle_symlink")
    try require(FileManager.default.isExecutableFile(atPath: spec.executable.path), "preinstalled_browser_missing")
    try require(Bundle(url: spec.bundle)?.bundleIdentifier == spec.id, "bundle_metadata_mismatch")
    for (tool, arguments) in [("/usr/bin/codesign", ["--verify", "--deep", "--strict", spec.bundle.path]),
                              ("/usr/sbin/spctl", ["--assess", "--type", "execute", spec.bundle.path])] {
        let process = Process(); process.executableURL = URL(fileURLWithPath: tool); process.arguments = arguments
        process.standardInput = FileHandle.nullDevice; process.standardOutput = FileHandle.nullDevice; process.standardError = FileHandle.nullDevice
        try process.run()
        try require(until(20) { !process.isRunning } && process.terminationStatus == 0, "signature_or_gatekeeper_unproven")
    }
}
func arguments(_ profile: URL, headless: Bool) -> [String] {
    var values = ["--user-data-dir=\(profile.path)", "--no-first-run", "--no-default-browser-check",
        "--remote-debugging-port=0", "--remote-debugging-address=127.0.0.1"]
    if headless { values.append("--headless=new") }
    values.append("about:blank"); return values
}
func endpoint(_ owned: Owned) throws -> URL {
    let file = owned.profile.appendingPathComponent("DevToolsActivePort")
    try require(until(20) { FileManager.default.fileExists(atPath: file.path) || owned.gone }, "debug_endpoint_unavailable")
    try require(!owned.gone, "owned_root_exited_early")
    let data = try Data(contentsOf: file)
    try require(data.count < 4096, "debug_endpoint_invalid")
    let lines = String(decoding: data, as: UTF8.self).split(separator: "\n").map(String.init)
    try require(lines.count == 2, "debug_endpoint_invalid")
    guard let port = UInt16(lines[0]), port > 0,
          lines[1].range(of: "^/devtools/browser/[A-Za-z0-9-]+$", options: .regularExpression) != nil else { throw CheckError(code: "debug_endpoint_invalid") }
    var url = URLComponents(); url.scheme = "ws"; url.host = "127.0.0.1"; url.port = Int(port); url.path = lines[1]
    return url.url!
}
func request(_ task: URLSessionWebSocketTask, id: Int, method: String) throws -> [String: Any] {
    let data = try JSONSerialization.data(withJSONObject: ["id": id, "method": method])
    let sent = Box<Bool>()
    task.send(.string(String(decoding: data, as: UTF8.self))) { sent.put($0 == nil) }
    try require(until(3) { sent.get() != nil } && sent.get() == true, "cdp_send_unproven")
    for _ in 0..<16 {
        let received = Box<Result<URLSessionWebSocketTask.Message, Error>>()
        task.receive { received.put($0) }
        try require(until(3) { received.get() != nil }, "cdp_reply_unproven")
        let message = try received.get()!.get()
        let bytes: Data
        switch message { case .string(let text): bytes = Data(text.utf8); case .data(let data): bytes = data; @unknown default: throw CheckError(code: "cdp_reply_invalid") }
        try require(bytes.count < 65536, "cdp_reply_invalid")
        guard let value = try JSONSerialization.jsonObject(with: bytes) as? [String: Any] else { throw CheckError(code: "cdp_reply_invalid") }
        if value["id"] as? Int == id { return value }
    }
    throw CheckError(code: "cdp_reply_unproven")
}
func close(_ owned: Owned) -> Bool {
    if owned.gone { return true }
    do {
        if let app = owned.app {
            try require(evidence(app).matches(owned.spec, after: owned.started), "native_binding_changed_before_close")
        }
        let task = URLSession.shared.webSocketTask(with: try endpoint(owned)); task.resume()
        defer { task.cancel(with: .normalClosure, reason: nil) }
        let result = try request(task, id: 1, method: "SystemInfo.getProcessInfo")
        let values = (result["result"] as? [String: Any])?["processInfo"] as? [[String: Any]] ?? []
        let roots = values.filter { $0["type"] as? String == "browser" }
        try require(roots.count == 1 && (roots[0]["id"] as? NSNumber)?.int32Value == owned.pid && !owned.gone, "cdp_owned_root_mismatch")
        // Keep the same authenticated local connection. No name/PID signal fallback.
        let sent = Box<Bool>()
        task.send(.string("{\"id\":2,\"method\":\"Browser.close\"}")) { sent.put($0 == nil) }
        _ = until(3) { sent.get() != nil || owned.gone }
        return until(10) { owned.gone }
    } catch { return false }
}
func ordinaryWindow(_ owned: Owned, foreground: Bool = false) -> Bool {
    guard let pid = owned.pid else { return false }
    return until(15) {
        guard owned.app?.isFinishedLaunching == true,
              (!foreground || owned.app?.isActive == true),
              owned.app?.activationPolicy == .regular,
              let rows = CGWindowListCopyWindowInfo([.optionAll, .excludeDesktopElements], kCGNullWindowID) as? [[String: Any]] else { return false }
        return rows.contains { row in
            guard row[kCGWindowOwnerPID as String] as? Int32 == pid,
                  row[kCGWindowLayer as String] as? Int == 0,
                  let bounds = row[kCGWindowBounds as String] as? [String: Any],
                  let width = bounds["Width"] as? Double, let height = bounds["Height"] as? Double else { return false }
            return width > 0 && height > 0
        }
    }
}
func launch(_ owned: Owned, workspace: Bool, headless: Bool, environment: [String: String]) throws {
    if workspace {
        let configuration = NSWorkspace.OpenConfiguration(); configuration.activates = true
        configuration.addsToRecentItems = false; configuration.createsNewApplicationInstance = false
        configuration.promptsUserIfNeeded = false; configuration.arguments = arguments(owned.profile, headless: false)
        configuration.environment = environment
        let response = Box<NSRunningApplication>(); let failed = Box<Bool>()
        NSWorkspace.shared.openApplication(at: owned.spec.bundle, configuration: configuration) { app, error in
            if let app { response.put(app) }; if error != nil { failed.put(true) }
        }
        try require(until(20) { response.get() != nil || failed.get() != nil } && failed.get() == nil, "launch_services_unavailable")
        owned.app = response.get()
        try require(owned.app != nil, "launch_services_unavailable")
    } else {
        let process = Process(); process.executableURL = owned.spec.executable; process.arguments = arguments(owned.profile, headless: headless)
        process.environment = environment; process.standardInput = FileHandle.nullDevice
        process.standardOutput = FileHandle.nullDevice; process.standardError = FileHandle.nullDevice
        owned.process = process; try process.run()
    }
    _ = try endpoint(owned)
    _ = until(10) { owned.binding() != nil }
}
func runHosted(mode: String, report: Report) throws {
    let env = ProcessInfo.processInfo.environment
    try require(hosted(env) && NSUserName() == "runner", "not_disposable_hosted_context")
    try require(mode == "headless" || mode == "headed", "invalid_mode")
    guard let temp = env["RUNNER_TEMP"], temp.hasPrefix("/Users/runner/") else { throw CheckError(code: "runner_temp_unproven") }
    guard let session = CGSessionCopyCurrentDictionary() as? [String: Any], session[kCGSessionOnConsoleKey as String] as? Bool == true else { throw CheckError(code: "gui_session_unavailable") }
    try require(NSRunningApplication.runningApplications(withBundleIdentifier: Spec.chrome.id).isEmpty && NSRunningApplication.runningApplications(withBundleIdentifier: Spec.cft.id).isEmpty, "preexisting_browser_refuse_to_touch")
    try checkInstalled(.chrome); try checkInstalled(.cft)
    for spec in [Spec.chrome, Spec.cft] {
        guard let version = Bundle(url: spec.bundle)?.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String,
              version.range(of: "^[0-9]+(\\.[0-9]+){3}$", options: .regularExpression) != nil else { throw CheckError(code: "browser_version_unproven") }
        report.versions[spec.name] = version
    }
    try require(NSRunningApplication.runningApplications(withBundleIdentifier: Spec.chrome.id).isEmpty && NSRunningApplication.runningApplications(withBundleIdentifier: Spec.cft.id).isEmpty, "browser_started_during_preflight")
    let root = URL(fileURLWithPath: temp).appendingPathComponent("slingring-identity-\(UUID().uuidString)")
    try privateDirectory(root)
    let home = root.appendingPathComponent("home"), tmp = root.appendingPathComponent("tmp")
    try privateDirectory(home); try privateDirectory(tmp)
    let environment = ["HOME": home.path, "TMPDIR": tmp.path + "/", "PATH": "/usr/bin:/bin"]
    var owned: [Owned] = []
    defer {
        for item in owned.reversed() { report.cleanup.append(["browser": item.spec.name, "root_exit_verified": close(item), "tree_exit": "unproven"]) }
        // Profiles are never uploaded. Only runner disposal removes the test root.
    }
    func create(_ spec: Spec, _ label: String) throws -> Owned {
        let profile = root.appendingPathComponent(label); try privateDirectory(profile)
        let item = Owned(spec, profile); owned.append(item); return item
    }
    let baseline = try create(.chrome, "chrome-baseline")
    try launch(baseline, workspace: true, headless: false, environment: environment)
    try require(ordinaryWindow(baseline, foreground: true) && report.record("normal_chrome_baseline", baseline), "normal_chrome_binding_unproven")
    try require(close(baseline), "baseline_cleanup_unproven")
    let cft = try create(.cft, "cft-test")
    try launch(cft, workspace: false, headless: mode == "headless", environment: environment)
    if mode == "headed" { try require(ordinaryWindow(cft), "headed_cft_window_unproven") }
    let nativeCft = report.record("cft_running", cft)
    let during = try create(.chrome, "chrome-during")
    try launch(during, workspace: true, headless: false, environment: environment)
    try require(ordinaryWindow(during, foreground: true) && report.record("normal_chrome_while_cft_running", during), "normal_chrome_binding_unproven")
    try require(!cft.gone, "cft_exited_before_concurrent_normal_open")
    let simultaneousNative = report.record("cft_still_running_during_normal_chrome_window", cft)
    if let cftApp = cft.app, let chromeApp = during.app { try require(!cftApp.isEqual(chromeApp), "shared_native_application_object") }
    try require(close(during) && close(cft), "owned_root_cleanup_unproven")
    let after = try create(.chrome, "chrome-after")
    try launch(after, workspace: true, headless: false, environment: environment)
    try require(ordinaryWindow(after, foreground: true) && report.record("normal_chrome_after_cft_close", after), "normal_chrome_reopen_unproven")
    try require(close(after), "final_cleanup_unproven")
    try require(nativeCft && simultaneousNative, "cft_native_binding_unproven")
}
func selfTest() throws {
    try require(!hosted([:]), "guard_test_failed")
    var env = ["GITHUB_ACTIONS":"true", "RUNNER_ENVIRONMENT":"github-hosted", "RUNNER_OS":"macOS", "RUNNER_ARCH":"ARM64", "GITHUB_REPOSITORY":"amxv/slingring", "GITHUB_EVENT_NAME":"workflow_dispatch", "SLINGRING_DISPOSABLE_BROWSER_TEST":"explicitly-authorized"]
    try require(hosted(env), "guard_test_failed"); env["RUNNER_ENVIRONMENT"] = "self-hosted"; try require(!hosted(env), "guard_test_failed")
    let date = Date()
    let good = Evidence(bundleID: Spec.cft.id, bundlePath: Spec.cft.bundle.path, executablePath: Spec.cft.executable.path, launchTime: date.timeIntervalSince1970, pid: 42, equalNativeObject: true, finishedLaunching: true, activationPolicy: 0)
    try require(good.matches(.cft, after: date) && !good.matches(.chrome, after: date), "binding_test_failed")
    let missing = Evidence(bundleID: Spec.cft.id, bundlePath: Spec.cft.bundle.path, executablePath: Spec.cft.executable.path, launchTime: nil, pid: 42, equalNativeObject: false, finishedLaunching: true, activationPolicy: 0)
    try require(!missing.matches(.cft, after: date), "binding_test_failed")
    print("identity fixture self-tests passed; no browser or AppKit app lookup performed")
}
let args = CommandLine.arguments
if args.count == 2 && args[1] == "--self-test" {
    do { try selfTest(); exit(0) } catch { fputs("self_test_failed\n", stderr); exit(1) }
}
guard args.count == 4 && args[1] == "--run-hosted" else { fputs("use --self-test or --run-hosted MODE REPORT\n", stderr); exit(2) }
let report = Report(); report.mode = args[2]
do { try runHosted(mode: args[2], report: report); report.status = "passed"; report.code = "native_identity_and_normal_reopen_verified" }
catch let error as CheckError { report.code = error.code }
catch { report.code = "fixture_error_unproven" }
if report.cleanup.contains(where: { $0["root_exit_verified"] as? Bool != true }) { report.status = "unproven"; report.code = "cleanup_unproven" }
do { try report.write(args[3]) } catch { fputs("report_write_failed\n", stderr); exit(2) }
print("hosted browser identity: \(report.status) (\(report.code))")
exit(report.status == "passed" ? 0 : 2)
