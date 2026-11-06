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
                ToolbarItem(placement: .leading) {
                    Button { dismiss(); onSettings() } label: { Image(systemName: "gearshape") }.accessibilityLabel("Settings")
                }
                ToolbarItem(placement: .trailing) {
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
            logo
                .clipShape(RoundedRectangle(cornerRadius: 16, style: .continuous))
                .overlay(RoundedRectangle(cornerRadius: 16, style: .continuous).strokeBorder(c.glassStroke, lineWidth: 1))
                .accessibilityHidden(true)
            Text("Raptor").font(.largeTitle.weight(.bold)).foregroundStyle(c.text).padding(.top, 20)
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
        .frame(maxWidth: 480)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(c.bg.ignoresSafeArea())
        .preferredColorScheme(c.scheme)
    }
}

/// The app icon, for the welcome screen.
private var logo: Image {
    #if canImport(UIKit)
    UIImage(named: "Logo", in: .module, compatibleWith: nil).map(Image.init(uiImage:)) ?? Image(systemName: "apple.terminal")
    #else
    Bundle.module.image(forResource: "Logo").map(Image.init(nsImage:)) ?? Image(systemName: "apple.terminal")
    #endif
}

/// Nothing open: point at the switcher, or at the sidebar when there's no switcher.
public struct PickSessionView: View {
    @Environment(RaptorModel.self) private var model
    var onSessions: (() -> Void)?
    public init(onSessions: (() -> Void)?) { self.onSessions = onSessions }

    public var body: some View {
        let c = Chrome(model.theme)
        ContentUnavailableView {
            Label("Pick a session", systemImage: "terminal")
        } description: {
            Text(onSessions == nil ? "Choose one from the list. Sessions keep running on the server." : "Your sessions keep running on the server.")
        } actions: {
            if let onSessions { Button("Show Sessions", action: onSessions).font(.headline) }
        }
        .tint(c.accent)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(c.bg.ignoresSafeArea())
        .preferredColorScheme(c.scheme)
    }
}

private extension ToolbarItemPlacement {
    #if os(iOS)
    static let leading = ToolbarItemPlacement.topBarLeading
    static let trailing = ToolbarItemPlacement.topBarTrailing
    #else
    static let leading = ToolbarItemPlacement.navigation
    static let trailing = ToolbarItemPlacement.primaryAction
    #endif
}
