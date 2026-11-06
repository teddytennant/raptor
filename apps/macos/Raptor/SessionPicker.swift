import RaptorKit
import SwiftUI

/// ⌘O, or a click on the session in the title bar: every server and its
/// sessions, filterable, with the arrow keys and Return.
struct SessionPicker: View {
    let ws: Workspace
    var onEditServer: (Server) -> Void
    @Environment(RaptorModel.self) private var model
    @State private var query = ""
    @State private var highlight: String?
    @FocusState private var searching: Bool

    private struct Row: Identifiable {
        let server: Server
        let session: Session
        var id: String { "\(server.id)|\(session.id)" }
    }

    private var groups: [(server: Server, rows: [Row])] {
        let q = query.trimmingCharacters(in: .whitespaces).lowercased()
        return model.state.servers.compactMap { srv in
            let rows = srv.sessions.filter { s in
                q.isEmpty || s.label.lowercased().contains(q) || detail(s.focusedBlock).lowercased().contains(q)
                    || serverTitle(srv).lowercased().contains(q)
            }.map { Row(server: srv, session: $0) }
            return q.isEmpty || !rows.isEmpty ? (srv, rows) : nil
        }
    }

    var body: some View {
        let groups = groups
        let rows = groups.flatMap(\.rows)
        VStack(spacing: 0) {
            HStack(spacing: 7) {
                Image(systemName: "magnifyingglass").foregroundStyle(.secondary)
                TextField("Switch to a session", text: $query)
                    .textFieldStyle(.plain)
                    .focused($searching)
                    .onSubmit { if let r = rows.first(where: { $0.id == highlight }) ?? rows.first { open(r) } }
                    .onKeyPress(.downArrow) { move(1, rows); return .handled }
                    .onKeyPress(.upArrow) { move(-1, rows); return .handled }
            }
            .font(.system(size: 13))
            .padding(.horizontal, 12).padding(.vertical, 10)
            Divider()
            if model.state.servers.isEmpty {
                VStack(spacing: 8) {
                    Text("No servers yet").font(.headline)
                    Text("Add a Rex server to see its sessions here.").font(.callout).foregroundStyle(.secondary)
                }
                .frame(maxWidth: .infinity).padding(.vertical, 28)
            } else if rows.isEmpty && !query.isEmpty {
                Text("No sessions match “\(query)”").foregroundStyle(.secondary)
                    .frame(maxWidth: .infinity).padding(.vertical, 28)
            } else {
                ScrollViewReader { proxy in
                    ScrollView {
                        VStack(alignment: .leading, spacing: 1) {
                            ForEach(groups, id: \.server.id) { g in
                                ServerHeading(server: g.server, ws: ws, onEdit: {
                                    ws.pickerShown = false
                                    onEditServer(g.server)
                                })
                                if let error = g.server.connectivity.offlineError, g.rows.isEmpty {
                                    Text(error).font(.system(size: 12)).foregroundStyle(.secondary).lineLimit(3)
                                        .padding(.horizontal, 12).padding(.bottom, 6)
                                }
                                ForEach(g.rows) { r in
                                    SessionRow(server: r.server, session: r.session, ws: ws,
                                               current: r.session.id == ws.sessionId,
                                               highlighted: r.id == highlight)
                                        .id(r.id)
                                        .onTapGesture { open(r) }
                                        .onHover { if $0 { highlight = r.id } }
                                }
                            }
                        }
                        .padding(6)
                    }
                    .frame(height: min(420, estimate(groups)))
                    .onChange(of: highlight) { _, id in
                        if let id { proxy.scrollTo(id) }
                    }
                }
            }
            Divider()
            HStack(spacing: 14) {
                Button {
                    ws.pickerShown = false
                    ws.newSession()
                } label: { Label("New Session", systemImage: "plus") }
                    .disabled(!model.state.servers.contains { $0.connectivity.isOnline })
                Spacer()
                Button {
                    ws.pickerShown = false
                    ws.addingServer = true
                } label: { Label("Add Server…", systemImage: "externaldrive.badge.plus") }
                SettingsLink { Image(systemName: "gearshape") }
                    .help("Settings")
            }
            .buttonStyle(.borderless)
            .font(.system(size: 12))
            .foregroundStyle(.secondary)
            .padding(.horizontal, 12).padding(.vertical, 8)
        }
        .frame(width: 340)
        .onAppear {
            highlight = ws.sessionId.map { "\(ws.serverId ?? "")|\($0)" }
            searching = true
        }
        .onChange(of: query) { _, _ in highlight = self.groups.flatMap(\.rows).first?.id }
    }

    private func estimate(_ groups: [(server: Server, rows: [Row])]) -> CGFloat {
        groups.reduce(12) { h, g in h + 30 + CGFloat(g.rows.count) * 44 + (g.rows.isEmpty ? 40 : 0) }
    }

