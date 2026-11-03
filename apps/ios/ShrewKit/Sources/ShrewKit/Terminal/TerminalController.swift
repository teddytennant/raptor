import Foundation
import Observation

/// Owns one open terminal: the engine handle, its status, and frame delivery.
/// Platform views (UIKit on iOS) observe it and draw `frame`.
@MainActor
@Observable
public final class TerminalController {
    public let serverId: String
    public let sessionId: String
    public let blockId: String
    public private(set) var status: TerminalStatus = .connecting
    public private(set) var hasControl = false
    @ObservationIgnored public private(set) var terminal: Terminal?
    /// Called on the main thread when a new frame should be drawn.
    @ObservationIgnored public var onFrame: (() -> Void)?
    @ObservationIgnored public var onClipboard: ((String) -> Void)?
    @ObservationIgnored private var listener: Listener?

    /// Whether the key bar rides on the keyboard (Settings).
    public let showsKeyBar: Bool

    public init(model: ShrewModel, serverId: String, sessionId: String, blockId: String) {
        showsKeyBar = Prefs.flag(model, Prefs.keyBar)
        self.serverId = serverId
        self.sessionId = sessionId
        self.blockId = blockId
        let l = Listener()
        listener = l
        terminal = try? model.core.openTerminal(serverId: serverId, sessionId: sessionId, blockId: blockId, listener: l)
        l.controller = self
    }

    public func frame() -> Frame? { terminal?.frame() }

    /// Take the size when nobody else holds it (e.g. the desktop app is closed).
    public func autoControl(ownedByOther: Bool) {
        if !ownedByOther { takeControl() }
    }

    public func takeControl() {
        terminal?.takeControl()
        hasControl = true
    }

    public func releaseControl() {
        terminal?.releaseControl()
        hasControl = false
    }

    /// Apply a color theme to the terminal (or go back to the server's colors).
    public func apply(_ theme: ThemeSpec) {
        guard let t = terminal else { return }
        if let ansi = theme.ansi {
            t.setTheme(foreground: theme.foreground, background: theme.background, cursor: theme.cursor, ansi: ansi)
        } else {
            t.useServerColors()
        }
    }

    public func toggleControl() {
        hasControl ? releaseControl() : takeControl()
    }

    public func close() {
        terminal?.shutdown()
        terminal = nil
    }

    fileprivate func setStatus(_ s: TerminalStatus) { status = s }

    private final class Listener: TerminalListener, @unchecked Sendable {
        weak var controller: TerminalController?
        private let lock = NSLock()
        private var pending = false

        func frameReady() {
            lock.lock()
            defer { lock.unlock() }
            guard !pending else { return }
            pending = true
            DispatchQueue.main.async { [weak self] in
                guard let self else { return }
                self.lock.lock()
                self.pending = false
                self.lock.unlock()
                MainActor.assumeIsolated { self.controller?.onFrame?() }
            }
        }

        func statusChanged(status: TerminalStatus) {
            DispatchQueue.main.async { [weak self] in
                MainActor.assumeIsolated { self?.controller?.setStatus(status) }
            }
        }

        func clipboard(text: String) {
            DispatchQueue.main.async { [weak self] in
                MainActor.assumeIsolated { self?.controller?.onClipboard?(text) }
            }
        }

        func sizeChanged(cols: UInt16, rows: UInt16) {
            frameReady()
        }
    }
}

public extension TerminalStatus {
    /// Text for the floating status pill, or nil when live.
    var banner: String? {
        switch self {
        case .live: return nil
        case .connecting: return "Connecting…"
        case .reconnecting: return "Reconnecting…"
        case .exited(let code): return code == 0 ? "Process exited" : "Process exited (\(code))"
        case .ended(let reason): return reason
        }
    }

    var isBusy: Bool {
        switch self {
        case .connecting, .reconnecting: return true
        default: return false
        }
    }
}
