import AppKit
import Network
import Observation
import RaptorKit

/// App-wide state: the engine, terminal settings, and which window is in front.
@MainActor
@Observable
final class AppModel {
    static let shared = AppModel()

    let model: RaptorModel
    private(set) var prefs = TerminalPrefs()
    private(set) var confirmClose = true
    /// The workspace of the most recently active window; menu commands act on it.
    @ObservationIgnored weak var current: Workspace?
    /// Opens a new Raptor window (set by the menu commands, which can reach SwiftUI's openWindow).
    @ObservationIgnored var openMainWindow: (() -> Void)?
    @ObservationIgnored private var workspaces: [WeakWorkspace] = []
    @ObservationIgnored private var pathMonitor: NWPathMonitor?
    @ObservationIgnored private var lastPath: NWPath.Status?

    private init() {
        let dir = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0].appendingPathComponent("Raptor")
        model = RaptorModel(dataDirectory: dir, platform: "macos", deviceLabel: Host.current().localizedName)
        loadPrefs()
    }

    func makeWorkspace() -> Workspace {
        let w = Workspace(app: self)
        workspaces.removeAll { $0.value == nil }
        workspaces.append(WeakWorkspace(value: w))
        if current == nil { current = w }
        return w
    }

    var allWorkspaces: [Workspace] { workspaces.compactMap(\.value) }

    // MARK: settings

    private func setting(_ key: String) -> String? { model.core.setting(key: key) }

    private func loadPrefs() {
        var p = TerminalPrefs()
        p.fontId = setting(Prefs.font) ?? p.fontId
        p.fontSize = setting("font_size").flatMap(Double.init).map { CGFloat(min(max($0, 8), 32)) } ?? p.fontSize
        p.lineHeight = setting(Prefs.lineHeight).flatMap(Double.init).map { CGFloat($0) } ?? p.lineHeight
        p.cursorShape = switch setting(Prefs.cursor) { case "block": .block; case "bar": .bar; case "underline": .underline; default: nil }
        p.blink = setting(Prefs.blink).map { $0 == "true" } ?? true
        p.optionAsMeta = setting("option_as_meta").flatMap(TerminalPrefs.OptionKey.init) ?? .off
        prefs = p
        confirmClose = setting("confirm_close").map { $0 == "true" } ?? true
    }

    func update(_ change: (inout TerminalPrefs) -> Void) {
        var p = prefs
        change(&p)
        guard p != prefs else { return }
        prefs = p
        let core = model.core
        core.setSetting(key: Prefs.font, value: p.fontId)
        core.setSetting(key: "font_size", value: String(Double(p.fontSize)))
        core.setSetting(key: Prefs.lineHeight, value: String(Double(p.lineHeight)))
        core.setSetting(key: Prefs.cursor, value: p.cursorShape.map { "\($0)".lowercased() } ?? "program")
        core.setSetting(key: Prefs.blink, value: p.blink ? "true" : "false")
        core.setSetting(key: "option_as_meta", value: p.optionAsMeta.rawValue)
        allWorkspaces.forEach { $0.applyAppearance() }
    }

    func setConfirmClose(_ on: Bool) {
        confirmClose = on
        model.core.setSetting(key: "confirm_close", value: on ? "true" : "false")
    }

    func setTheme(_ id: String) {
        model.setTheme(id)
        allWorkspaces.forEach { $0.applyAppearance() }
    }

    func setFontSize(_ s: CGFloat) { update { $0.fontSize = min(max(s.rounded(), 8), 32) } }
    func biggerText() { setFontSize(prefs.fontSize + 1) }
    func smallerText() { setFontSize(prefs.fontSize - 1) }
    func actualSize() { setFontSize(13) }

    func remember(serverId: String, sessionId: String) {
        model.core.setSetting(key: "mac_last_session", value: "\(serverId)|\(sessionId)")
    }

    var lastSession: (String, String)? {
        guard let v = setting("mac_last_session") else { return nil }
        let parts = v.split(separator: "|", maxSplits: 1).map(String.init)
        return parts.count == 2 ? (parts[0], parts[1]) : nil
    }

    // MARK: lifecycle

    /// Reconnect right away after sleep or a network change instead of waiting out a backoff.
    func startWatchingSystem() {
        let ws = NSWorkspace.shared.notificationCenter
        ws.addObserver(forName: NSWorkspace.willSleepNotification, object: nil, queue: .main) { _ in
            MainActor.assumeIsolated { self.model.setForeground(false) }
        }
        ws.addObserver(forName: NSWorkspace.didWakeNotification, object: nil, queue: .main) { _ in
            MainActor.assumeIsolated { self.model.setForeground(true) }
        }
        let monitor = NWPathMonitor()
        monitor.pathUpdateHandler = { path in
            DispatchQueue.main.async {
                MainActor.assumeIsolated {
                    let changed = self.lastPath != nil && self.lastPath != path.status
                    self.lastPath = path.status
                    if changed, path.status == .satisfied { self.reconnectAll() }
                }
            }
        }
        monitor.start(queue: .global(qos: .utility))
        pathMonitor = monitor
    }

    /// Bring up the pane a notification is about: its session, its tab, the pane focused.
    func show(serverId: String, sessionId: String, blockId: String) {
        let ws = current ?? allWorkspaces.first
        let tab = model.session(serverId, sessionId)?.windows.first { $0.blocks.contains { $0.id == blockId } }
        ws?.open(serverId: serverId, sessionId: sessionId, windowId: tab?.id, blockId: blockId)
            .window?.makeKeyAndOrderFront(nil)
    }

    func reconnectAll() {
        for s in model.state.servers { model.core.reconnect(serverId: s.id) }
    }

    /// Agents waiting on a decision, for the Dock badge.
    var blockedCount: Int { model.state.attention.filter { $0.block.agent?.state == .blocked }.count }
}

private struct WeakWorkspace {
    weak var value: Workspace?
}
