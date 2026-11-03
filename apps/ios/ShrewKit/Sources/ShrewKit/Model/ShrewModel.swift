import Foundation
import Observation

/// Where the UI is looking, so we don't notify about what's already on screen.
public struct Focus: Equatable, Sendable {
    public var serverId: String
    public var sessionId: String
    public var blockId: String?
    public init(serverId: String, sessionId: String, blockId: String?) {
        self.serverId = serverId
        self.sessionId = sessionId
        self.blockId = blockId
    }
}

/// The app's single source of truth. Wraps the Rust engine and republishes
/// its state on the main actor for SwiftUI.
@MainActor
@Observable
public final class ShrewModel {
    public private(set) var state = AppState(servers: [], attention: [])
    public let core: ShrewApp
    public var focus: Focus?
    /// The chosen color theme (see `Themes`); persisted in the engine's settings.
    public private(set) var themeId: String = "tokyo-night"
    public var theme: ThemeSpec { Themes.byId(themeId) }
    /// Bumped when terminal appearance settings change, so open terminals re-read them.
    public var appearanceVersion = 0

    public func setTheme(_ id: String) {
        themeId = id
        core.setSetting(key: "theme", value: id)
    }
    public var inForeground = true
    /// Set when a notification is tapped; the UI navigates and clears it.
    public var pendingOpen: Focus?
    /// Called for notifications the user should see (the app posts them locally).
    @ObservationIgnored public var onNotify: ((Notification) -> Void)?

    public init(dataDirectory: URL, platform: String, deviceLabel: String?) {
        Fonts.register()
        let bridge = Bridge()
        try? FileManager.default.createDirectory(at: dataDirectory, withIntermediateDirectories: true)
        core = ShrewApp(dataDir: dataDirectory.path, platform: platform, deviceLabel: deviceLabel, listener: bridge)
        bridge.model = self
        state = core.state()
        if let t = core.setting(key: "theme") { themeId = t }
    }

    fileprivate func refresh() {
        state = core.state()
    }

    fileprivate func deliver(_ n: Notification) {
        if let f = focus, inForeground, f.serverId == n.serverId, f.sessionId == n.sessionId, f.blockId == nil || f.blockId == n.blockId {
            return
        }
        if n.kind == .bell && inForeground { return }
        onNotify?(n)
    }

    public func setForeground(_ fg: Bool) {
        inForeground = fg
        core.setForeground(foreground: fg)
    }

    public func server(_ id: String) -> Server? { state.servers.first { $0.id == id } }

    public func session(_ serverId: String, _ sessionId: String) -> Session? {
        server(serverId)?.sessions.first { $0.id == sessionId }
    }

    // Engine callbacks arrive on Rust threads; hop to the main actor.
    private final class Bridge: AppListener, @unchecked Sendable {
        weak var model: ShrewModel?
        private let lock = NSLock()
        private var scheduled = false

        func stateChanged() {
            lock.lock()
            defer { lock.unlock() }
            if scheduled { return }
            scheduled = true
            DispatchQueue.main.async { [weak self] in
                guard let self else { return }
                self.lock.lock()
                self.scheduled = false
                self.lock.unlock()
                MainActor.assumeIsolated { self.model?.refresh() }
            }
        }

        func notify(notification: Notification) {
            DispatchQueue.main.async { [weak self] in
                MainActor.assumeIsolated { self?.model?.deliver(notification) }
            }
        }
    }
}

// MARK: - Presentation helpers shared by every screen

public extension AgentStatus {
    var label: String {
        switch state {
        case .blocked:
            switch waitingFor {
            case "permission": return "Needs permission"
            case "question": return "Has a question"
            case "auth": return "Needs credentials"
            default: return "Waiting for you"
            }
        case .working: return progress.map { "Working · \($0)%" } ?? "Working"
        case .done: return "Done"
        case .error: return "Error"
        case .idle: return "Idle"
        }
    }
}

public extension Connectivity {
    func label(latency: UInt32?) -> String {
        switch self {
        case .online: return latency.map { "Online · \($0) ms" } ?? "Online"
        case .connecting: return "Connecting…"
        case .offline(let error, _): return error
        case .idle: return "Paused"
        }
    }

    var isOnline: Bool { if case .online = self { return true } else { return false } }
    var isConnecting: Bool { if case .connecting = self { return true } else { return false } }
}

public extension Session {
    var activeWindow: Window? { windows.first { $0.id == activeWindowId } ?? windows.first }
    var focusedBlock: Block? {
        guard let w = activeWindow else { return nil }
        return w.blocks.first { $0.id == w.focusedBlockId } ?? w.blocks.first
    }
}

public extension AgentState {
    var urgent: Bool { self == .blocked || self == .error }
}
