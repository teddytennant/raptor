import SwiftUI

/// Home: what needs you, then every server and its sessions, as a quiet list.
public struct HomeView: View {
    @Environment(ShrewModel.self) private var model
    var onOpen: (String, String, String?) -> Void
    @State private var addingServer = false
    @State private var editing: Server?

    public init(onOpen: @escaping (_ server: String, _ session: String, _ block: String?) -> Void) {
        self.onOpen = onOpen
    }

    public var body: some View {
        let c = Chrome(model.theme)
        ScrollView {
            HomeList(onOpen: onOpen, onAdd: { addingServer = true }, onEdit: { editing = $0 })
                .padding(.bottom, 40)
        }
        .background(c.bg.ignoresSafeArea())
        .scrollContentBackground(.hidden)
        .refreshable {
            for s in model.state.servers { model.core.reconnect(serverId: s.id) }
            try? await Task.sleep(for: .milliseconds(600))
        }
        .navigationTitle("Shrew")
        #if os(iOS)
        .toolbarBackground(c.bg, for: .navigationBar)
        #endif
        .toolbar {
            ToolbarItem(placement: .primaryAction) {
                Button { addingServer = true } label: { Image(systemName: "plus") }
                    .accessibilityLabel("Add server")
            }
        }
        .environment(\.chrome, c)
        .tint(c.accent)
        .preferredColorScheme(c.scheme)
        .sheet(isPresented: $addingServer) { AddServerView(editing: nil) { addingServer = false } }
        .sheet(item: $editing) { srv in AddServerView(editing: srv) { editing = nil } }
    }
}

extension Server: Identifiable {}

/// The home screen's content, separate from its scroll container so it can be
/// rendered offscreen (snapshots) and embedded elsewhere (iPad sidebar).
public struct HomeList: View {
    @Environment(ShrewModel.self) private var model
    var onOpen: (String, String, String?) -> Void
    var onAdd: () -> Void
    var onEdit: (Server) -> Void
    var current: Focus?

    public init(current: Focus? = nil, onOpen: @escaping (String, String, String?) -> Void, onAdd: @escaping () -> Void, onEdit: @escaping (Server) -> Void) {
        self.current = current
        self.onOpen = onOpen
        self.onAdd = onAdd
        self.onEdit = onEdit
    }

    public var body: some View {
        let state = model.state
        let needs = state.attention.filter { $0.block.agent?.state.urgent == true }
        let c = Chrome(model.theme)
        LazyVStack(alignment: .leading, spacing: 0) {
            if state.servers.isEmpty {
                EmptyHome(onAdd: onAdd).padding(.top, 60)
            }
            if !needs.isEmpty {
                SectionHeader(title: "Needs you")
                ForEach(needs, id: \.block.id) { item in
                    NeedsRow(item: item) { onOpen(item.serverId, item.sessionId, item.block.id) }
                }
            }
            ForEach(state.servers, id: \.id) { srv in
                ServerSection(server: srv, current: current, onEdit: { onEdit(srv) }, onOpen: { onOpen(srv.id, $0, nil) })
            }
        }
        .environment(\.chrome, c)
        .animation(.snappy, value: state)
    }
}

private struct SectionHeader: View {
    var title: String
    @Environment(\.chrome) private var c
    var body: some View {
        Text(title.uppercased())
            .font(.system(size: 12, weight: .medium))
            .tracking(0.6)
            .foregroundStyle(c.tertiary)
            .padding(.horizontal, 20)
            .padding(.top, 18)
            .padding(.bottom, 6)
    }
}

private struct EmptyHome: View {
    var onAdd: () -> Void
    @Environment(\.chrome) private var c
    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text("No servers yet").font(.title2.weight(.semibold)).foregroundStyle(c.text)
            Text("Shrew connects to a Rex server — on your Mac, or any machine on your tailnet — and keeps your sessions one tap away.")
                .font(.subheadline)
                .foregroundStyle(c.secondary)
            Button("Add a server", action: onAdd).font(.headline).padding(.top, 8)
        }
        .padding(.horizontal, 24)
    }
}

/// What to call a server: its machine name when known, else what the user typed.
func serverTitle(_ srv: Server) -> String {
    let looksDefault = srv.name.contains("@") || (srv.name.first?.isNumber ?? false)
    if looksDefault, let h = srv.hostName, !h.isEmpty { return h }
    return srv.name
}

