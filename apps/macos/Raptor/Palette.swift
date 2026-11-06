import RaptorKit
import SwiftUI

/// ⇧⌘P: every command, every session and tab, and the server's own actions.
struct CommandPaletteView: View {
    let ws: Workspace
    @Environment(AppModel.self) private var app
    @Environment(RaptorModel.self) private var model
    @Environment(\.chrome) private var c
    @Environment(\.openSettings) private var openSettings
    @State private var query = ""
    @State private var selected = 0
    @State private var serverActions: [PaletteAction] = []
    @FocusState private var focused: Bool

    struct Item: Identifiable {
        let id: String
        let title: String
        let subtitle: String
        let symbol: String
        var shortcut = ""
        let run: () -> Void
    }

    private var items: [Item] {
        var out: [Item] = [
            Item(id: "split-right", title: "Split Right", subtitle: "Pane", symbol: "rectangle.split.2x1", shortcut: "⌘D") { ws.split(vertical: false) },
            Item(id: "split-down", title: "Split Down", subtitle: "Pane", symbol: "rectangle.split.1x2", shortcut: "⇧⌘D") { ws.split(vertical: true) },
            Item(id: "zoom", title: "Zoom Pane", subtitle: "Pane", symbol: "arrow.up.left.and.arrow.down.right", shortcut: "⇧⌘↩") { ws.toggleZoom() },
            Item(id: "close-pane", title: "Close Pane", subtitle: "Pane", symbol: "xmark", shortcut: "⌘W") { ws.closePane() },
            Item(id: "new-tab", title: "New Tab", subtitle: "Tab", symbol: "plus.square.on.square", shortcut: "⌘T") { ws.newTab() },
            Item(id: "rename-tab", title: "Rename Tab…", subtitle: "Tab", symbol: "pencil") { ws.renameTab() },
            Item(id: "close-tab", title: "Close Tab", subtitle: "Tab", symbol: "xmark.square", shortcut: "⌥⌘W") { ws.closeTab() },
            Item(id: "new-session", title: "New Session", subtitle: "Session", symbol: "plus.rectangle.on.rectangle", shortcut: "⇧⌘N") { ws.newSession() },
            Item(id: "rename-session", title: "Rename Session…", subtitle: "Session", symbol: "pencil") { ws.renameSession() },
            Item(id: "find", title: "Find…", subtitle: "Terminal", symbol: "magnifyingglass", shortcut: "⌘F") { ws.findShown = true },
            Item(id: "bigger", title: "Bigger Text", subtitle: "View", symbol: "textformat.size.larger", shortcut: "⌘+") { app.biggerText() },
            Item(id: "smaller", title: "Smaller Text", subtitle: "View", symbol: "textformat.size.smaller", shortcut: "⌘−") { app.smallerText() },
            Item(id: "reconnect", title: "Reconnect All Servers", subtitle: "Servers", symbol: "arrow.clockwise", shortcut: "⇧⌘R") { app.reconnectAll() },
            Item(id: "settings", title: "Settings…", subtitle: "Raptor", symbol: "gearshape", shortcut: "⌘,") { openSettings() },
        ]
        for t in Themes.all where t.id != model.themeId {
            out.append(Item(id: "theme-\(t.id)", title: "Theme: \(t.name)", subtitle: "Appearance", symbol: "paintpalette") { app.setTheme(t.id) })
        }
        if let s = ws.session {
            for (i, w) in s.windows.enumerated() where w.id != s.activeWindowId {
                out.append(Item(id: "tab-\(w.id)", title: tabTitle(w), subtitle: "Tab \(i + 1)", symbol: "macwindow", shortcut: i < 9 ? "⌘\(i + 1)" : "") { ws.selectTab(w.id) })
            }
        }
        for srv in model.state.servers {
            for s in srv.sessions where s.id != ws.sessionId {
                out.append(Item(id: "session-\(s.id)", title: s.label, subtitle: "Session on \(serverTitle(srv))", symbol: "rectangle.stack") {
                    ws.open(serverId: srv.id, sessionId: s.id)
                })
            }
        }
        for a in serverActions {
            out.append(Item(id: "action-\(a.name)", title: a.title, subtitle: a.category.isEmpty ? "Server" : a.category.capitalized, symbol: "bolt") {
                guard let srv = ws.serverId, let ses = ws.sessionId else { return }
                let core = model.core, block = ws.focusedBlockId
                Task { try? await core.runAction(serverId: srv, sessionId: ses, blockId: block, action: a.name) }
            })
        }
        let words = query.lowercased().split(separator: " ")
        guard !words.isEmpty else { return out }
        return out.filter { item in
            let hay = (item.title + " " + item.subtitle).lowercased()
            return words.allSatisfy { hay.contains($0) }
        }
    }

