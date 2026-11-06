import AppKit
import RaptorKit
import SwiftUI

/// The window's title bar: the session switcher, the session's tabs, and the
/// Needs You, command and new tab buttons. It replaces the system title bar,
/// so it also moves the window and holds the traffic lights.
struct TitleBar: View {
    static let height: CGFloat = 46
    @Bindable var ws: Workspace
    var onEditServer: (Server) -> Void
    @Environment(RaptorModel.self) private var model
    @Environment(\.chrome) private var c

    var body: some View {
        HStack(spacing: 4) {
            SessionSwitcher(ws: ws, onEditServer: onEditServer)
            if let s = ws.session, !s.windows.isEmpty {
                Rectangle().fill(c.hairline).frame(width: 1, height: 18).padding(.horizontal, 4)
                TabPills(ws: ws, session: s)
            }
            Spacer(minLength: 16)
            InboxButton(ws: ws)
            if ws.session != nil {
                BarButton(symbol: "command", help: "Command palette (⇧⌘P)") { ws.paletteShown.toggle() }
                BarButton(symbol: "plus", help: "New tab (⌘T)") { ws.newTab() }
            }
        }
        .padding(.leading, ws.fullScreen ? 10 : TrafficLights.inset)
        .padding(.trailing, 8)
        .frame(height: Self.height)
        .background { WindowDragArea() }
        .background { TitleBarKeeper(fullScreen: $ws.fullScreen) }
        .background(c.outer)
    }
}

/// The current session and server, Rex style; opens the session picker.
private struct SessionSwitcher: View {
    @Bindable var ws: Workspace
    var onEditServer: (Server) -> Void
    @Environment(RaptorModel.self) private var model
    @Environment(\.chrome) private var c
    @State private var hover = false

    /// Something in another session wants attention.
    private var elsewhere: AgentState? {
        model.state.attention
            .filter { $0.sessionId != ws.sessionId }
            .compactMap { $0.block.agent?.state }
            .filter { $0 != .idle }
            .max { $0.rank < $1.rank }
    }

    var body: some View {
        Button { ws.pickerShown.toggle() } label: {
            HStack(spacing: 9) {
                Image(systemName: ws.server?.isLocal == false ? "server.rack" : "laptopcomputer")
                    .font(.system(size: 15, weight: .regular))
                    .foregroundStyle(c.secondary)
                    .frame(width: 22)
                    .overlay(alignment: .topTrailing) {
                        if let a = elsewhere {
                            Circle().fill(Palette.agent(a)).frame(width: 7, height: 7)
                                .overlay(Circle().strokeBorder(c.outer, lineWidth: 1.5))
                                .offset(x: 3, y: -2)
                        }
                    }
                VStack(alignment: .leading, spacing: 0) {
                    Text(title).font(.system(size: 13, weight: .semibold)).foregroundStyle(c.text)
                    Text(subtitle).font(.system(size: 11)).foregroundStyle(c.secondary)
                }
                .lineLimit(1)
                Image(systemName: "chevron.up.chevron.down")
                    .font(.system(size: 8.5, weight: .semibold))
                    .foregroundStyle(c.tertiary)
            }
            .padding(.leading, 6).padding(.trailing, 8)
            .frame(height: 34)
            .frame(maxWidth: 240, alignment: .leading)
            .background(RoundedRectangle(cornerRadius: 8, style: .continuous).fill(hover || ws.pickerShown ? c.raised : .clear))
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .onHover { hover = $0 }
        .help("Switch session (⌘O)")
        .fixedSize()
        .popover(isPresented: $ws.pickerShown, arrowEdge: .bottom) {
            SessionPicker(ws: ws, onEditServer: onEditServer)
                .environment(model)
        }
        .accessibilityLabel("Session \(title) on \(subtitle)")
    }

    private var title: String {
        guard let s = ws.session else { return model.state.servers.isEmpty ? "Raptor" : "No Session" }
        return s.label.isEmpty ? "Session" : s.label
    }

    private var subtitle: String {
        guard let srv = ws.server else { return model.state.servers.isEmpty ? "No servers" : "Pick a session" }
        if !srv.connectivity.isOnline { return srv.connectivity.isConnecting ? "Connecting…" : "\(serverTitle(srv)) · offline" }
        return srv.isLocal ? "This Mac" : serverTitle(srv)
    }
}

// MARK: - tabs

/// The session's Rex windows as pills; they scroll once they no longer fit.
private struct TabPills: View {
    let ws: Workspace
    let session: Session
    @State private var shown: String?

    var body: some View {
        let active = session.activeWindow?.id
        ViewThatFits(in: .horizontal) {
            row(active)
            ScrollView(.horizontal, showsIndicators: false) { row(active).scrollTargetLayout() }
                .scrollPosition(id: $shown, anchor: .center)
                .mask {
                    HStack(spacing: 0) {
                        LinearGradient(colors: [.clear, .black], startPoint: .leading, endPoint: .trailing).frame(width: 14)
                        Color.black
                        LinearGradient(colors: [.black, .clear], startPoint: .leading, endPoint: .trailing).frame(width: 14)
                    }
                }
                .onAppear { shown = active }
                .onChange(of: active) { _, id in withAnimation(.easeOut(duration: 0.2)) { shown = id } }
        }
    }