/// One server: a quiet heading (press and hold for its options), then its
/// sessions in a single rounded group ending with "New session".
private struct ServerSection: View {
    @Environment(ShrewModel.self) private var model
    @Environment(\.chrome) private var c
    var server: Server
    var current: Focus?
    var onEdit: () -> Void
    var onOpen: (String) -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                Text(serverTitle(server).uppercased())
                    .font(.system(size: 12, weight: .semibold)).tracking(0.6).foregroundStyle(c.secondary).lineLimit(1)
                Spacer()
                switch server.connectivity {
                case .online: if let ms = server.latencyMs { Text("\(ms) ms").font(.mono(12)).foregroundStyle(c.tertiary) }
                case .connecting: Text("connecting").font(.system(size: 12)).foregroundStyle(Palette.connectivity(server.connectivity))
                case .offline: Text("offline").font(.system(size: 12)).foregroundStyle(Palette.error)
                case .idle: EmptyView()
                }
            }
            .padding(.horizontal, 32)
            .contentShape(Rectangle())
            .contextMenu {
                Button("Edit…", systemImage: "pencil", action: onEdit)
                Button("Reconnect", systemImage: "arrow.clockwise") { model.core.reconnect(serverId: server.id) }
                Divider()
                Button("Remove", systemImage: "trash", role: .destructive) { model.core.removeServer(serverId: server.id) }
            }

            VStack(spacing: 0) {
                if case .offline(let error, _) = server.connectivity {
                    HStack {
                        Text(error).font(.subheadline).foregroundStyle(c.secondary).lineLimit(2)
                        Spacer()
                        Button("Retry") { model.core.reconnect(serverId: server.id) }.font(.subheadline.weight(.semibold))
                    }
                    .padding(.horizontal, 16).padding(.vertical, 12)
                } else {
                    ForEach(server.sessions, id: \.id) { s in
                        SessionRow(server: server, session: s, selected: current?.serverId == server.id && current?.sessionId == s.id) { onOpen(s.id) }
                        Rectangle().fill(c.hairline).frame(height: 0.5).padding(.leading, 16)
                    }
                    if server.connectivity.isOnline {
                        Button {
                            Task { if let id = try? await model.core.createSession(serverId: server.id, label: nil) { onOpen(id) } }
                        } label: {
                            Label("New session", systemImage: "plus").font(.system(size: 15)).foregroundStyle(c.accent)
                                .frame(maxWidth: .infinity, alignment: .leading).padding(.horizontal, 16).padding(.vertical, 13)
                                .contentShape(Rectangle())
                        }
                        .buttonStyle(RowStyle())
                    } else {
                        Text("Connecting…").font(.subheadline).foregroundStyle(c.tertiary)
                            .frame(maxWidth: .infinity, alignment: .leading).padding(16)
                    }
                }
            }
            .background(c.surface, in: RoundedRectangle(cornerRadius: 14, style: .continuous))
            .clipShape(RoundedRectangle(cornerRadius: 14, style: .continuous))
            .padding(.horizontal, 16)
        }
        .padding(.top, 18)
    }
}

private struct SessionRow: View {
    @Environment(ShrewModel.self) private var model
    @Environment(\.chrome) private var c
    var server: Server
    var session: Session
    var selected = false
    var onOpen: () -> Void
    @State private var renaming = false
    @State private var newName = ""

    var body: some View {
        let focused = session.focusedBlock
        let online = server.connectivity.isOnline
        Button(action: onOpen) {
            VStack(spacing: 0) {
                HStack(spacing: 14) {
                    VStack(alignment: .leading, spacing: 2) {
                        HStack(spacing: 8) {
                            Text(session.label).font(.system(size: 16, weight: .medium)).foregroundStyle(online ? c.text : c.tertiary).lineLimit(1)
                            if let s = session.agentState, s != .idle {
                                Circle().fill(Palette.agent(s)).frame(width: 7, height: 7)
                            }
                        }
                        let d = detail(focused)
                        if !d.isEmpty { Text(d).font(.mono(12)).foregroundStyle(c.secondary).lineLimit(1) }
                    }
                    Spacer(minLength: 8)
                    if session.windows.count > 1 || session.blockCount > 1 {
                        Text("\(session.windows.count > 1 ? session.windows.count : Int(session.blockCount))")
                            .font(.mono(12))
                            .foregroundStyle(c.secondary)
                            .padding(.horizontal, 7)
                            .padding(.vertical, 2)
                            .background(c.raised, in: RoundedRectangle(cornerRadius: 6, style: .continuous))
                    }
                    if selected {
                        Image(systemName: "checkmark").font(.system(size: 15, weight: .semibold)).foregroundStyle(c.accent)
                    }
                }
                .padding(.horizontal, 16)
                .padding(.vertical, 11)
            }
            .background(selected ? c.raised : .clear)
            .contentShape(Rectangle())
        }
        .buttonStyle(RowStyle())
        .disabled(!online)
        .contextMenu {
            Button("Rename…", systemImage: "pencil") { newName = session.label; renaming = true }
            Button("New Window", systemImage: "plus.square.on.square") {
                Task { try? await model.core.newWindow(serverId: server.id, sessionId: session.id) }
            }
            Divider()
            Button("End Session", systemImage: "xmark.circle", role: .destructive) {
                Task { try? await model.core.destroySession(serverId: server.id, sessionId: session.id) }
            }
        }
        .alert("Rename session", isPresented: $renaming) {
            TextField("Name", text: $newName)
            Button("Rename") { Task { try? await model.core.renameSession(serverId: server.id, sessionId: session.id, label: newName) } }
            Button("Cancel", role: .cancel) {}
        }
    }
}

