import AppKit
import RaptorKit
import SwiftUI

/// A Raptor window: the session switcher and tabs in the title bar, the
/// session's panes below.
struct MainView: View {
    @Environment(AppModel.self) private var app
    @Environment(RaptorModel.self) private var model
    @State private var ws: Workspace?
    @SceneStorage("server") private var savedServer = ""
    @SceneStorage("session") private var savedSession = ""
    @State private var editing: Server?

    var body: some View {
        let chrome = Chrome(model.theme)
        VStack(spacing: 0) {
            if let ws {
                TitleBar(ws: ws, onEditServer: { editing = $0 })
                Detail(ws: ws, onAddServer: { ws.addingServer = true })
            }
        }
        .ignoresSafeArea(.container, edges: .top)
        .background(chrome.outer)
        .navigationTitle(ws?.session?.label ?? "Raptor")
        .preferredColorScheme(chrome.scheme)
        .tint(chrome.accent)
        .environment(\.chrome, chrome)
        .background(WindowReader { window in
            guard let ws else { return }
            ws.window = window
            window.tabbingMode = .disallowed
            window.backgroundColor = NSColor(cgColor: cgColor(0xFF00_0000 | chrome.outerHex))
        })
        .onAppear(perform: start)
        .onChange(of: model.state) { _, _ in
            ws?.reconcile()
            restoreIfNeeded()
        }
        .onChange(of: ws?.sessionId) { _, id in
            savedSession = id ?? ""
            savedServer = ws?.serverId ?? ""
        }
        .onReceive(NotificationCenter.default.publisher(for: NSWindow.didBecomeKeyNotification)) { n in
            if let ws, (n.object as? NSWindow) === ws.window { app.current = ws }
        }
        .onDisappear { ws?.close() }
        .sheet(item: $editing) { ServerSheet(editing: $0) }
        .modifier(WorkspaceDialogs(ws: ws))
    }

    private func start() {
        if ws == nil {
            let w = app.makeWorkspace()
            ws = w
            app.current = w
        }
        restoreIfNeeded()
    }

    /// Open the session this window showed last time, else the app's last one, else the first.
    private func restoreIfNeeded() {
        guard let ws, ws.sessionId == nil else { return }
        let servers = model.state.servers
        if !savedSession.isEmpty, let s = model.session(savedServer, savedSession) {
            ws.open(serverId: savedServer, sessionId: s.id)
        } else if let (srv, ses) = app.lastSession, model.session(srv, ses) != nil,
                  !app.allWorkspaces.contains(where: { $0 !== ws && $0.sessionId == ses }) {
            ws.open(serverId: srv, sessionId: ses)
        } else if let srv = servers.first(where: { !$0.sessions.isEmpty && $0.connectivity.isOnline }), savedSession.isEmpty {
            let taken = Set(app.allWorkspaces.compactMap(\.sessionId))
            let s = srv.sessions.first { !taken.contains($0.id) } ?? srv.sessions[0]
            ws.open(serverId: srv.id, sessionId: s.id)
        }
    }
}

/// What to call a server: its machine name when the saved name is just an address.
func serverTitle(_ srv: Server) -> String {
    let looksDefault = srv.name.contains("@") || (srv.name.first?.isNumber ?? false)
    if looksDefault, let h = srv.hostName, !h.isEmpty { return h }
    return srv.name
}

/// Hands the hosting NSWindow to SwiftUI code.
struct WindowReader: NSViewRepresentable {
    let onWindow: (NSWindow) -> Void

    func makeNSView(context: Context) -> NSView { Reader(onWindow: onWindow) }
    func updateNSView(_ v: NSView, context: Context) {
        if let w = v.window { onWindow(w) }
    }

    final class Reader: NSView {
        let onWindow: (NSWindow) -> Void
        init(onWindow: @escaping (NSWindow) -> Void) {
            self.onWindow = onWindow
            super.init(frame: .zero)
        }

        @available(*, unavailable)
        required init?(coder: NSCoder) { fatalError() }

        override func viewDidMoveToWindow() {
            super.viewDidMoveToWindow()
            if let window { onWindow(window) }
        }
    }
}

// MARK: - detail

