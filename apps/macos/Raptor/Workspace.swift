import AppKit
import Observation
import RaptorKit

/// One Raptor window: which session it shows, its open terminals, and every
/// command the menus, palette and keyboard can run against it.
@MainActor
@Observable
final class Workspace {
    let app: AppModel
    var model: RaptorModel { app.model }
    private(set) var serverId: String?
    private(set) var sessionId: String?
    var paletteShown = false
    var findShown = false
    var findQuery = ""
    var findMiss = false
    /// Bumped by each ⌘F, so an open find bar takes the keyboard back.
    var findRequests = 0
    var addingServer = false
    var replying: AttentionItem?
    var renaming: RenameRequest?
    var confirm: ConfirmRequest?
    var error: String?
    /// Focus we asked the server for, shown until its view catches up.
    var pendingFocus: String?
    /// Bumped when panes need a re-layout without a state change (e.g. font size).
    var layoutVersion = 0
    @ObservationIgnored private(set) var terminals: [String: TerminalView] = [:]
    @ObservationIgnored weak var window: NSWindow?
    @ObservationIgnored private var pendingResize: (Split, Double)?
    @ObservationIgnored private var resizing = false

    init(app: AppModel) {
        self.app = app
    }

    var server: Server? { serverId.flatMap { model.server($0) } }
    var session: Session? {
        guard let serverId, let sessionId else { return nil }
        return model.session(serverId, sessionId)
    }
    var activeWindow: RexWindow? { session?.activeWindow }
    var focusedBlockId: String? {
        guard let w = activeWindow else { return nil }
        if let p = pendingFocus, w.blocks.contains(where: { $0.id == p }) { return p }
        return w.focusedBlockId ?? w.blocks.first?.id
    }
    var focusedBlock: Block? { activeWindow?.blocks.first { $0.id == focusedBlockId } }
    var focusedTerminal: TerminalView? { focusedBlockId.flatMap { terminals[$0] } }

    // MARK: opening

    func open(serverId: String, sessionId: String, windowId: String? = nil, blockId: String? = nil) {
        if self.sessionId != sessionId || self.serverId != serverId {
            closeAllTerminals()
            self.serverId = serverId
            self.sessionId = sessionId
            pendingFocus = nil
            app.remember(serverId: serverId, sessionId: sessionId)
        }
        model.focus = Focus(serverId: serverId, sessionId: sessionId, blockId: blockId)
        if let windowId, windowId != session?.activeWindowId { selectTab(windowId) }
        if let blockId { focusPane(blockId) }
    }

    /// Drop terminals whose blocks are gone and follow sessions that disappear.
    func reconcile() {
        guard let serverId, let sessionId else { return }
        guard let srv = model.server(serverId), srv.connectivity.isOnline else { return }
        guard let s = srv.sessions.first(where: { $0.id == sessionId }) else {
            closeAllTerminals()
            self.sessionId = nil
            if let next = srv.sessions.first { open(serverId: serverId, sessionId: next.id) }
            return
        }
        let live = Set(s.windows.flatMap { $0.blocks.map(\.id) })
        for (id, view) in terminals where !live.contains(id) {
            view.controller.close()
            terminals[id] = nil
        }
        if let p = pendingFocus, s.activeWindow?.focusedBlockId == p { pendingFocus = nil }
    }

    func terminal(for block: Block) -> TerminalView? {
        guard let serverId, let sessionId else { return nil }
        if let t = terminals[block.id] { return t }
        let c = TerminalController(model: model, serverId: serverId, sessionId: sessionId, blockId: block.id)
        guard c.terminal != nil else { return nil }
        c.apply(model.theme)
        let v = TerminalView(controller: c, prefs: app.prefs)
        v.onActivate = { [weak self, weak v] in
            guard let self, let v else { return }
            self.activate(v, blockId: block.id)
        }
        v.onFontSize = { [weak self] s in self?.app.setFontSize(s) }
        v.onAction = { [weak self] a in self?.run(a, blockId: block.id) }
        // Take the size unless another connected client holds it.
        c.autoControl(ownedByOther: block.ownedByOther)
        terminals[block.id] = v
        return v
    }

    private func activate(_ v: TerminalView, blockId: String) {
        if !v.controller.hasControl {
            v.controller.takeControl()
            v.pushGrid()
        }
        if focusedBlockId != blockId { focusPane(blockId) }
    }

    private func closeAllTerminals() {
        for v in terminals.values { v.controller.close() }
        terminals.removeAll()
    }

    func close() {
        closeAllTerminals()
    }

    func applyAppearance() {
        for v in terminals.values {
            v.prefs = app.prefs
            v.controller.apply(model.theme)
        }
        layoutVersion += 1
    }

    // MARK: commands

    private func run(_ body: @escaping (RaptorApp, String, String) async throws -> Void) {
        guard let serverId, let sessionId else { return }
        let core = model.core
        Task {
            do { try await body(core, serverId, sessionId) } catch { self.error = describe(error) }
        }
    }

