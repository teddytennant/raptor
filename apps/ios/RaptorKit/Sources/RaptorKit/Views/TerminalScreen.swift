import SwiftUI

/// A session, full-bleed: the terminal is the screen and the controls float on it.
public struct TerminalScreen: View {
    @Environment(RaptorModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    let serverId: String
    @State private var sessionId: String
    @State private var windowId: String?
    @State private var blockId: String?
    @State private var termColors: (UInt32, UInt32)?
    @State private var controller: TerminalController?
    @State private var topInset: CGFloat = 0
    @State private var bottomInset: CGFloat = 0
    @State private var keyboard = false
    @State private var showPalette = false
    @State private var slideEdge: Edge = .trailing
    /// Set by the app root: the left button and the title open the session
    /// switcher, and switching sessions goes through the root.
    var onSessions: (() -> Void)?
    var onOpen: ((Focus) -> Void)?

    public init(serverId: String, sessionId: String, blockId: String?, onSessions: (() -> Void)? = nil, onOpen: ((Focus) -> Void)? = nil) {
        self.onSessions = onSessions
        self.onOpen = onOpen
        self.serverId = serverId
        _sessionId = State(initialValue: sessionId)
        _blockId = State(initialValue: blockId)
    }

    private var chrome: Chrome {
        let t = model.theme
        if t.ansi == nil, let (bg, fg) = termColors { return Chrome(bg: bg, text: fg, accent: t.accent) }
        return Chrome(t)
    }

    public var body: some View {
        let c = chrome
        let server = model.server(serverId)
        let session = model.session(serverId, sessionId)
        let window = session?.windows.first { $0.id == windowId } ?? session?.windows.first { w in w.blocks.contains { $0.id == blockId } } ?? session?.activeWindow
        let block = window?.blocks.first { $0.id == blockId }

        ZStack(alignment: .top) {
            c.bg.ignoresSafeArea()
            if let b = blockId {
                LiveTerminal(serverId: serverId, sessionId: sessionId, blockId: b, theme: model.theme, chrome: c,
                             insetTop: topInset, insetBottom: bottomInset, controller: $controller,
                             onSwipe: { swipe($0, session: session) }, onPalette: { showPalette = true }) { termColors = ($0, $1) }
                    .id(b)
                    .transition(.asymmetric(insertion: .move(edge: slideEdge), removal: .move(edge: slideEdge == .trailing ? .leading : .trailing)))
                    .ignoresSafeArea(.container, edges: [.top, .bottom])
            } else if let session, session.windows.isEmpty {
                ContentUnavailableView {
                    Label("No windows", systemImage: "macwindow")
                } description: {
                    Text("Everything in \(session.label) has exited.")
                } actions: {
                    Button("New Window") { Task { try? await model.core.newWindow(serverId: serverId, sessionId: sessionId) } }
                        .buttonStyle(.borderedProminent)
                    Button("End Session", role: .destructive) {
                        Task { try? await model.core.destroySession(serverId: serverId, sessionId: sessionId) }
                    }
                }
                .foregroundStyle(c.text)
                .tint(c.accent)
            } else if case .offline(let error, _) = server?.connectivity {
                ContentUnavailableView {
                    Label("Can't reach \(server.map(serverTitle) ?? "the server")", systemImage: "wifi.exclamationmark")
                } description: {
                    Text(error)
                } actions: {
                    Button("Try Again") { model.core.reconnect(serverId: serverId) }.buttonStyle(.borderedProminent)
                }
                .foregroundStyle(c.text)
                .tint(c.accent)
            } else {
                ProgressView().tint(c.accent).frame(maxWidth: .infinity, maxHeight: .infinity)
            }

            // The top bar sits over the terminal like glass: live rows start
            // below it, and scrollback moving up behind it shows through blurred.
            VStack(spacing: 8) {
                header(c, server: server, session: session, window: window, block: block)
                // Windows as slim tabs in the top bar: always reachable, even while typing.
                if let session, session.windows.count > 1 {
                    WindowTabs(session: session, current: window?.id) { w in
                        slideEdge = (session.windows.firstIndex { $0.id == w.id } ?? 0) > (session.windows.firstIndex { $0.id == window?.id } ?? 0) ? .trailing : .leading
                        withAnimation(.snappy(duration: 0.25)) {
                            windowId = w.id
                            blockId = w.blocks.first { $0.id == w.focusedBlockId }?.id ?? w.blocks.first?.id
                        }
                        Haptics.select()
                        Task { try? await model.core.focusWindow(serverId: serverId, sessionId: sessionId, windowId: w.id) }
                    } onNew: {
                        Task { try? await model.core.newWindow(serverId: serverId, sessionId: sessionId) }
                    }
                }
            }
                .padding(.bottom, 8)
                .background {
                    Rectangle()
                        .fill(.ultraThinMaterial)
                        .overlay(c.bg.opacity(0.45))
                        .overlay(alignment: .bottom) { Rectangle().fill(c.hairline).frame(height: 0.5) }
                        .environment(\.colorScheme, c.scheme)
                        .ignoresSafeArea(edges: .top)
                }
                .background(GeometryReader { g in Color.clear.preference(key: TopInsetKey.self, value: g.frame(in: .global).maxY) })

        }
        .environment(\.chrome, c)
        .preferredColorScheme(c.scheme)
        .toolbar(.hidden, for: .navigationBar)
        .statusBarHidden(false)
        .onPreferenceChange(TopInsetKey.self) { topInset = $0 }
        .onPreferenceChange(BottomInsetKey.self) { bottomInset = keyboard ? 0 : $0 }
        .animation(.snappy(duration: 0.25), value: keyboard)
        #if os(iOS)
        .onReceive(NotificationCenter.default.publisher(for: UIResponder.keyboardWillShowNotification)) { _ in keyboard = true; bottomInset = 0 }
        .onReceive(NotificationCenter.default.publisher(for: UIResponder.keyboardWillHideNotification)) { _ in keyboard = false }
        #endif
        .onAppear { pick(session) }
        .onChange(of: session) { pick(model.session(serverId, sessionId)) }
        .onChange(of: sessionId) { blockId = nil; windowId = nil; pick(model.session(serverId, sessionId)) }
        .onChange(of: blockId) { model.focus = Focus(serverId: serverId, sessionId: sessionId, blockId: blockId) }
        .onDisappear { model.focus = nil }
        .sheet(isPresented: $showPalette) {
            CommandPalette(serverId: serverId, local: localCommands(session: session), sessionId: sessionId, blockId: blockId)
                .environment(\.chrome, c)
        }
    }

    /// Every pane in the session, in window order then layout order.
    private func panes(_ s: Session?) -> [(window: String, block: String)] {
        (s?.windows ?? []).flatMap { w in w.blocks.map { (w.id, $0.id) } }
    }

    /// Swipe to the next (+1) or previous (-1) pane, crossing into other windows.
    /// Swipe left/right moves between tabs (windows), landing on each tab's
    /// focused pane. With a single tab, it cycles that tab's panes instead.
    private func swipe(_ dir: Int, session: Session?) {
        if let wins = session?.windows, wins.count > 1 {
            let i = wins.firstIndex { $0.id == windowId } ?? 0
            let w = wins[(i + dir + wins.count) % wins.count]
            slideEdge = dir > 0 ? .trailing : .leading
            Haptics.select()
            withAnimation(.snappy(duration: 0.28)) {
                windowId = w.id
                blockId = w.blocks.first { $0.id == w.focusedBlockId }?.id ?? w.blocks.first?.id
            }
            Task { try? await model.core.focusWindow(serverId: serverId, sessionId: sessionId, windowId: w.id) }
            return
        }
        let all = panes(session)
        guard all.count > 1, let i = all.firstIndex(where: { $0.block == blockId }) else { return }
        let next = all[(i + dir + all.count) % all.count]
        slideEdge = dir > 0 ? .trailing : .leading
        Haptics.select()
        withAnimation(.snappy(duration: 0.28)) {
            windowId = next.window
            blockId = next.block
        }
        Task { try? await model.core.focusBlock(serverId: serverId, sessionId: sessionId, blockId: next.block) }
    }

    private func localCommands(session: Session?) -> [PaletteItem] {
        var items: [PaletteItem] = []
        let srv = serverId, sid = sessionId, bid = blockId
        let core = model.core
        func add(_ id: String, _ title: String, _ symbol: String, _ cat: String = "Raptor", subtitle: String = "", keywords: String = "", _ run: @escaping () -> Void) {
            items.append(PaletteItem(id: id, title: title, subtitle: subtitle, category: cat, keywords: keywords, symbol: symbol, run: run))
        }
        add("next-pane", "Next Tab", "arrow.right.square", keywords: "switch window pane") { swipe(1, session: session) }
        add("prev-pane", "Previous Tab", "arrow.left.square", keywords: "switch window pane") { swipe(-1, session: session) }
        add("new-window", "New Window", "plus.square.on.square") { Task { try? await core.newWindow(serverId: srv, sessionId: sid) } }
        if let b = bid {
            add("split-right", "Split Right", "rectangle.split.2x1") { Task { _ = try? await core.split(serverId: srv, sessionId: sid, blockId: b, vertical: false) } }
            add("split-down", "Split Down", "rectangle.split.1x2") { Task { _ = try? await core.split(serverId: srv, sessionId: sid, blockId: b, vertical: true) } }
            add("close-pane", "Close Pane", "xmark.square") { closePane(b, session: session) }
        }
        if let ctl = controller {
            add("fit", ctl.hasControl ? "Use Desktop Size" : "Fit to This Screen", "arrow.up.left.and.arrow.down.right", keywords: "resize size") { ctl.toggleControl() }
            #if os(iOS)
            add("copy-all", "Copy All Text", "doc.on.doc") { UIPasteboard.general.string = ctl.terminal?.allText() }
            add("paste", "Paste", "doc.on.clipboard") { if let s = UIPasteboard.general.string { ctl.terminal?.paste(text: s) } }
            #endif
        }
        add("new-session", "New Session", "plus.rectangle.on.rectangle", "Sessions") {
            Task { if let id = try? await core.createSession(serverId: srv, label: nil) { switchTo(id) } }
        }
        for s in model.server(srv)?.sessions ?? [] where s.id != sid {
            add("session-\(s.id)", "Go to \(s.label)", "rectangle.stack", "Sessions", subtitle: detail(s.focusedBlock)) { switchTo(s.id) }
        }
        for t in Themes.all {
            add("theme-\(t.id)", t.name, t.id == model.themeId ? "checkmark.circle.fill" : "paintpalette", "Themes", keywords: "theme color") { model.setTheme(t.id) }
        }
        return items
    }

    /// Keep a live pane on screen. If the current one is gone (closed here or
    /// elsewhere), prefer another pane in the same window, then the session's
    /// focused pane; a window's focus can still name the pane that just closed.
    private func pick(_ session: Session?) {
        guard let session else { return }
        if let b = blockId, session.windows.contains(where: { $0.blocks.contains { $0.id == b } }) { return }
        let all = panes(session)
        let next = all.first { $0.window == windowId }
            ?? session.activeWindow.flatMap { w in w.blocks.first { $0.id == w.focusedBlockId }.map { (w.id, $0.id) } }
            ?? all.first
        windowId = next?.0
        blockId = next?.1
    }

    /// Close a pane: move to its neighbour first so the screen never shows the
    /// closed terminal, then ask the server.
    private func closePane(_ id: String, session: Session?) {
        if id == blockId {
            let all = panes(session).filter { $0.block != id }
            if let next = all.first(where: { $0.window == windowId }) ?? all.first {
                withAnimation(.snappy(duration: 0.25)) {
                    windowId = next.window
                    blockId = next.block
                }
            }
        }
        Task { try? await model.core.closeBlock(serverId: serverId, sessionId: sessionId, blockId: id) }
    }

    private func switchTo(_ id: String) {
        if let onOpen { onOpen(Focus(serverId: serverId, sessionId: id, blockId: nil)) } else { sessionId = id }
    }

    @ViewBuilder
    private func header(_ c: Chrome, server: Server?, session: Session?, window: Window?, block: Block?) -> some View {
        HStack(spacing: 10) {
            Button { if let onSessions { onSessions() } else { dismiss() } } label: {
                Image(systemName: onSessions == nil ? "chevron.left" : "list.bullet").font(.system(size: 16, weight: .semibold)).foregroundStyle(c.text).frame(width: 40, height: 40)
            }
            .glass(Circle(), c)
            .accessibilityLabel(onSessions == nil ? "Back" : "Sessions")

            Tile(glyph(block), size: 32)

            Button { onSessions?() } label: {
                VStack(alignment: .leading, spacing: 1) {
                    HStack(spacing: 6) {
                        Text(block.flatMap(runningName) ?? "terminal").font(.system(size: 15, weight: .semibold)).foregroundStyle(c.text)
                        if let p = block?.shortPwd, !p.isEmpty {
                            Text(p).font(.mono(13)).foregroundStyle(c.secondary).truncationMode(.middle)
                        }
                    }
                    .lineLimit(1)
                    HStack(spacing: 3) {
                        Text(session?.label ?? "").font(.system(size: 12)).foregroundStyle(c.tertiary)
                        Image(systemName: "chevron.down").font(.system(size: 9, weight: .bold)).foregroundStyle(c.tertiary)
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel("Sessions")

            if let window, window.blocks.count > 1 {
                PaneMap(window: window, selected: blockId) { picked in
                    blockId = picked
                    Haptics.select()
                    Task { try? await model.core.focusBlock(serverId: serverId, sessionId: sessionId, blockId: picked) }
                }
                .frame(width: 54, height: 38)
            }

            MoreMenu(serverId: serverId, sessionId: sessionId, blockId: blockId, controller: controller, onClosePane: { closePane($0, session: session) }) { showPalette = true }
        }
        .padding(.horizontal, 12)
        .padding(.top, 4)
    }
}

private struct TopInsetKey: PreferenceKey {
    static let defaultValue: CGFloat = 0
    static func reduce(value: inout CGFloat, nextValue: () -> CGFloat) { value = max(value, nextValue()) }
}

private struct BottomInsetKey: PreferenceKey {
    static let defaultValue: CGFloat = 0
    static func reduce(value: inout CGFloat, nextValue: () -> CGFloat) { value = max(value, nextValue()) }
}

/// The window's pane layout in miniature. Tap a pane to switch to it.
private struct PaneMap: View {
    var window: Window
    var selected: String?
    var onPick: (String) -> Void
    @Environment(\.chrome) private var c

    var body: some View {
        GeometryReader { g in
            ZStack(alignment: .topLeading) {
                ForEach(window.blocks, id: \.id) { b in
                    let tint = b.agent.flatMap { $0.state == .idle ? nil : Palette.agent($0.state) }
                    RoundedRectangle(cornerRadius: 3, style: .continuous)
                        .fill(b.id == selected ? c.accent.opacity(0.6) : (tint?.opacity(0.55) ?? c.key))
                        .frame(width: max(2, b.w * g.size.width - 2), height: max(2, b.h * g.size.height - 2))
                        .offset(x: b.x * g.size.width + 1, y: b.y * g.size.height + 1)
                        .onTapGesture { onPick(b.id) }
                }
            }
        }
        .padding(4)
        .glass(RoundedRectangle(cornerRadius: 10, style: .continuous), c)
        .accessibilityLabel("Panes")
    }
}

/// The session's windows as slim tabs in the top bar.
private struct WindowTabs: View {
    var session: Session
    var current: String?
    var onPick: (Window) -> Void
    var onNew: () -> Void
    @Environment(\.chrome) private var c

    var body: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 6) {
                ForEach(session.windows, id: \.id) { w in
                    let focused = w.blocks.first { $0.id == w.focusedBlockId } ?? w.blocks.first
                    let sel = w.id == current
                    let label = (!w.label.isEmpty && !w.label.hasPrefix("Window ")) ? w.label : (focused.flatMap(runningName) ?? "shell")
                    Button { onPick(w) } label: {
                        HStack(spacing: 6) {
                            if w.blocks.contains(where: { $0.agent?.state == .blocked }) {
                                Circle().fill(Palette.blocked).frame(width: 6, height: 6)
                            }
                            Text(label).font(.system(size: 13, weight: sel ? .semibold : .regular)).foregroundStyle(sel ? c.text : c.secondary).lineLimit(1)
                            if w.blocks.count > 1 {
                                Text("\(w.blocks.count)").font(.mono(11)).foregroundStyle(c.tertiary)
                            }
                        }
                        .padding(.horizontal, 11)
                        .frame(height: 30)
                        .background(sel ? c.raised : .clear, in: RoundedRectangle(cornerRadius: 9, style: .continuous))
                        .overlay(RoundedRectangle(cornerRadius: 9, style: .continuous).strokeBorder(sel ? c.glassStroke : .clear, lineWidth: 1))
                    }
                    .buttonStyle(.plain)
                }
                Button(action: onNew) {
                    Image(systemName: "plus").font(.system(size: 14, weight: .semibold)).foregroundStyle(c.secondary).frame(width: 30, height: 30)
                }
                .accessibilityLabel("New window")
            }
            .padding(.horizontal, 10)
        }
    }
}

private struct MoreMenu: View {
    @Environment(RaptorModel.self) private var model
    @Environment(\.chrome) private var c
    let serverId: String
    let sessionId: String
    let blockId: String?
    let controller: TerminalController?
    var onClosePane: (String) -> Void
    var onPalette: () -> Void

