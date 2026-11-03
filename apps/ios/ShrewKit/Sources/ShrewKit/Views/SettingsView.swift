import SwiftUI

/// Boolean settings stored by the core, shared with the Android app's keys.
public enum Prefs {
    public static let fitOnOpen = "fit_on_open"
    public static let keyBar = "key_bar"
    public static let notifyBlocked = "notify_blocked"
    public static let notifyDone = "notify_done"
    public static let notifyExit = "notify_exit"
    public static let notifyProgram = "notify_program"

    public static func flag(_ model: ShrewModel, _ key: String, default d: Bool = true) -> Bool {
        model.core.setting(key: key).map { $0 == "true" } ?? d
    }

    static func set(_ model: ShrewModel, _ key: String, _ on: Bool) {
        model.core.setSetting(key: key, value: on ? "true" : "false")
    }
}

/// Settings: appearance, terminal behaviour, notifications, servers, about.
public struct SettingsView: View {
    @Environment(ShrewModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    var onAddServer: () -> Void
    var onEditServer: (Server) -> Void
    @State private var fontSize: Double = 13

    public init(onAddServer: @escaping () -> Void, onEditServer: @escaping (Server) -> Void) {
        self.onAddServer = onAddServer
        self.onEditServer = onEditServer
    }

    public var body: some View {
        let c = Chrome(model.theme)
        NavigationStack {
            Form {
                Section("Appearance") {
                    sample(model.theme)
                        .listRowInsets(EdgeInsets(top: 12, leading: 12, bottom: 12, trailing: 12))
                    ScrollView(.horizontal, showsIndicators: false) {
                        HStack(spacing: 14) {
                            ForEach(Themes.all) { t in
                                let on = t.id == model.themeId
                                Button { model.setTheme(t.id); Haptics.select() } label: {
                                    VStack(spacing: 6) {
                                        Circle().fill(Color(hex: t.background))
                                            .overlay(Circle().fill(Color(hex: t.accent)).frame(width: 12, height: 12))
                                            .overlay(Circle().strokeBorder(on ? c.accent : c.hairline, lineWidth: on ? 2.5 : 1))
                                            .frame(width: 36, height: 36)
                                        Text(shortName(t)).font(.caption2).foregroundStyle(on ? c.text : c.secondary)
                                    }
                                }
                                .buttonStyle(.plain)
                            }
                        }
                        .padding(.vertical, 4)
                    }
                    HStack {
                        Text("Text size")
                        Slider(value: $fontSize, in: 9...22, step: 1) { editing in
                            if !editing { model.core.setSetting(key: "font_size", value: String(fontSize)) }
                        }
                        Text("\(Int(fontSize))").font(.mono(14)).foregroundStyle(c.secondary).frame(width: 28)
                    }
                }
                Section {
                    toggle("Fit to this screen when opening", Prefs.fitOnOpen)
                    toggle("Key bar above the keyboard", Prefs.keyBar)
                } header: {
                    Text("Terminal")
                } footer: {
                    Text("Fitting resizes the session to your phone while you use it, so text is readable and the keyboard never covers the prompt. Desktop clients see the phone's size until you leave.")
                }
                Section("Notifications") {
                    toggle("An agent needs you", Prefs.notifyBlocked)
                    toggle("An agent finishes", Prefs.notifyDone)
                    toggle("A program exits", Prefs.notifyExit, default: false)
                    toggle("Programs send a notification", Prefs.notifyProgram)
                }
                Section("Servers") {
                    ForEach(model.state.servers, id: \.id) { srv in
                        Button { dismiss(); onEditServer(srv) } label: {
                            HStack(spacing: 12) {
                                Circle().fill(Palette.connectivity(srv.connectivity)).frame(width: 8, height: 8)
                                VStack(alignment: .leading, spacing: 2) {
                                    Text(srv.name).foregroundStyle(c.text)
                                    Text([srv.address, srv.connectivity.isOnline ? srv.version.map { "Rex \($0)" } : nil].compactMap { $0 }.joined(separator: " · "))
                                        .font(.mono(12)).foregroundStyle(c.secondary).lineLimit(1)
                                }
                                Spacer()
                                Image(systemName: "chevron.right").font(.footnote.weight(.semibold)).foregroundStyle(c.tertiary)
                            }
                        }
                    }
                    Button("Add a Server", systemImage: "plus") { dismiss(); onAddServer() }
                }
                Section("About") {
                    LabeledContent("Version", value: Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "")
                    Link(destination: URL(string: "https://github.com/teddytennant/shrew")!) {
                        LabeledContent("Source code", value: "MIT")
                    }
                }
            }
            .scrollContentBackground(.hidden)
            .background(c.bg)
            .navigationTitle("Settings")
            #if os(iOS)
            .navigationBarTitleDisplayMode(.inline)
            #endif
            .toolbar { ToolbarItem(placement: .confirmationAction) { Button("Done") { dismiss() } } }
        }
        .tint(c.accent)
        .preferredColorScheme(c.scheme)
        .onAppear { fontSize = min(max(Double(model.core.setting(key: "font_size") ?? "") ?? 13, 9), 22) }
    }

    private func shortName(_ t: ThemeSpec) -> String {
        switch t.id {
        case "tokyo-night": "Tokyo"
        case "mocha": "Mocha"
        case "solarized-light": "Solarized"
        case "server": "Server"
        default: t.name
        }
    }

    private func toggle(_ label: String, _ key: String, default d: Bool = true) -> some View {
        Toggle(label, isOn: Binding(get: { Prefs.flag(model, key, default: d) }, set: { Prefs.set(model, key, $0) }))
    }

    /// A few colored lines in the chosen theme and size.
    private func sample(_ t: ThemeSpec) -> some View {
        let fg = Color(hex: t.foreground)
        func ansi(_ i: Int) -> Color { t.ansi.map { Color(hex: $0[i]) } ?? fg }
        let line1 = Text("me@box").foregroundColor(ansi(2)) + Text(":").foregroundColor(fg) + Text("~/src").foregroundColor(ansi(4)) + Text("$ cargo test").foregroundColor(fg)
        let line2 = Text("test result: ").foregroundColor(fg) + Text("ok").foregroundColor(ansi(2)) + Text(". 11 passed; ").foregroundColor(fg) + Text("0 failed").foregroundColor(ansi(1))
        return VStack(alignment: .leading, spacing: 3) { line1; line2 }
            .font(.mono(fontSize))
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(14)
            .background(Color(hex: t.background), in: RoundedRectangle(cornerRadius: 10, style: .continuous))
    }
}