    var body: some View {
        let list = items
        VStack(spacing: 0) {
            HStack(spacing: 8) {
                Image(systemName: "magnifyingglass").foregroundStyle(c.secondary)
                TextField("Type a command, session or tab", text: $query)
                    .textFieldStyle(.plain)
                    .font(.system(size: 15))
                    .focused($focused)
                    .onSubmit { run(list) }
            }
            .padding(.horizontal, 14).padding(.vertical, 12)
            Divider()
            ScrollViewReader { proxy in
                ScrollView {
                    LazyVStack(spacing: 0) {
                        ForEach(Array(list.enumerated()), id: \.element.id) { i, item in
                            row(item, selected: i == selected)
                                .id(i)
                                .onTapGesture { selected = i; run(list) }
                        }
                        if list.isEmpty {
                            Text("No matches").foregroundStyle(c.tertiary).padding(20)
                        }
                    }
                    .padding(6)
                }
                .frame(maxHeight: 340)
                .onChange(of: selected) { _, i in proxy.scrollTo(i) }
            }
        }
        .frame(width: 540)
        .fixedSize(horizontal: false, vertical: true)
        .background(.regularMaterial, in: RoundedRectangle(cornerRadius: 12, style: .continuous))
        .overlay(RoundedRectangle(cornerRadius: 12, style: .continuous).strokeBorder(c.glassStroke))
        .shadow(color: .black.opacity(0.35), radius: 30, y: 12)
        .onAppear { focused = true }
        .onChange(of: query) { _, _ in selected = 0 }
        .onKeyPress(.downArrow) { selected = min(selected + 1, max(0, list.count - 1)); return .handled }
        .onKeyPress(.upArrow) { selected = max(selected - 1, 0); return .handled }
        .onKeyPress(.escape) { ws.paletteShown = false; return .handled }
        .task {
            guard let srv = ws.serverId else { return }
            serverActions = (try? await model.core.paletteActions(serverId: srv)) ?? []
        }
    }

    private func row(_ item: Item, selected: Bool) -> some View {
        HStack(spacing: 10) {
            Image(systemName: item.symbol).frame(width: 18).foregroundStyle(selected ? .white : c.secondary)
            Text(item.title).foregroundStyle(selected ? .white : c.text).lineLimit(1)
            Spacer()
            Text(item.subtitle).font(.system(size: 11)).foregroundStyle(selected ? .white.opacity(0.75) : c.tertiary).lineLimit(1)
            if !item.shortcut.isEmpty {
                Text(item.shortcut).font(.system(size: 11)).foregroundStyle(selected ? .white.opacity(0.85) : c.secondary)
                    .frame(minWidth: 34, alignment: .trailing)
            }
        }
        .font(.system(size: 13))
        .padding(.horizontal, 10).padding(.vertical, 7)
        .background(selected ? Color.accentColor : .clear, in: RoundedRectangle(cornerRadius: 7, style: .continuous))
        .contentShape(Rectangle())
    }

    private func run(_ list: [Item]) {
        guard list.indices.contains(selected) else { return }
        let item = list[selected]
        ws.paletteShown = false
        item.run()
    }
}

/// ⌘F: searches the focused terminal's screen and history.
struct FindBar: View {
    let ws: Workspace
    @Environment(\.chrome) private var c
    @State private var query = ""
    @State private var notFound = false
    @FocusState private var focused: Bool

    var body: some View {
        HStack(spacing: 6) {
            Image(systemName: "magnifyingglass").foregroundStyle(c.secondary)
            TextField("Find", text: $query)
                .textFieldStyle(.plain)
                .frame(width: 180)
                .focused($focused)
                .onSubmit { find(older: !NSEvent.modifierFlags.contains(.shift)) }
            if notFound { Text("Not found").font(.system(size: 11)).foregroundStyle(Palette.error) }
            Button { find(older: true) } label: { Image(systemName: "chevron.up") }
                .help("Find previous (older)").keyboardShortcut("g", modifiers: [.command, .shift])
            Button { find(older: false) } label: { Image(systemName: "chevron.down") }
                .help("Find next (newer)").keyboardShortcut("g")
            Button("Done") { close() }
        }
        .buttonStyle(.borderless)
        .font(.system(size: 12))
        .padding(.horizontal, 10).padding(.vertical, 6)
        .background(.regularMaterial, in: RoundedRectangle(cornerRadius: 9, style: .continuous))
        .overlay(RoundedRectangle(cornerRadius: 9, style: .continuous).strokeBorder(c.glassStroke))
        .shadow(color: .black.opacity(0.25), radius: 10, y: 4)
        .onAppear { focused = true }
        .onChange(of: query) { _, _ in notFound = false }
        .onKeyPress(.escape) { close(); return .handled }
    }

    private func find(older: Bool) {
        guard !query.isEmpty, let t = ws.focusedTerminal else { return }
        notFound = !t.find(query, older: older)
    }

    private func close() {
        ws.findShown = false
        ws.focusedTerminal?.resetFind()
        if let t = ws.focusedTerminal { t.window?.makeFirstResponder(t) }
    }
}