    private func row(_ active: String?) -> some View {
        HStack(spacing: 2) {
            ForEach(Array(session.windows.enumerated()), id: \.element.id) { i, w in
                TabPill(ws: ws, window: w, index: i, active: w.id == active).id(w.id)
            }
        }
    }
}

private struct TabPill: View {
    let ws: Workspace
    let window: RexWindow
    let index: Int
    let active: Bool
    @Environment(\.chrome) private var c
    @State private var hover = false

    private var agent: AgentState? {
        window.blocks.compactMap { $0.agent?.state }.filter { $0 != .idle }.max { $0.rank < $1.rank }
    }

    var body: some View {
        HStack(spacing: 7) {
            icon
            Text(tabTitle(window))
                .font(.system(size: 12.5, weight: active ? .medium : .regular))
                .foregroundStyle(active ? c.text : c.secondary)
                .lineLimit(1)
                .modifier(CapWidth(max: 200))
            Button { ws.closeTab(window.id) } label: {
                Image(systemName: "xmark").font(.system(size: 8, weight: .bold)).frame(width: 16, height: 16)
                    .background(Circle().fill(c.key.opacity(hover ? 1 : 0)))
            }
            .buttonStyle(.plain)
            .foregroundStyle(c.secondary)
            .opacity(hover ? 1 : 0)
            .help("Close tab (⌥⌘W)")
            .accessibilityLabel("Close \(tabTitle(window))")
        }
        .padding(.leading, 10).padding(.trailing, 5)
        .frame(height: 30)
        .background(RoundedRectangle(cornerRadius: 9, style: .continuous).fill(active ? c.raised : hover ? c.surface : .clear))
        .overlay(RoundedRectangle(cornerRadius: 9, style: .continuous).strokeBorder(active ? c.glassStroke : .clear))
        .contentShape(Rectangle())
        .onTapGesture { ws.selectTab(window.id) }
        .onHover { hover = $0 }
        .help(index < 9 ? "\(tabTitle(window)) (⌘\(index + 1))" : tabTitle(window))
        .contextMenu {
            Button("Rename Tab…") { ws.renameTab(window.id) }
            Button("New Tab") { ws.newTab() }
            Divider()
            Button("Close Tab", role: .destructive) { ws.closeTab(window.id) }
        }
        .accessibilityElement(children: .combine)
        .accessibilityAddTraits(active ? [.isButton, .isSelected] : .isButton)
    }

    @ViewBuilder private var icon: some View {
        let focused = window.blocks.first { $0.id == window.focusedBlockId } ?? window.blocks.first
        ZStack {
            RoundedRectangle(cornerRadius: 4, style: .continuous).fill(active ? c.accent.opacity(0.18) : c.key)
            Text(glyph(focused))
                .font(.system(size: 8.5, weight: .bold, design: .monospaced))
                .foregroundStyle(active ? c.accent : c.secondary)
        }
        .frame(width: 18, height: 18)
        .overlay(alignment: .topTrailing) {
            if let a = agent {
                Circle().fill(Palette.agent(a)).frame(width: 7, height: 7)
                    .overlay(Circle().strokeBorder(active ? c.raised : c.outer, lineWidth: 1.5))
                    .offset(x: 3, y: -3)
            }
        }
    }
}

/// Hugs its content up to `max` points, and shrinks below that when space is short.
private struct CapWidth: ViewModifier {
    let max: CGFloat
    func body(content: Content) -> some View { Capped(max: max) { content } }

    private struct Capped: Layout {
        let max: CGFloat
        func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
            let w = Swift.min(proposal.width ?? .infinity, max)
            return subviews.first?.sizeThatFits(ProposedViewSize(width: w, height: proposal.height)) ?? .zero
        }
        func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
            subviews.first?.place(at: bounds.origin, proposal: ProposedViewSize(bounds.size))
        }
    }
}

// MARK: - buttons

struct BarButton: View {
    let symbol: String
    let help: String
    var dot: Color?
    var on = false
    let action: () -> Void
    @Environment(\.chrome) private var c
    @State private var hover = false

    var body: some View {
        Button(action: action) {
            Image(systemName: symbol)
                .font(.system(size: 13.5, weight: .regular))
                .foregroundStyle(hover || on ? c.text : c.secondary)
                .frame(width: 30, height: 30)
                .overlay(alignment: .topTrailing) {
                    if let dot {
                        Circle().fill(dot).frame(width: 7, height: 7)
                            .overlay(Circle().strokeBorder(c.outer, lineWidth: 1.5))
                            .offset(x: -4, y: 5)
                    }
                }
                .background(RoundedRectangle(cornerRadius: 8, style: .continuous).fill(hover || on ? c.raised : .clear))
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .onHover { hover = $0 }
        .help(help)
        .accessibilityLabel(help)
    }
}

/// The eye: agents waiting on you, across every server.
private struct InboxButton: View {
    @Bindable var ws: Workspace
    @Environment(RaptorModel.self) private var model