    var body: some View {
        Menu {
            Button("Command Palette", systemImage: "command", action: onPalette)
            Button("New Window", systemImage: "plus.square.on.square") {
                Task { try? await model.core.newWindow(serverId: serverId, sessionId: sessionId) }
            }
            Divider()
            if let ctl = controller {
                Button(ctl.hasControl ? "Use Desktop Size" : "Fit to This Screen",
                       systemImage: ctl.hasControl ? "arrow.down.right.and.arrow.up.left" : "arrow.up.left.and.arrow.down.right") { ctl.toggleControl() }
            }
            if let b = blockId {
                Button("Split Right", systemImage: "rectangle.split.2x1") {
                    Task { _ = try? await model.core.split(serverId: serverId, sessionId: sessionId, blockId: b, vertical: false) }
                }
                Button("Split Down", systemImage: "rectangle.split.1x2") {
                    Task { _ = try? await model.core.split(serverId: serverId, sessionId: sessionId, blockId: b, vertical: true) }
                }
            }
            Divider()
            #if os(iOS)
            Button("Paste", systemImage: "doc.on.clipboard") {
                if let s = UIPasteboard.general.string { controller?.terminal?.paste(text: s) }
            }
            Button("Copy All Text", systemImage: "doc.on.doc") { UIPasteboard.general.string = controller?.terminal?.allText() }
            #endif
            Picker(selection: Binding(get: { model.themeId }, set: { model.setTheme($0) })) {
                ForEach(Themes.all) { t in Text(t.name).tag(t.id) }
            } label: {
                Label("Theme", systemImage: "paintpalette")
            }
            .pickerStyle(.menu)
            if let b = blockId {
                Divider()
                Button("Close Pane", systemImage: "xmark", role: .destructive) { onClosePane(b) }
            }
        } label: {
            Image(systemName: "ellipsis").font(.system(size: 16, weight: .semibold)).foregroundStyle(c.text).frame(width: 40, height: 40)
                .glass(Circle(), c)
        }
        .accessibilityLabel("More")
    }
}

/// One live terminal plus its status pill.
struct LiveTerminal: View {
    @Environment(RaptorModel.self) private var model
    let serverId: String
    let sessionId: String
    let blockId: String
    let theme: ThemeSpec
    let chrome: Chrome
    let insetTop: CGFloat
    let insetBottom: CGFloat
    @Binding var controller: TerminalController?
    var onSwipe: (Int) -> Void
    var onPalette: () -> Void
    var onColors: (UInt32, UInt32) -> Void
    @State private var own: TerminalController?