    func run(_ a: PaneAction, blockId: String) {
        switch a {
        case .splitRight: split(vertical: false, from: blockId)
        case .splitDown: split(vertical: true, from: blockId)
        case .zoom: toggleZoom(blockId)
        case .close: closePane(blockId)
        case .takeSize:
            terminals[blockId]?.controller.takeControl()
            terminals[blockId]?.pushGrid()
        }
    }

    func split(vertical: Bool, from blockId: String? = nil) {
        guard let b = blockId ?? focusedBlockId else { return }
        run { core, srv, ses in
            if let id = try await core.split(serverId: srv, sessionId: ses, blockId: b, vertical: vertical) {
                await MainActor.run { self.pendingFocus = id }
            }
        }
    }

    func closePane(_ blockId: String? = nil, confirmed: Bool = false) {
        guard let id = blockId ?? focusedBlockId, let block = activeWindow?.blocks.first(where: { $0.id == id }) else { return }
        if !confirmed, app.confirmClose, isBusy(block) {
            confirm = ConfirmRequest(title: "Close “\(block.name)”?", message: "\(runningName(block) ?? "A program") is still running in this pane. Closing it stops the program on the server.", action: "Close Pane") { [weak self] in
                self?.closePane(id, confirmed: true)
            }
            return
        }
        if activeWindow?.blocks.count == 1, session?.windows.count == 1 {
            if !confirmed {
                confirm = ConfirmRequest(title: "Close the last pane?", message: "This is the only pane in “\(session?.label ?? "this session")”. Closing it ends the session.", action: "Close Pane") { [weak self] in
                    self?.closePane(id, confirmed: true)
                }
                return
            }
        }
        run { core, srv, ses in try await core.closeBlock(serverId: srv, sessionId: ses, blockId: id) }
    }

    private func isBusy(_ b: Block) -> Bool {
        let shells: Set<String> = ["bash", "zsh", "fish", "sh", "nu", "dash", "ksh", "tcsh", "login", ""]
        return b.exited == nil && !shells.contains(b.process)
    }

    func showFind() {
        findShown = true
        findRequests += 1
    }

    /// ⌘G and ⇧⌘G: the next match in the focused terminal, newer or older.
    func findStep(older: Bool, beep: Bool = true) {
        guard findShown else { return showFind() }
        guard !findQuery.isEmpty, let t = focusedTerminal?.terminal else { return }
        findMiss = !t.find(query: findQuery, older: older)
        if findMiss, beep { NSSound.beep() }
    }

    func toggleZoom(_ blockId: String? = nil) {
        guard let id = blockId ?? focusedBlockId else { return }
        run { core, srv, ses in try await core.zoomBlock(serverId: srv, sessionId: ses, blockId: id) }
    }

    func focusPane(_ blockId: String) {
        pendingFocus = blockId
        model.focus = serverId.flatMap { s in sessionId.map { Focus(serverId: s, sessionId: $0, blockId: blockId) } }
        if let t = terminals[blockId], t.window?.firstResponder !== t { t.window?.makeFirstResponder(t) }
        run { core, srv, ses in try await core.focusBlock(serverId: srv, sessionId: ses, blockId: blockId) }
    }

    /// Cycle panes in layout order.
    func cyclePane(_ delta: Int) {
        guard let w = activeWindow, w.blocks.count > 1, let cur = focusedBlockId else { return }
        let order = w.blocks.filter { !$0.floating }.sorted { ($0.y, $0.x) < ($1.y, $1.x) }
        guard let i = order.firstIndex(where: { $0.id == cur }) else { return }
        focusPane(order[(i + delta + order.count) % order.count].id)
    }

    enum Direction { case left, right, up, down }

    /// Focus the nearest pane in a direction, measured from the focused pane's center.
    func focusPane(_ dir: Direction) {
        guard let w = activeWindow, let cur = w.blocks.first(where: { $0.id == focusedBlockId }) else { return }
        let cx = cur.x + cur.w / 2, cy = cur.y + cur.h / 2
        let candidates = w.blocks.filter { b in
            guard b.id != cur.id else { return false }
            switch dir {
            case .left: return b.x + b.w <= cur.x + 0.001 && b.y < cur.y + cur.h && b.y + b.h > cur.y
            case .right: return b.x >= cur.x + cur.w - 0.001 && b.y < cur.y + cur.h && b.y + b.h > cur.y
            case .up: return b.y + b.h <= cur.y + 0.001 && b.x < cur.x + cur.w && b.x + b.w > cur.x
            case .down: return b.y >= cur.y + cur.h - 0.001 && b.x < cur.x + cur.w && b.x + b.w > cur.x
            }
        }
        let best = candidates.min { a, b in
            func d(_ x: Block) -> Double { hypot(x.x + x.w / 2 - cx, x.y + x.h / 2 - cy) }
            return d(a) < d(b)
        }
        if let best { focusPane(best.id) }
    }