    var body: some View {
        let inbox = model.state.attention.filter { $0.block.agent?.state != .idle }
        let top = inbox.compactMap { $0.block.agent?.state }.max { $0.rank < $1.rank }
        BarButton(symbol: "eye", help: "Needs You", dot: top.map { Palette.agent($0) }, on: ws.inboxShown) {
            ws.inboxShown.toggle()
        }
        .popover(isPresented: $ws.inboxShown, arrowEdge: .bottom) {
            InboxPopover(ws: ws).environment(model)
        }
    }
}

// MARK: - window plumbing

/// Moves the window like a title bar would, and zooms or minimizes on a
/// double click as set in System Settings.
struct WindowDragArea: NSViewRepresentable {
    func makeNSView(context: Context) -> NSView { DragView() }
    func updateNSView(_ v: NSView, context: Context) {}

    final class DragView: NSView {
        override var mouseDownCanMoveWindow: Bool { true }

        override func mouseDown(with event: NSEvent) {
            guard let window else { return }
            if event.clickCount == 2 {
                switch UserDefaults.standard.string(forKey: "AppleActionOnDoubleClick") {
                case "Minimize": window.performMiniaturize(nil)
                case "None": break
                default: window.performZoom(nil)
                }
            } else {
                window.performDrag(with: event)
            }
        }
    }
}

/// Keeps the traffic lights in place as AppKit lays the window out again, and
/// tracks full screen, where they're hidden and the bar can use their space.
private struct TitleBarKeeper: NSViewRepresentable {
    @Binding var fullScreen: Bool

    func makeNSView(context: Context) -> KeeperView { KeeperView() }
    func updateNSView(_ v: KeeperView, context: Context) {
        v.onFullScreen = { fullScreen = $0 }
        if let w = v.window { TrafficLights.place(in: w) }
    }

    final class KeeperView: NSView {
        var onFullScreen: (Bool) -> Void = { _ in }
        private var tokens: [NSObjectProtocol] = []

        override func hitTest(_ point: NSPoint) -> NSView? { nil }

        override func viewDidMoveToWindow() {
            super.viewDidMoveToWindow()
            tokens.forEach(NotificationCenter.default.removeObserver)
            tokens = []
            guard let window else { return }
            let names: [Foundation.Notification.Name] = [
                NSWindow.didResizeNotification, NSWindow.didEndLiveResizeNotification, NSWindow.didBecomeKeyNotification,
                NSWindow.didResignKeyNotification, NSWindow.didChangeBackingPropertiesNotification,
                NSWindow.willEnterFullScreenNotification, NSWindow.didExitFullScreenNotification,
            ]
            // AppKit lays the title bar out again on its own, for one when the title changes.
            let close = window.standardWindowButton(.closeButton)
            for v in [close, close?.superview?.superview].compactMap({ $0 }) {
                v.postsFrameChangedNotifications = true
                tokens.append(NotificationCenter.default.addObserver(forName: NSView.frameDidChangeNotification, object: v, queue: .main) { [weak self] _ in
                    MainActor.assumeIsolated {
                        if let w = self?.window { TrafficLights.place(in: w) }
                    }
                })
            }
            for n in names {
                tokens.append(NotificationCenter.default.addObserver(forName: n, object: window, queue: .main) { [weak self] note in
                    MainActor.assumeIsolated {
                        guard let self, let w = self.window else { return }
                        if note.name == NSWindow.willEnterFullScreenNotification { self.onFullScreen(true) }
                        if note.name == NSWindow.didExitFullScreenNotification { self.onFullScreen(false) }
                        TrafficLights.place(in: w)
                    }
                })
            }
            TrafficLights.place(in: window)
        }

        override func layout() {
            super.layout()
            if let window { TrafficLights.place(in: window) }
        }
    }
}

/// Centers the close, minimize and zoom buttons in our taller title bar.
@MainActor
enum TrafficLights {
    static let x: CGFloat = 16
    /// Where the title bar's own content starts.
    static let inset: CGFloat = 86

    static func place(in w: NSWindow) {
        guard !w.styleMask.contains(.fullScreen),
              let close = w.standardWindowButton(.closeButton),
              let mini = w.standardWindowButton(.miniaturizeButton),
              let zoom = w.standardWindowButton(.zoomButton),
              let container = close.superview?.superview else { return }
        let h = TitleBar.height
        var f = container.frame
        if f.height != h || f.minY != w.frame.height - h {
            f.size.height = h
            f.origin.y = w.frame.height - h
            container.frame = f
        }
        let step = max(20, mini.frame.minX - close.frame.minX)
        for (i, b) in [close, mini, zoom].enumerated() {
            let o = NSPoint(x: x + CGFloat(i) * step, y: ((h - b.frame.height) / 2).rounded())
            if b.frame.origin != o { b.setFrameOrigin(o) }
        }
    }
}