    var body: some View {
        ZStack {
            if let ctl = own {
                TerminalSurface(controller: ctl, fontSize: fontSize, chrome: chrome, insetTop: insetTop, insetBottom: insetBottom,
                                appearance: TerminalAppearance(model), onColors: onColors, onSwipe: onSwipe, onPalette: onPalette) {
                    model.core.setSetting(key: "font_size", value: String(Double($0)))
                }
                if let text = ctl.status.banner {
                    HStack(spacing: 8) {
                        if ctl.status.isBusy { ProgressView().controlSize(.small).tint(chrome.accent) }
                        Text(text).font(.footnote).foregroundStyle(chrome.text)
                    }
                    .padding(.horizontal, 14)
                    .padding(.vertical, 8)
                    .glass(Capsule(), chrome)
                    .transition(.opacity)
                }
            }
        }
        .animation(.snappy, value: own?.status)
        .onAppear {
            let c = TerminalController(model: model, serverId: serverId, sessionId: sessionId, blockId: blockId)
            // The phone takes the size while you use it (like tmux following the
            // active client): readable text, and the keyboard never hides the prompt.
            if Prefs.flag(model, Prefs.fitOnOpen) { c.takeControl() }
            c.apply(theme)
            own = c
            controller = c
        }
        .onChange(of: theme.id) { own?.apply(theme) }
        .onDisappear {
            own?.close()
            own = nil
        }
    }