/// An agent waiting on you: what it said, the last lines, and a reply box.
private struct NeedsRow: View {
    @Environment(ShrewModel.self) private var model
    @Environment(\.chrome) private var c
    var item: AttentionItem
    var onOpen: () -> Void
    @State private var reply = ""
    @State private var preview: String?

    var body: some View {
        let a = item.block.agent!
        let tint = Palette.agent(a.state)
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 8) {
                Circle().fill(tint).frame(width: 7, height: 7)
                Text(a.app.isEmpty ? item.block.name : a.app).font(.system(size: 15, weight: .medium)).foregroundStyle(c.text)
                Text(a.label).font(.system(size: 13)).foregroundStyle(tint)
                Spacer()
                Text(item.sessionLabel).font(.mono(11)).foregroundStyle(c.tertiary)
            }
            let msg = a.message.isEmpty ? a.title : a.message
            if !msg.isEmpty { Text(msg).font(.subheadline).foregroundStyle(c.text).lineLimit(3) }
            if let preview, !preview.isEmpty {
                Text(preview).font(.mono(11)).foregroundStyle(c.secondary).lineLimit(4)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(10)
                    .background(c.bg, in: RoundedRectangle(cornerRadius: 8, style: .continuous))
            }
            HStack(spacing: 8) {
                TextField("Reply", text: $reply)
                    .font(.mono(14))
                    .autocorrectionDisabled()
                    #if os(iOS)
                    .textInputAutocapitalization(.never)
                    #endif
                    .submitLabel(.send)
                    .onSubmit(send)
                Button(action: send) { Image(systemName: "arrow.up.circle.fill").font(.system(size: 22)) }
                    .disabled(reply.isEmpty)
                    .foregroundStyle(reply.isEmpty ? c.tertiary : c.accent)
            }
            .padding(.leading, 12)
            .padding(.trailing, 6)
            .padding(.vertical, 6)
            .background(c.raised, in: RoundedRectangle(cornerRadius: 8, style: .continuous))
        }
        .padding(14)
        .background(c.surface, in: RoundedRectangle(cornerRadius: 12, style: .continuous))
        .onTapGesture(perform: onOpen)
        .padding(.horizontal, 12)
        .padding(.vertical, 4)
        .task(id: a.updatedAt) {
            let text = try? await model.core.previewText(serverId: item.serverId, sessionId: item.sessionId, blockId: item.block.id)
            preview = text?.split(separator: "\n", omittingEmptySubsequences: true).suffix(4).joined(separator: "\n")
        }
    }

    private func send() {
        let t = reply
        guard !t.isEmpty else { return }
        reply = ""
        Haptics.tap()
        Task { try? await model.core.quickReply(serverId: item.serverId, sessionId: item.sessionId, blockId: item.block.id, text: t + "\r") }
    }
}

/// Row press feedback: a subtle highlight, like a native list.
struct RowStyle: ButtonStyle {
    @Environment(\.chrome) private var c
    func makeBody(configuration: Configuration) -> some View {
        configuration.label.background(configuration.isPressed ? c.raised : .clear)
    }
}

/// Subtle press feedback for card buttons.
struct PressableStyle: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .foregroundStyle(.primary)
            .scaleEffect(configuration.isPressed ? 0.98 : 1)
            .animation(.snappy(duration: 0.18), value: configuration.isPressed)
    }
}

enum Haptics {
    static func tap() {
        #if os(iOS)
        UIImpactFeedbackGenerator(style: .light).impactOccurred()
        #endif
    }

    static func select() {
        #if os(iOS)
        UISelectionFeedbackGenerator().selectionChanged()
        #endif
    }
}

extension Array where Element: Hashable {
    func uniqued() -> [Element] {
        var seen = Set<Element>()
        return filter { seen.insert($0).inserted }
    }
}
