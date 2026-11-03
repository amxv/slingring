import AppKit
import Darwin
import Foundation

private let product = "Slingring"
private let docsAddress = "https://slingring.ashray.xyz/docs/"

private func cliURL() -> URL {
    let adjacent = Bundle.main.bundleURL.deletingLastPathComponent().appendingPathComponent("slingring")
    if FileManager.default.isExecutableFile(atPath: adjacent.path) { return adjacent }
    return FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent(".local/bin/slingring")
}

private final class SingleInstanceLock {
    let descriptor: Int32
    init?() {
        let fd = Darwin.open("/tmp/slingring-menubar-\(getuid()).lock", O_CREAT | O_RDWR | O_NOFOLLOW, 0o600)
        guard fd >= 0 else { return nil }
        guard flock(fd, LOCK_EX | LOCK_NB) == 0 else { Darwin.close(fd); return nil }
        descriptor = fd
    }
    deinit { Darwin.close(descriptor) }
}

private struct CommandResult { let code: Int32; let output: String }

private final class AppDelegate: NSObject, NSApplicationDelegate, NSMenuDelegate {
    private let menu = NSMenu()
    private var status: NSStatusItem!
    private var refreshing = false
    private var remoteEnabled = false
    private var serviceRunning = false
    private var serviceInstalled = false
    private var automaticUpdates = false
    private var browserHeaded = false
    private var latestURL: URL?
    private var effortItems: [NSMenuItem] = []
    private var browserModeItems: [NSMenuItem] = []
    private let profilesMenu = NSMenu()
    private var loadingProfiles = false
    private let stateItem = NSMenuItem(title: "Open menu to read status", action: nil, keyEquivalent: "")
    private let queueItem = NSMenuItem(title: "No active tasks", action: nil, keyEquivalent: "")
    private let lastAttemptItem = NSMenuItem(title: "No completed submission attempts", action: nil, keyEquivalent: "")
    private lazy var remoteItem = item("Enable Remote Access", #selector(toggleRemote), "network")
    private lazy var serviceItem = item("Start Service", #selector(toggleService), "power")
    private lazy var loginItem = item("Start Service at Login", #selector(toggleLogin), "arrow.right.circle")
    private lazy var updatesItem = item("Automatic Updates (Daily)", #selector(toggleUpdates), "arrow.clockwise")
    private lazy var latestItem = item("Open Latest Chat", #selector(openLatest), "bubble.left")

    func applicationDidFinishLaunching(_ notification: Notification) {
        status = NSStatusBar.system.statusItem(withLength: NSStatusItem.squareLength)
        status.button?.image = NSImage(systemSymbolName: "circle.dotted.circle", accessibilityDescription: product)
        status.button?.image?.isTemplate = true
        status.button?.toolTip = product
        menu.delegate = self
        menu.autoenablesItems = false
        stateItem.isEnabled = false; queueItem.isEnabled = false; lastAttemptItem.isEnabled = false
        menu.addItem(stateItem); menu.addItem(queueItem); menu.addItem(lastAttemptItem)
        menu.addItem(.separator())
        menu.addItem(serviceItem)
        latestItem.toolTip = "Open the most recent confirmed submission. A newer task may still be starting."
        menu.addItem(latestItem)
        let configure = NSMenuItem(title: "Configure", action: nil, keyEquivalent: "")
        let settings = NSMenu()
        settings.autoenablesItems = false
        configure.submenu = settings
        menu.addItem(configure)
        let profileChoice = NSMenuItem(title: "Chrome Profile", action: nil, keyEquivalent: "")
        profilesMenu.delegate = self
        profilesMenu.autoenablesItems = false
        profileChoice.submenu = profilesMenu
        profilesMenu.addItem(NSMenuItem(title: "Open to load profiles…", action: nil, keyEquivalent: ""))
        settings.addItem(profileChoice)
        let browserMode = NSMenuItem(title: "Browser Mode", action: nil, keyEquivalent: "")
        let browserChoices = NSMenu()
        for (label, value) in [("Headless (Default)", "headless"), ("Headed (Visible Browser)", "headed")] {
            let choice = NSMenuItem(title: label, action: #selector(selectBrowserMode(_:)), keyEquivalent: "")
            choice.target = self; choice.representedObject = value
            choice.toolTip = "Applies to newly submitted tasks. Active tasks continue with their original setting."
            browserChoices.addItem(choice); browserModeItems.append(choice)
        }
        browserMode.submenu = browserChoices; settings.addItem(browserMode)
        let reasoning = NSMenuItem(title: "Default Reasoning", action: nil, keyEquivalent: "")
        let choices = NSMenu()
        for level in ["Instant", "Medium", "High", "Extra High", "Pro", "Use ChatGPT Setting"] {
            let choice = NSMenuItem(title: level, action: #selector(selectEffort(_:)), keyEquivalent: "")
            choice.target = self
            choice.representedObject = level == "Use ChatGPT Setting" ? "site" : level
            choices.addItem(choice); effortItems.append(choice)
        }
        reasoning.submenu = choices; settings.addItem(reasoning)
        settings.addItem(.separator())
        settings.addItem(remoteItem)
        settings.addItem(item("Copy Remote MCP URL", #selector(copyURL), "doc.on.doc"))
        settings.addItem(loginItem)
        settings.addItem(.separator())
        settings.addItem(item("Check for Updates…", #selector(checkUpdates), "arrow.down.circle"))
        settings.addItem(updatesItem)
        settings.addItem(item("Open Documentation", #selector(openDocs), "book"))
        let version = Bundle.main.object(forInfoDictionaryKey: "AppReleaseVersion") as? String ?? "development"
        let label = NSMenuItem(title: "Slingring \(version)", action: nil, keyEquivalent: "")
        label.isEnabled = false; settings.addItem(label)
        menu.addItem(.separator())
        menu.addItem(item("Quit Menu Bar", #selector(quit), "xmark.circle"))
        status.menu = menu
        // Deliberately no timers, background refresh, browser, or WebView.
    }

    func menuWillOpen(_ opened: NSMenu) {
        if opened === profilesMenu { refreshProfiles() }
        else if opened === menu { refresh() }
    }

    private func refreshProfiles() {
        guard !loadingProfiles else { return }; loadingProfiles = true
        run(["profile", "list", "--json"]) { [weak self] result in
            guard let self else { return }; self.loadingProfiles = false
            self.profilesMenu.removeAllItems()
            guard result.code == 0, let data = result.output.data(using: .utf8),
                  let value = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
                  let profiles = value["profiles"] as? [[String: Any]], !profiles.isEmpty else {
                let empty = NSMenuItem(title: "No profiles available — open Chrome first", action: nil, keyEquivalent: "")
                empty.isEnabled = false; self.profilesMenu.addItem(empty)
                return
            }
            for profile in profiles {
                guard let directory = profile["directory"] as? String, let name = profile["name"] as? String else { continue }
                let choice = NSMenuItem(title: "\(name) — \(directory)", action: #selector(self.selectProfile(_:)), keyEquivalent: "")
                choice.target = self; choice.representedObject = directory
                choice.state = profile["selected"] as? Bool == true ? .on : .off
                choice.toolTip = "Use this Chrome profile for new ChatGPT chats. Your regular browser stays untouched."
                self.profilesMenu.addItem(choice)
            }
        }
    }

    @objc private func selectProfile(_ sender: NSMenuItem) {
        guard let profile = sender.representedObject as? String else { return }
        change(["profile", "use", profile])
    }

    private func item(_ title: String, _ action: Selector, _ symbol: String) -> NSMenuItem {
        let value = NSMenuItem(title: title, action: action, keyEquivalent: "")
        value.target = self
        value.image = NSImage(systemSymbolName: symbol, accessibilityDescription: title)
        value.image?.isTemplate = true
        return value
    }

    private func run(_ arguments: [String], completion: @escaping (CommandResult) -> Void) {
        let executable = cliURL()
        DispatchQueue.global(qos: .utility).async {
            let process = Process()
            process.executableURL = executable; process.arguments = arguments
            process.standardInput = FileHandle.nullDevice
            let pipe = Pipe(); process.standardOutput = pipe; process.standardError = pipe
            do {
                try process.run()
                // A command-scoped deadline is not an idle polling timer.
                let deadline = DispatchWorkItem { if process.isRunning { process.terminate() } }
                DispatchQueue.global(qos: .utility).asyncAfter(deadline: .now() + 180, execute: deadline)
                let bytes = pipe.fileHandleForReading.readDataToEndOfFile()
                process.waitUntilExit(); deadline.cancel()
                let result = CommandResult(code: process.terminationStatus, output: String(decoding: bytes, as: UTF8.self))
                DispatchQueue.main.async { completion(result) }
            } catch {
                DispatchQueue.main.async { completion(CommandResult(code: 1, output: error.localizedDescription)) }
            }
        }
    }

    private func alert(_ title: String, _ message: String) {
        let panel = NSAlert(); panel.messageText = title; panel.informativeText = message
        panel.addButton(withTitle: "OK"); NSApp.activate(ignoringOtherApps: true); panel.runModal()
    }

    private func refresh() {
        guard !refreshing else { return }; refreshing = true
        run(["status", "--json"]) { [weak self] result in
            guard let self else { return }; self.refreshing = false
            guard result.code == 0, let data = result.output.data(using: .utf8),
                  let value = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else {
                self.stateItem.title = "CLI unavailable"
                self.queueItem.title = "Task status unavailable"
                self.lastAttemptItem.title = "Reopen the menu to retry status"
                return
            }
            self.serviceRunning = value["running"] as? Bool ?? false
            self.remoteEnabled = value["remote_enabled"] as? Bool ?? false
            self.serviceInstalled = value["installed"] as? Bool ?? false
            let remote = value["remote_state"] as? String ?? "offline"
            self.stateItem.title = self.remoteEnabled ? "Remote: \(remote.capitalized)" : "Local Only"
            let task = TaskPresentation(value)
            self.queueItem.title = task.queue
            self.lastAttemptItem.title = task.lastAttempt
            self.remoteItem.title = self.remoteEnabled ? "Disable Remote Access" : "Enable Remote Access"
            self.remoteItem.state = self.remoteEnabled ? .on : .off
            self.serviceItem.title = self.serviceRunning ? "Stop Service" : "Start Service"
            self.loginItem.state = self.serviceInstalled ? .on : .off
            let config = value["config"] as? [String: Any] ?? [:]
            self.browserHeaded = config["browser_headed"] as? Bool ?? false
            for choice in self.browserModeItems {
                choice.state = ((choice.representedObject as? String) == "headed") == self.browserHeaded ? .on : .off
            }
            let effort = config["default_effort"] as? String ?? "site"
            for choice in self.effortItems { choice.state = (choice.representedObject as? String) == effort ? .on : .off }
            self.latestURL = task.latestURL
            self.latestItem.isEnabled = self.latestURL != nil
        }
        run(["auto-update", "status"]) { [weak self] result in
            guard let self, result.code == 0 else { return }
            self.automaticUpdates = result.output.contains("on (every 24 hours)")
            self.updatesItem.state = self.automaticUpdates ? .on : .off
        }
    }

    private func change(_ arguments: [String]) {
        run(arguments) { [weak self] result in
            guard let self else { return }
            if result.code != 0 { self.alert("Slingring command failed", result.output) }
            self.refresh()
        }
    }
    @objc private func selectEffort(_ sender: NSMenuItem) {
        guard let effort = sender.representedObject as? String else { return }
        change(["config", "set", "default-effort", effort])
    }
    @objc private func selectBrowserMode(_ sender: NSMenuItem) {
        guard let mode = sender.representedObject as? String else { return }
        change(["config", "set", "browser-mode", mode])
    }
    @objc private func toggleRemote() { change(["remote", remoteEnabled ? "off" : "on"]) }
    @objc private func toggleService() { change(["service", serviceRunning ? "stop" : "start"]) }
    @objc private func toggleLogin() { change(["service", serviceInstalled ? "uninstall" : "install"]) }
    @objc private func toggleUpdates() { change(["auto-update", automaticUpdates ? "off" : "on"]) }
    @objc private func copyURL() {
        run(["remote", "url"]) { [weak self] result in
            guard result.code == 0 else { self?.alert("Remote access is not configured", "Use slingring remote configure first. See the remote setup guide."); return }
            NSPasteboard.general.clearContents()
            NSPasteboard.general.setString(result.output.trimmingCharacters(in: .whitespacesAndNewlines), forType: .string)
        }
    }
    @objc private func checkUpdates() {
        run(["upgrade", "--check"]) { [weak self] result in
            guard let self else { return }
            if result.code != 0 || !result.output.contains("Update available.") { self.alert("Update check", result.output); return }
            let panel = NSAlert(); panel.messageText = "Install Slingring update?"; panel.informativeText = result.output
            panel.addButton(withTitle: "Install"); panel.addButton(withTitle: "Later")
            NSApp.activate(ignoringOtherApps: true)
            if panel.runModal() == .alertFirstButtonReturn { self.change(["upgrade"]) }
        }
    }
    @objc private func openLatest() { if let latestURL { NSWorkspace.shared.open(latestURL) } }
    @objc private func openDocs() { if let url = URL(string: docsAddress) { NSWorkspace.shared.open(url) } }
    @objc private func quit() { NSApp.terminate(nil) }
}

if CommandLine.arguments.contains("--version") {
    print("Slingring \(Bundle.main.object(forInfoDictionaryKey: "AppReleaseVersion") as? String ?? "development")"); exit(0)
}
if CommandLine.arguments.contains("--check") {
    guard FileManager.default.isExecutableFile(atPath: cliURL().path) else { fputs("Companion CLI missing\n", stderr); exit(1) }
    print("Slingring menu bar and CLI are ready"); exit(0)
}
guard let singleton = SingleInstanceLock() else { exit(0) }
let app = NSApplication.shared
app.setActivationPolicy(.accessory)
private let delegate = AppDelegate()
app.delegate = delegate
withExtendedLifetime(singleton) { app.run() }