    private var fontSize: CGFloat { CGFloat(Double(model.core.setting(key: "font_size") ?? "") ?? 13) }
}

#if os(iOS)
struct TerminalSurface: UIViewRepresentable {
    let controller: TerminalController
    let fontSize: CGFloat
    let chrome: Chrome
    let insetTop: CGFloat
    let insetBottom: CGFloat
    let appearance: TerminalAppearance
    var onColors: (UInt32, UInt32) -> Void
    var onSwipe: (Int) -> Void
    var onPalette: () -> Void
    var onFontSize: (CGFloat) -> Void

    final class Coordinator { var applied: TerminalAppearance? }
    func makeCoordinator() -> Coordinator { Coordinator() }

    func makeUIView(context: Context) -> TerminalUIView {
        let v = TerminalUIView(controller: controller, fontSize: fontSize)
        v.showsKeyBar = controller.showsKeyBar
        v.onFontSizeChanged = onFontSize
        // Don't pop the keyboard on open: look first, tap the terminal to type.
        return v
    }

    func updateUIView(_ v: TerminalUIView, context: Context) {
        if context.coordinator.applied != appearance {
            context.coordinator.applied = appearance
            v.applyAppearance(fontId: appearance.font, lineHeight: appearance.lineHeight, cursor: appearance.cursor, blink: appearance.blink)
        }
        v.chrome = chrome
        v.insetTop = insetTop
        v.insetBottom = insetBottom
        v.onColors = onColors
        v.onSwipePane = onSwipe
        v.onPalette = onPalette
    }
}
#else
/// macOS stand-in so RaptorKit builds and can be exercised on a Mac.
struct TerminalSurface: NSViewRepresentable {
    let controller: TerminalController
    let fontSize: CGFloat
    let chrome: Chrome
    let insetTop: CGFloat
    let insetBottom: CGFloat
    let appearance: TerminalAppearance
    var onColors: (UInt32, UInt32) -> Void
    var onSwipe: (Int) -> Void
    var onPalette: () -> Void
    var onFontSize: (CGFloat) -> Void