    private func move(_ d: Int, _ rows: [Row]) {
        guard !rows.isEmpty else { return }
        let i = rows.firstIndex { $0.id == highlight } ?? (d > 0 ? -1 : rows.count)
        highlight = rows[min(max(i + d, 0), rows.count - 1)].id
    }

    private func open(_ r: Row) {
        ws.pickerShown = false
        ws.open(serverId: r.server.id, sessionId: r.session.id)
    }
}

private struct ServerHeading: View {
    let server: Server
    let ws: Workspace
    var onEdit: () -> Void
    @Environment(RaptorModel.self) private var model

    var body: some View {
        HStack(spacing: 6) {
            Circle().fill(Palette.connectivity(server.connectivity)).frame(width: 6, height: 6)
            Text(server.isLocal ? "This Mac" : serverTitle(server))
                .font(.system(size: 11, weight: .semibold))
                .foregroundStyle(.secondary)
                .lineLimit(1)
            Group {
                switch server.connectivity {
                case .online: if let ms = server.latencyMs, !server.isLocal { Text("\(ms) ms") }
                case .connecting: Text("connecting")
                case .offline: Text("offline")
                case .idle: EmptyView()
                }
            }
            .font(.system(size: 10).monospacedDigit())
            .foregroundStyle(.tertiary)
            Spacer()
            Button {
                ws.pickerShown = false
                ws.newSession(on: server.id)
            } label: { Image(systemName: "plus").frame(width: 18, height: 18) }
                .buttonStyle(.borderless)
                .disabled(!server.connectivity.isOnline)
                .help("New session on \(serverTitle(server))")
            Menu {
                Button("New Session") {
                    ws.pickerShown = false
                    ws.newSession(on: server.id)
                }
                .disabled(!server.connectivity.isOnline)
                Button("Reconnect") { model.core.reconnect(serverId: server.id) }
                Divider()
                Button("Edit Server…", action: onEdit)
                Button("Remove Server", role: .destructive) { model.core.removeServer(serverId: server.id) }
            } label: {
                Image(systemName: "ellipsis")
            }
            .menuStyle(.borderlessButton)
            .menuIndicator(.hidden)
            .tint(.secondary)
            .fixedSize()
            .help("Server options")
        }
        .foregroundStyle(.secondary)
        .padding(.leading, 8).padding(.trailing, 4)
        .frame(height: 28)
        .padding(.top, 2)
    }
}

private struct SessionRow: View {
    let server: Server
    let session: Session
    let ws: Workspace
    let current: Bool
    let highlighted: Bool

    var body: some View {
        let focused = session.focusedBlock
        HStack(spacing: 9) {
            ZStack {
                RoundedRectangle(cornerRadius: 6, style: .continuous).fill(.quaternary)
                Text(glyph(focused)).font(.system(size: 10, weight: .bold, design: .monospaced)).foregroundStyle(.secondary)
            }
            .frame(width: 26, height: 26)
            .overlay(alignment: .topTrailing) {
                if let a = session.agentState, a != .idle {
                    Circle().fill(Palette.agent(a)).frame(width: 8, height: 8)
                        .overlay(Circle().strokeBorder(.background, lineWidth: 1.5))
                        .offset(x: 3, y: -3)
                }
            }
            VStack(alignment: .leading, spacing: 1) {
                Text(session.label.isEmpty ? "Session" : session.label)
                    .font(.system(size: 13, weight: current ? .semibold : .regular))
                    .lineLimit(1)
                let d = detail(focused)
                if !d.isEmpty {
                    Text(d).font(.system(size: 11)).foregroundStyle(.secondary).lineLimit(1).truncationMode(.middle)
                }
            }
            Spacer(minLength: 0)
            if session.windows.count > 1 {
                Text("\(session.windows.count) tabs").font(.system(size: 10.5).monospacedDigit()).foregroundStyle(.tertiary)
            }
            Image(systemName: "checkmark").font(.system(size: 11, weight: .semibold)).foregroundStyle(.secondary)
                .opacity(current ? 1 : 0)
        }
        .padding(.horizontal, 8)
        .frame(height: 42)
        .background(RoundedRectangle(cornerRadius: 7, style: .continuous).fill(highlighted ? AnyShapeStyle(.quaternary) : AnyShapeStyle(.clear)))
        .contentShape(Rectangle())
        .contextMenu {
            Button("Open") {
                ws.pickerShown = false
                ws.open(serverId: server.id, sessionId: session.id)
            }
            Button("Rename…") {
                ws.pickerShown = false
                ws.open(serverId: server.id, sessionId: session.id)
                ws.renameSession()
            }
            Divider()
            Button("End Session…", role: .destructive) {
                ws.pickerShown = false
                ws.endSession(serverId: server.id, sessionId: session.id, label: session.label)
            }
        }
        .help(session.clients.isEmpty ? "" : "Also open on " + session.clients.joined(separator: ", "))
        .accessibilityElement(children: .combine)
        .accessibilityAddTraits(current ? [.isButton, .isSelected] : .isButton)
    }
}

