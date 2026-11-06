import RaptorKit
import SwiftUI

struct Sidebar: View {
    let ws: Workspace
    var onAddServer: () -> Void
    var onEditServer: (Server) -> Void
    @Environment(RaptorModel.self) private var model
    @Environment(\.chrome) private var c
    @State private var replying: AttentionItem?

    private var selection: Binding<String?> {
        Binding(
            get: { ws.sessionId.map { "\(ws.serverId ?? "")|\($0)" } },
            set: { tag in
                guard let tag else { return }
                let parts = tag.split(separator: "|").map(String.init)
                if parts.count == 2 { ws.open(serverId: parts[0], sessionId: parts[1]) }
            }
        )
    }

    var body: some View {
        List(selection: selection) {
            let inbox = model.state.attention.filter { $0.block.agent?.state != .idle }
            if !inbox.isEmpty {
                Section("Needs You") {
                    ForEach(inbox, id: \.block.id) { item in
                        InboxRow(item: item)
                            .contentShape(Rectangle())
                            .onTapGesture { open(item) }
                            .contextMenu {
                                Button("Show") { open(item) }
                                Button("Reply…") { replying = item }
                            }
                    }
                }
            }
            ForEach(model.state.servers, id: \.id) { srv in
                Section {
                    if let error = srv.connectivity.offlineError, srv.sessions.isEmpty {
                        Label { Text(error).lineLimit(2).foregroundStyle(.secondary) } icon: { Image(systemName: "exclamationmark.triangle").foregroundStyle(Palette.error) }
                            .font(.system(size: 12))
                            .selectionDisabled()
                    }
                    ForEach(srv.sessions, id: \.id) { s in
                        SessionRow(server: srv, session: s, ws: ws)
                            .tag("\(srv.id)|\(s.id)")
                    }
                } header: {
                    ServerHeader(server: srv, ws: ws, onEdit: { onEditServer(srv) })
                }
            }
        }
        .listStyle(.sidebar)
        .safeAreaInset(edge: .bottom, spacing: 0) {
            HStack {
                Button(action: onAddServer) { Label("Add Server", systemImage: "plus") }
                    .buttonStyle(.borderless)
                Spacer()
                SettingsLink { Image(systemName: "gearshape") }
                    .buttonStyle(.borderless)
                    .help("Settings")
            }
            .font(.system(size: 12))
            .foregroundStyle(.secondary)
            .padding(.horizontal, 14).padding(.vertical, 9)
        }
        .sheet(item: Binding(get: { replying.map(ReplyTarget.init) }, set: { replying = $0?.item })) { ReplySheet(item: $0.item) }
    }

    private func open(_ item: AttentionItem) {
        ws.open(serverId: item.serverId, sessionId: item.sessionId, windowId: item.windowId, blockId: item.block.id)
    }
}

private struct ReplyTarget: Identifiable {
    let item: AttentionItem
    var id: String { item.block.id }
}

private struct ServerHeader: View {
    let server: Server
    let ws: Workspace
    var onEdit: () -> Void
    @Environment(RaptorModel.self) private var model

    var body: some View {
        HStack(spacing: 6) {
            Circle().fill(Palette.connectivity(server.connectivity)).frame(width: 6, height: 6)
            Text(serverTitle(server)).lineLimit(1)
            Spacer()
            Group {
                switch server.connectivity {
                case .online: if let ms = server.latencyMs { Text("\(ms) ms") }
                case .connecting: Text("connecting")
                case .offline: Text("offline")
                case .idle: EmptyView()
                }
            }
            .font(.system(size: 10, weight: .regular).monospacedDigit())
            .foregroundStyle(.tertiary)
            Menu {
                Button("New Session") { ws.newSession(on: server.id) }.disabled(!server.connectivity.isOnline)
                Button("Reconnect") { model.core.reconnect(serverId: server.id) }
                Divider()
                Button("Edit Server…", action: onEdit)
                Button("Remove Server", role: .destructive) { model.core.removeServer(serverId: server.id) }
            } label: {
                Image(systemName: "ellipsis.circle")
            }
            .menuStyle(.borderlessButton)
            .menuIndicator(.hidden)
            .fixedSize()
            .help("Server options")
        }
        .contextMenu {
            Button("New Session") { ws.newSession(on: server.id) }.disabled(!server.connectivity.isOnline)
            Button("Reconnect") { model.core.reconnect(serverId: server.id) }
            Button("Edit Server…", action: onEdit)
        }
    }
}

private struct SessionRow: View {
    let server: Server
    let session: Session
    let ws: Workspace

