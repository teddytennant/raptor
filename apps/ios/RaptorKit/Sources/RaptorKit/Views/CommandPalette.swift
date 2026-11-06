import SwiftUI

/// One row in the command palette.
public struct PaletteItem: Identifiable {
    public let id: String
    public let title: String
    public let subtitle: String
    public let category: String
    public let keywords: String
    public let symbol: String
    public let run: () -> Void
}

/// A searchable list of everything you can do here: Raptor's own commands plus
/// the server's actions (from its Lua config and block creators).
struct CommandPalette: View {
    @Environment(RaptorModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    @Environment(\.chrome) private var c
    let serverId: String
    let local: [PaletteItem]
    let sessionId: String
    let blockId: String?
    @State private var query = ""
    @State private var server: [PaletteAction] = []
    @State private var loading = true
    @FocusState private var focused: Bool

    var body: some View {
        let items = all.filter(matches)
        NavigationStack {
            List {
                ForEach(groups(items), id: \.0) { category, rows in
                    Section(category) {
                        ForEach(rows) { item in
                            Button {
                                Haptics.tap()
                                dismiss()
                                item.run()
                            } label: {
                                HStack(spacing: 12) {
                                    Image(systemName: item.symbol).frame(width: 22).foregroundStyle(c.accent)
                                    VStack(alignment: .leading, spacing: 2) {
                                        Text(item.title).foregroundStyle(c.text)
                                        if !item.subtitle.isEmpty {
                                            Text(item.subtitle).font(.caption).foregroundStyle(c.secondary).lineLimit(1)
                                        }
                                    }
                                }
                            }
                            .listRowBackground(c.surface)
                        }
                    }
                }
                if loading {
                    HStack { ProgressView(); Text("Loading server actions…").foregroundStyle(c.secondary) }.listRowBackground(c.surface)
                }
            }
            .scrollContentBackground(.hidden)
            .background(c.bg)
            .navigationTitle("Commands")
            #if os(iOS)
            .searchable(text: $query, placement: .navigationBarDrawer(displayMode: .always), prompt: "Run a command")
            .navigationBarTitleDisplayMode(.inline)
            #else
            .searchable(text: $query, prompt: "Run a command")
            #endif
            .toolbar { ToolbarItem(placement: .cancellationAction) { Button("Close") { dismiss() } } }
        }
        .presentationDetents([.medium, .large])
        .preferredColorScheme(c.scheme)
        .tint(c.accent)
        .task {
            server = (try? await model.core.paletteActions(serverId: serverId)) ?? []
            loading = false
        }
    }

    private var all: [PaletteItem] {
        local + server.map { a in
            PaletteItem(
                id: a.name, title: a.title, subtitle: a.description, category: a.category.isEmpty ? "Actions" : a.category,
                keywords: a.keywords + " " + a.name, symbol: a.name.contains(".") ? "square.stack.3d.up" : "curlybraces"
            ) { [model, serverId, sessionId, blockId] in
                Task { try? await model.core.runAction(serverId: serverId, sessionId: sessionId, blockId: blockId, action: a.name) }
            }
        }
    }

    private func matches(_ i: PaletteItem) -> Bool {
        let q = query.trimmingCharacters(in: .whitespaces).lowercased()
        guard !q.isEmpty else { return true }
        let hay = "\(i.title) \(i.subtitle) \(i.category) \(i.keywords)".lowercased()
        return q.split(separator: " ").allSatisfy { hay.contains($0) }
    }

    private func groups(_ items: [PaletteItem]) -> [(String, [PaletteItem])] {
        var order: [String] = []
        var map: [String: [PaletteItem]] = [:]
        for i in items {
            if map[i.category] == nil { order.append(i.category) }
            map[i.category, default: []].append(i)
        }
        return order.map { ($0, map[$0]!) }
    }
}