private struct Detail: View {
    @Bindable var ws: Workspace
    var onAddServer: () -> Void
    @Environment(RaptorModel.self) private var model
    @Environment(\.chrome) private var chrome

    var body: some View {
        ZStack(alignment: .top) {
            ZStack(alignment: .top) {
                chrome.bg
                if model.state.servers.isEmpty {
                    Welcome(onAddServer: onAddServer)
                } else if let srv = ws.server, let s = ws.session, let w = s.activeWindow {
                    ZStack(alignment: .top) {
                        PaneArea(ws: ws, window: w, focusedId: ws.focusedBlockId, chrome: chrome, version: ws.layoutVersion)
                        if !srv.connectivity.isOnline {
                            ConnectionBanner(server: srv)
                                .padding(.top, 12)
                        }
                        if ws.findShown {
                            FindBar(ws: ws)
                                .frame(maxWidth: .infinity, alignment: .trailing)
                                .padding(.top, 8).padding(.trailing, 14)
                        }
                    }
                } else if let srv = ws.server, !srv.connectivity.isOnline {
                    ServerStatusView(server: srv)
                } else if let s = ws.session {
                    EmptySession(ws: ws, session: s)
                } else {
                    Placeholder(ws: ws)
                }
            }
            .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
            .overlay(RoundedRectangle(cornerRadius: 10, style: .continuous).strokeBorder(chrome.hairline))
            .padding([.horizontal, .bottom], 6)
            if ws.paletteShown {
                Color.black.opacity(0.001).onTapGesture { ws.paletteShown = false }
                CommandPaletteView(ws: ws)
                    .padding(.top, 28)
            }
        }
    }
}

private struct Welcome: View {
    var onAddServer: () -> Void
    @Environment(\.chrome) private var c