    func selectTab(_ windowId: String) {
        run { core, srv, ses in try await core.focusWindow(serverId: srv, sessionId: ses, windowId: windowId) }
    }

    func selectTab(index: Int) {
        guard let ws = session?.windows, !ws.isEmpty else { return }
        let i = index < 0 ? ws.count - 1 : index
        if ws.indices.contains(i) { selectTab(ws[i].id) }
    }

    func cycleTab(_ delta: Int) {
        guard let ws = session?.windows, ws.count > 1, let cur = ws.firstIndex(where: { $0.id == activeWindow?.id }) else { return }
        selectTab(ws[(cur + delta + ws.count) % ws.count].id)
    }

    func newTab() {
        run { core, srv, ses in try await core.newWindow(serverId: srv, sessionId: ses) }
    }

    func closeTab(_ windowId: String? = nil) {
        guard let id = windowId ?? activeWindow?.id, let w = session?.windows.first(where: { $0.id == id }) else { return }
        let busy = w.blocks.filter(isBusy)
        if app.confirmClose, !busy.isEmpty || session?.windows.count == 1 {
            let last = session?.windows.count == 1
            confirm = ConfirmRequest(
                title: "Close tab “\(tabTitle(w))”?",
                message: last ? "This is the last tab in the session. Closing it ends the session." : "\(busy.count == 1 ? "A pane is still running a program. Closing the tab stops it" : "\(busy.count) panes are still running programs. Closing the tab stops them") on the server.",
                action: "Close Tab"
            ) { [weak self] in
                self?.run { core, srv, ses in try await core.closeWindow(serverId: srv, sessionId: ses, windowId: id) }
            }
            return
        }
        run { core, srv, ses in try await core.closeWindow(serverId: srv, sessionId: ses, windowId: id) }
    }

    func renameTab(_ windowId: String? = nil) {
        guard let id = windowId ?? activeWindow?.id, let w = session?.windows.first(where: { $0.id == id }) else { return }
        renaming = RenameRequest(title: "Rename Tab", value: w.label) { [weak self] label in
            self?.run { core, srv, ses in try await core.renameWindow(serverId: srv, sessionId: ses, windowId: id, label: label) }
        }
    }

    func renameSession() {
        guard let s = session else { return }
        renaming = RenameRequest(title: "Rename Session", value: s.label) { [weak self] label in
            self?.run { core, srv, ses in try await core.renameSession(serverId: srv, sessionId: ses, label: label) }
        }
    }

    func newSession(on serverId: String? = nil) {
        guard let srv = serverId ?? self.serverId ?? model.state.servers.first?.id else { return }
        let core = model.core
        Task {
            do {
                let id = try await core.createSession(serverId: srv, label: nil)
                self.open(serverId: srv, sessionId: id)
            } catch { self.error = describe(error) }
        }
    }

    func endSession(serverId: String, sessionId: String, label: String) {
        confirm = ConfirmRequest(title: "End session “\(label)”?", message: "Every program in it stops on the server.", action: "End Session") { [weak self] in
            guard let core = self?.model.core else { return }
            Task { try? await core.destroySession(serverId: serverId, sessionId: sessionId) }
        }
    }

    func copy() { focusedTerminal?.copy(nil) }
    func paste() { focusedTerminal?.paste(nil) }
    func selectAll() { focusedTerminal?.selectAll(nil) }

    /// Divider drags send one request at a time, always the latest position, so
    /// replies can't land out of order and leave the border where the drag began.
    func resizeSplit(_ s: Split, ratio: Double) {
        pendingResize = (s, ratio)
        guard !resizing, let serverId, let sessionId else { return }
        resizing = true
        let core = model.core
        Task {
            while let (s, r) = pendingResize {
                pendingResize = nil
                try? await core.resizePane(serverId: serverId, sessionId: sessionId, before: s.before, after: s.after, horizontal: s.horizontal, ratio: r)
            }
            resizing = false
        }
    }
}

struct RenameRequest: Identifiable {
    let id = UUID()
    let title: String
    let value: String
    let commit: (String) -> Void
}

struct ConfirmRequest: Identifiable {
    let id = UUID()
    let title: String
    let message: String
    let action: String
    let run: () -> Void
}

func tabTitle(_ w: RexWindow) -> String {
    // The server names unlabeled windows "Window <id>"; what's running reads better.
    if !w.label.isEmpty, !w.label.hasPrefix("Window ") { return w.label }
    let b = w.blocks.first { $0.id == w.focusedBlockId } ?? w.blocks.first
    return b.map { runningName($0) ?? $0.name } ?? "Tab"
}

typealias RexWindow = RaptorKit.Window

extension Connectivity {
    var offlineError: String? {
        if case .offline(let e, _) = self { return e }
        return nil
    }
}
