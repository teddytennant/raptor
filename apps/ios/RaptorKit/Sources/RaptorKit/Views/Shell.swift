import SwiftUI

/// The session switcher, pulled up from the terminal's title: everything you
/// can jump to, with settings and adding a server in its header.
public struct SessionSheet: View {
    @Environment(RaptorModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    var current: Focus?
    var onOpen: (Focus) -> Void
    var onAddServer: () -> Void
    var onEditServer: (Server) -> Void
    var onSettings: () -> Void

    public init(current: Focus?, onOpen: @escaping (Focus) -> Void, onAddServer: @escaping () -> Void,
                onEditServer: @escaping (Server) -> Void, onSettings: @escaping () -> Void) {
        self.current = current
        self.onOpen = onOpen
        self.onAddServer = onAddServer
        self.onEditServer = onEditServer
        self.onSettings = onSettings
    }

    public var body: some View {
        let c = Chrome(model.theme)
        NavigationStack {
            ScrollView {
                HomeList(current: current,
                         onOpen: { s, ses, b in dismiss(); onOpen(Focus(serverId: s, sessionId: ses, blockId: b)) },
                         onAdd: { dismiss(); onAddServer() },
                         onEdit: { srv in dismiss(); onEditServer(srv) })
                    .padding(.bottom, 24)
            }
            .background(c.bg.ignoresSafeArea())
            .navigationTitle("Sessions")
            #if os(iOS)
            .navigationBarTitleDisplayMode(.inline)
            #endif
            .toolbar {
                ToolbarItem(placement: .topBarLeading) {
                    Button { dismiss(); onSettings() } label: { Image(systemName: "gearshape") }.accessibilityLabel("Settings")
                }
                ToolbarItem(placement: .topBarTrailing) {
                    Button { dismiss(); onAddServer() } label: { Image(systemName: "plus") }.accessibilityLabel("Add server")
                }
            }
        }
        .environment(\.chrome, c)
        .tint(c.accent)
        .preferredColorScheme(c.scheme)
        .presentationDetents([.medium, .large])
        .presentationDragIndicator(.visible)
    }
}

/// First launch: nothing to open yet.
public struct WelcomeView: View {
    @Environment(RaptorModel.self) private var model
    var onAddServer: () -> Void
    public init(onAddServer: @escaping () -> Void) { self.onAddServer = onAddServer }

    public var body: some View {
        let c = Chrome(model.theme)
        VStack(alignment: .leading, spacing: 0) {
            Spacer()
            Image(systemName: "apple.terminal").font(.system(size: 36, weight: .medium)).foregroundStyle(c.accent)
            Text("Raptor").font(.system(size: 34, weight: .bold)).foregroundStyle(c.text).padding(.top, 20)
            Text("Your terminal sessions, from a Rex server on your Mac or any machine on your tailnet. They keep running when you close the app.")
                .font(.body).foregroundStyle(c.secondary).padding(.top, 10)
            Button(action: onAddServer) {
                Text("Add a server").font(.headline).frame(maxWidth: .infinity).frame(height: 52)
                    .foregroundStyle(Color(hex: 0x0B0D10))
                    .background(c.accent, in: RoundedRectangle(cornerRadius: 14, style: .continuous))
            }
            .padding(.top, 32)
            Spacer()
        }
        .padding(.horizontal, 28)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(c.bg.ignoresSafeArea())
        .preferredColorScheme(c.scheme)
    }
}

/// Nothing open: point at the switcher.
public struct PickSessionView: View {
    @Environment(RaptorModel.self) private var model
    var onSessions: () -> Void
    public init(onSessions: @escaping () -> Void) { self.onSessions = onSessions }

    public var body: some View {
        let c = Chrome(model.theme)
        ContentUnavailableView {
            Label("Pick a session", systemImage: "terminal")
        } description: {
            Text("Your sessions keep running on the server.")
        } actions: {
            Button("Show Sessions", action: onSessions).font(.headline)
        }
        .tint(c.accent)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(c.bg.ignoresSafeArea())
        .preferredColorScheme(c.scheme)
    }
}