    var body: some View {
        VStack(spacing: 14) {
            Image(nsImage: NSApp.applicationIconImage).resizable().frame(width: 96, height: 96)
            Text("Connect to a Rex server").font(.title2.weight(.semibold)).foregroundStyle(c.text)
            Text("Raptor shows the sessions on any Rex server you can reach: this Mac, a box on your network, or one on your tailnet.")
                .multilineTextAlignment(.center).foregroundStyle(c.secondary).frame(maxWidth: 380)
            Button("Add Server…", action: onAddServer)
                .buttonStyle(.borderedProminent).controlSize(.large).padding(.top, 6)
                .keyboardShortcut(.defaultAction)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}

private struct Placeholder: View {
    let ws: Workspace
    @Environment(RaptorModel.self) private var model
    @Environment(\.chrome) private var c

    var body: some View {
        VStack(spacing: 10) {
            Image(systemName: "rectangle.stack").font(.system(size: 36, weight: .light)).foregroundStyle(c.tertiary)
            Text("No session open").font(.title3.weight(.semibold)).foregroundStyle(c.text)
            Text("Pick one with ⌘O, or start a new one.").foregroundStyle(c.secondary)
            if model.state.servers.contains(where: { $0.connectivity.isOnline }) {
                Button("New Session") { ws.newSession() }.padding(.top, 4)
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}

/// A session whose last tab closed; the server keeps it until it's ended.
private struct EmptySession: View {
    let ws: Workspace
    let session: Session
    @Environment(\.chrome) private var c

    var body: some View {
        VStack(spacing: 10) {
            Image(systemName: "macwindow.badge.plus").font(.system(size: 36, weight: .light)).foregroundStyle(c.tertiary)
            Text("“\(session.label)” has no tabs").font(.title3.weight(.semibold)).foregroundStyle(c.text)
            HStack {
                Button("New Tab") { ws.newTab() }.keyboardShortcut(.defaultAction)
                Button("End Session") { ws.endSession(serverId: ws.serverId ?? "", sessionId: session.id, label: session.label) }
            }
            .padding(.top, 4)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}

private struct ServerStatusView: View {
    let server: Server
    @Environment(RaptorModel.self) private var model
    @Environment(\.chrome) private var c

    var body: some View {
        VStack(spacing: 10) {
            if server.connectivity.isConnecting || server.connectivity == .idle {
                ProgressView().controlSize(.regular)
                Text("Connecting to \(serverTitle(server))…").foregroundStyle(c.secondary)
            } else {
                Image(systemName: "bolt.horizontal.circle").font(.system(size: 36, weight: .light)).foregroundStyle(c.tertiary)
                Text("Can't reach \(serverTitle(server))").font(.title3.weight(.semibold)).foregroundStyle(c.text)
                Text(server.connectivity.label(latency: nil)).foregroundStyle(c.secondary).multilineTextAlignment(.center).frame(maxWidth: 420)
                Button("Try Again") { model.core.reconnect(serverId: server.id) }.padding(.top, 4)
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}

/// Shown over the panes while their server is unreachable; the panes keep their last content.
private struct ConnectionBanner: View {
    let server: Server
    @Environment(RaptorModel.self) private var model
    @Environment(\.chrome) private var c
    @State private var now = Date()

    var body: some View {
        HStack(spacing: 8) {
            switch server.connectivity {
            case .offline(let error, let retry):
                Image(systemName: "wifi.exclamationmark").foregroundStyle(Palette.error)
                Text(error).lineLimit(1)
                if retry > 0 { Text("· retrying in \(max(1, Int((Double(retry) / 1000).rounded())))s").foregroundStyle(c.secondary) }
                Button("Retry") { model.core.reconnect(serverId: server.id) }.controlSize(.small)
            default:
                ProgressView().controlSize(.small)
                Text("Reconnecting to \(serverTitle(server))…")
            }
        }
        .font(.system(size: 12, weight: .medium))
        .foregroundStyle(c.text)
        .padding(.horizontal, 14).padding(.vertical, 7)
        .background(.regularMaterial, in: Capsule())
        .overlay(Capsule().strokeBorder(c.glassStroke))
        .shadow(color: .black.opacity(0.25), radius: 10, y: 4)
    }
}

extension AgentState {
    /// Higher is more urgent.
    var rank: Int {
        switch self {
        case .blocked: return 4
        case .error: return 3
        case .working: return 2
        case .done: return 1
        case .idle: return 0
        }
    }
}

struct RenameSheet: View {
    let request: RenameRequest
    @State private var text = ""
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text(request.title).font(.headline)
            TextField("Name", text: $text).textFieldStyle(.roundedBorder).frame(width: 280)
                .onSubmit(commit)
            HStack {
                Spacer()
                Button("Cancel", role: .cancel) { dismiss() }.keyboardShortcut(.cancelAction)
                Button("Rename", action: commit).keyboardShortcut(.defaultAction)
            }
        }
        .padding(20)
        .onAppear { text = request.value }
    }

    private func commit() {
        request.commit(text.trimmingCharacters(in: .whitespaces))
        dismiss()
    }
}

/// The sheets and alerts a workspace can raise.
private struct WorkspaceDialogs: ViewModifier {
    let ws: Workspace?

    func body(content: Content) -> some View {
        let adding = Binding(get: { ws?.addingServer ?? false }, set: { ws?.addingServer = $0 })
        let renaming = Binding(get: { ws?.renaming }, set: { ws?.renaming = $0 })
        let confirming = Binding(get: { ws?.confirm != nil }, set: { if !$0 { ws?.confirm = nil } })
        let failed = Binding(get: { ws?.error != nil }, set: { if !$0 { ws?.error = nil } })
        let replying = Binding(get: { ws?.replying.map(ReplyTarget.init) }, set: { ws?.replying = $0?.item })
        content
            .sheet(isPresented: adding) { ServerSheet(editing: nil) }
            .sheet(item: renaming) { RenameSheet(request: $0) }
            .sheet(item: replying) { ReplySheet(item: $0.item) }
            .alert(ws?.confirm?.title ?? "", isPresented: confirming, presenting: ws?.confirm) { c in
                Button(c.action, role: .destructive) { c.run() }
                Button("Cancel", role: .cancel) {}
            } message: { c in
                Text(c.message)
            }
            .alert("Something went wrong", isPresented: failed) {
                Button("OK", role: .cancel) {}
            } message: {
                Text(ws?.error ?? "")
            }
    }
}

private struct ReplyTarget: Identifiable {
    let item: AttentionItem
    var id: String { item.block.id }
}