/// The eye in the title bar: every agent that's waiting, working or done.
struct InboxPopover: View {
    let ws: Workspace
    @Environment(RaptorModel.self) private var model
    @State private var hovered: String?

    var body: some View {
        let inbox = model.state.attention.filter { $0.block.agent?.state != .idle }
        VStack(alignment: .leading, spacing: 0) {
            Text("Needs You").font(.system(size: 13, weight: .semibold))
                .padding(.horizontal, 14).padding(.top, 12).padding(.bottom, 6)
            if inbox.isEmpty {
                VStack(spacing: 6) {
                    Image(systemName: "checkmark.circle").font(.system(size: 26, weight: .light)).foregroundStyle(.tertiary)
                    Text("Nothing needs you").foregroundStyle(.secondary)
                    Text("Agents that ask for input or finish show up here.")
                        .font(.callout).foregroundStyle(.tertiary).multilineTextAlignment(.center)
                }
                .frame(maxWidth: .infinity).padding(.vertical, 22).padding(.horizontal, 16)
            } else {
                ScrollView {
                    VStack(spacing: 1) {
                        ForEach(inbox, id: \.block.id) { item in
                            InboxRow(item: item, hovered: hovered == item.block.id) { reply(item) }
                                .onTapGesture { open(item) }
                                .onHover { hovered = $0 ? item.block.id : (hovered == item.block.id ? nil : hovered) }
                                .contextMenu {
                                    Button("Show") { open(item) }
                                    Button("Reply…") { reply(item) }
                                }
                        }
                    }
                    .padding(.horizontal, 6).padding(.bottom, 6)
                }
                .frame(height: min(400, CGFloat(inbox.count) * 54 + 6))
            }
        }
        .frame(width: 360)
    }

    private func open(_ item: AttentionItem) {
        ws.inboxShown = false
        ws.open(serverId: item.serverId, sessionId: item.sessionId, windowId: item.windowId, blockId: item.block.id)
    }

    private func reply(_ item: AttentionItem) {
        ws.inboxShown = false
        ws.replying = item
    }
}

private struct InboxRow: View {
    let item: AttentionItem
    let hovered: Bool
    var onReply: () -> Void

    var body: some View {
        let a = item.block.agent
        HStack(alignment: .top, spacing: 9) {
            Circle().fill(Palette.agent(a?.state)).frame(width: 8, height: 8).padding(.top, 5)
                .overlay {
                    if a?.state == .working {
                        Circle().stroke(Palette.agent(a?.state).opacity(0.4), lineWidth: 2).frame(width: 14, height: 14).padding(.top, 5)
                    }
                }
            VStack(alignment: .leading, spacing: 2) {
                HStack {
                    Text(a.map { $0.title.isEmpty ? ($0.app.isEmpty ? item.block.name : $0.app) : $0.title } ?? item.block.name)
                        .font(.system(size: 13)).lineLimit(1)
                    Spacer(minLength: 4)
                    Text(item.sessionLabel).font(.system(size: 10.5)).foregroundStyle(.tertiary).lineLimit(1)
                }
                Text(a.map { $0.message.isEmpty ? $0.label : $0.message } ?? "")
                    .font(.system(size: 11.5))
                    .foregroundStyle(a?.state.urgent == true ? Palette.agent(a?.state) : .secondary)
                    .lineLimit(2)
            }
            if hovered, a?.state.urgent == true {
                Button(action: onReply) { Image(systemName: "arrowshape.turn.up.left") }
                    .buttonStyle(.borderless)
                    .help("Reply without opening")
                    .padding(.top, 3)
            }
        }
        .padding(.horizontal, 8).padding(.vertical, 7)
        .background(RoundedRectangle(cornerRadius: 7, style: .continuous).fill(hovered ? AnyShapeStyle(.quaternary) : AnyShapeStyle(.clear)))
        .contentShape(Rectangle())
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
            if !preview.isEmpty {
                ScrollView {
                    Text(preview).font(.system(size: 11, design: .monospaced)).frame(maxWidth: .infinity, alignment: .leading).textSelection(.enabled)
                }
                .defaultScrollAnchor(.bottom)
                .frame(height: 160)
                .padding(8)
                .background(.quaternary.opacity(0.5), in: RoundedRectangle(cornerRadius: 8))
            }
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
            var lines = (t ?? "").split(separator: "\n", omittingEmptySubsequences: false)
            while let last = lines.last, last.allSatisfy(\.isWhitespace) { lines.removeLast() }
            preview = lines.suffix(14).joined(separator: "\n")
        }
    }

    private func send() {
        let core = model.core
        Task {
            do {
                try await core.quickReply(serverId: item.serverId, sessionId: item.sessionId, blockId: item.block.id, text: text + "\r")
                dismiss()
            } catch { self.error = describe(error) }
        }
    }
}