    var body: some View {
        let focused = session.focusedBlock
        HStack(spacing: 8) {
            ZStack {
                RoundedRectangle(cornerRadius: 5, style: .continuous).fill(.quaternary)
                Text(glyph(focused)).font(.system(size: 9.5, weight: .bold, design: .monospaced))
            }
            .frame(width: 22, height: 22)
            .overlay(alignment: .topTrailing) {
                if let a = session.agentState, a != .idle {
                    Circle().fill(Palette.agent(a)).frame(width: 8, height: 8)
                        .overlay(Circle().strokeBorder(.background, lineWidth: 1.5))
                        .offset(x: 3, y: -3)
                }
            }
            VStack(alignment: .leading, spacing: 1) {
                Text(session.label.isEmpty ? "Session" : session.label).lineLimit(1)
                let d = detail(focused)
                if !d.isEmpty {
                    Text(d).font(.system(size: 11)).foregroundStyle(.secondary).lineLimit(1).truncationMode(.middle)
                }
            }
            Spacer(minLength: 0)
            if session.windows.count > 1 {
                Text("\(session.windows.count)").font(.system(size: 10, weight: .medium).monospacedDigit()).foregroundStyle(.secondary)
                    .padding(.horizontal, 5).padding(.vertical, 1)
                    .background(.quaternary, in: Capsule())
                    .help("\(session.windows.count) tabs")
            }
        }
        .padding(.vertical, 2)
        .contextMenu {
            Button("Open") { ws.open(serverId: server.id, sessionId: session.id) }
            Button("Rename…") {
                ws.open(serverId: server.id, sessionId: session.id)
                ws.renameSession()
            }
            Divider()
            Button("End Session…", role: .destructive) { ws.endSession(serverId: server.id, sessionId: session.id, label: session.label) }
        }
        .help(session.clients.isEmpty ? "" : "Also open on " + session.clients.joined(separator: ", "))
    }
}

private struct InboxRow: View {
    let item: AttentionItem

    var body: some View {
        let a = item.block.agent
        HStack(alignment: .top, spacing: 8) {
            Circle().fill(Palette.agent(a?.state)).frame(width: 8, height: 8).padding(.top, 4)
                .overlay {
                    if a?.state == .working {
                        Circle().stroke(Palette.agent(a?.state).opacity(0.4), lineWidth: 2).frame(width: 14, height: 14).padding(.top, 4)
                    }
                }
            VStack(alignment: .leading, spacing: 2) {
                HStack {
                    Text(a.map { $0.title.isEmpty ? ($0.app.isEmpty ? item.block.name : $0.app) : $0.title } ?? item.block.name)
                        .lineLimit(1)
                    Spacer(minLength: 4)
                    Text(item.sessionLabel).font(.system(size: 10)).foregroundStyle(.tertiary).lineLimit(1)
                }
                Text(a.map { $0.message.isEmpty ? $0.label : $0.message } ?? "")
                    .font(.system(size: 11))
                    .foregroundStyle(a?.state.urgent == true ? Palette.agent(a?.state) : .secondary)
                    .lineLimit(2)
            }
        }
        .padding(.vertical, 2)
    }
}

/// Answer an agent without opening its terminal.
struct ReplySheet: View {
    let item: AttentionItem
    @Environment(RaptorModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    @State private var text = ""
    @State private var preview = ""
    @State private var error: String?

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text(item.block.agent?.title.isEmpty == false ? item.block.agent!.title : item.block.name).font(.headline)
            if let m = item.block.agent?.message, !m.isEmpty { Text(m).foregroundStyle(.secondary) }
            ScrollView {
                Text(preview).font(.system(size: 11, design: .monospaced)).frame(maxWidth: .infinity, alignment: .leading).textSelection(.enabled)
            }
            .frame(height: 160)
            .padding(8)
            .background(.quaternary.opacity(0.5), in: RoundedRectangle(cornerRadius: 8))
            TextField("Reply, e.g. 1 or y", text: $text).textFieldStyle(.roundedBorder).onSubmit(send)
            if let error { Text(error).font(.caption).foregroundStyle(Palette.error) }
            HStack {
                Spacer()
                Button("Cancel", role: .cancel) { dismiss() }.keyboardShortcut(.cancelAction)
                Button("Send", action: send).keyboardShortcut(.defaultAction).disabled(text.isEmpty)
            }
        }
        .padding(20)
        .frame(width: 460)
        .task {
            let t = try? await model.core.previewText(serverId: item.serverId, sessionId: item.sessionId, blockId: item.block.id)
            preview = (t ?? "").split(separator: "\n", omittingEmptySubsequences: false).suffix(14).joined(separator: "\n")
        }
    }

    private func send() {
        let core = model.core
        Task {
            do {
                try await core.quickReply(serverId: item.serverId, sessionId: item.sessionId, blockId: item.block.id, text: text + "\r")
                dismiss()
            } catch { self.error = "\(error)" }
        }
    }
}