    final class View: NSView {
        var controller: TerminalController?
        let renderer: TerminalRenderer
        init(fontSize: CGFloat) {
            renderer = TerminalRenderer(fontSize: fontSize)
            super.init(frame: .zero)
        }
        required init?(coder: NSCoder) { fatalError() }
        override var isFlipped: Bool { true }
        override func draw(_ dirtyRect: NSRect) {
            guard let ctx = NSGraphicsContext.current?.cgContext, let f = controller?.frame() else { return }
            renderer.draw(f, in: ctx, size: bounds.size, options: .init())
        }
    }

    func makeNSView(context: Context) -> View {
        let v = View(fontSize: fontSize)
        v.controller = controller
        controller.onFrame = { [weak v] in v?.needsDisplay = true }
        return v
    }

    func updateNSView(_ v: View, context: Context) {}
}
#endif

/// The terminal settings a surface draws with; a change re-applies them live.
struct TerminalAppearance: Equatable {
    var font: String?
    var lineHeight: CGFloat
    var cursor: String?
    var blink: Bool

    @MainActor init(_ model: RaptorModel) {
        _ = model.appearanceVersion // observe: Settings bumps it
        font = model.core.setting(key: Prefs.font)
        lineHeight = CGFloat(Double(model.core.setting(key: Prefs.lineHeight) ?? "") ?? 1.06)
        cursor = model.core.setting(key: Prefs.cursor)
        blink = Prefs.flag(model, Prefs.blink)
    }
}
