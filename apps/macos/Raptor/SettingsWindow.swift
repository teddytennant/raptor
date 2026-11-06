import RaptorKit
import SwiftUI

struct SettingsWindow: View {
    var body: some View {
        TabView {
            AppearanceSettings().frame(width: 560, height: 680).tabItem { Label("Appearance", systemImage: "paintpalette") }
            GeneralSettings().frame(width: 560, height: 400).tabItem { Label("General", systemImage: "gearshape") }
            ServerSettings().frame(width: 560).tabItem { Label("Servers", systemImage: "server.rack") }
        }
    }
}

private struct AppearanceSettings: View {
    @Environment(AppModel.self) private var app
    @Environment(RaptorModel.self) private var model

    var body: some View {
        let p = app.prefs
        Form {
            Section {
                LazyVGrid(columns: Array(repeating: GridItem(.flexible(), spacing: 12), count: 4), spacing: 12) {
                    ForEach(Themes.all) { t in
                        ThemeCard(theme: t, selected: t.id == model.themeId) { app.setTheme(t.id) }
                    }
                }
                .padding(.vertical, 4)
            } header: {
                Text("Theme")
            }
            Section("Text") {
                Picker("Font", selection: Binding(get: { p.fontId }, set: { v in app.update { $0.fontId = v } })) {
                    ForEach(Fonts.all) { f in Text(f.name).font(.custom(f.faces[0], size: 13)).tag(f.id) }
                }
                LabeledContent("Size") {
                    Stepper(value: Binding(get: { Double(p.fontSize) }, set: { v in app.setFontSize(CGFloat(v)) }), in: 8...32, step: 1) {
                        Text("\(Int(p.fontSize)) pt").monospacedDigit()
                    }
                }
                LabeledContent("Line spacing") {
                    HStack {
                        Slider(value: Binding(get: { Double(p.lineHeight) }, set: { v in app.update { $0.lineHeight = CGFloat((v * 20).rounded() / 20) } }), in: 1.0...1.6)
                        Text(String(format: "%.2f", Double(p.lineHeight))).monospacedDigit().frame(width: 36, alignment: .trailing)
                    }
                }
                Sample(prefs: p, theme: model.theme)
                    .frame(height: 96)
                    .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
            }
            Section("Cursor") {
                Picker("Shape", selection: Binding(get: { p.cursorShape.map { "\($0)" } ?? "program" }, set: { v in
                    app.update { $0.cursorShape = switch v { case "block": .block; case "bar": .bar; case "underline": .underline; default: nil } }
                })) {
                    Text("As the program asks").tag("program")
                    Text("Block").tag("block")
                    Text("Bar").tag("bar")
                    Text("Underline").tag("underline")
                }
                Toggle("Blink", isOn: Binding(get: { p.blink }, set: { v in app.update { $0.blink = v } }))
            }
        }
        .formStyle(.grouped)
    }
}

private struct ThemeCard: View {
    let theme: ThemeSpec
    let selected: Bool
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            VStack(alignment: .leading, spacing: 6) {
                VStack(alignment: .leading, spacing: 3) {
                    HStack(spacing: 3) {
                        ForEach(0..<6) { i in
                            Circle().fill(Color(hex: (theme.ansi ?? Themes.all[1].ansi!)[i + 1])).frame(width: 6, height: 6)
                        }
                    }
                    Text("~ $ ls").font(.custom("JetBrainsMono-Regular", size: 10)).foregroundStyle(Color(hex: theme.foreground))
                }
                .padding(8)
                .frame(maxWidth: .infinity, minHeight: 46, alignment: .topLeading)
                .background(Color(hex: theme.background), in: RoundedRectangle(cornerRadius: 7, style: .continuous))
                .overlay(RoundedRectangle(cornerRadius: 7, style: .continuous).strokeBorder(selected ? Color.accentColor : Color.primary.opacity(0.12), lineWidth: selected ? 2 : 1))
                Text(theme.name).font(.system(size: 11)).foregroundStyle(selected ? .primary : .secondary).lineLimit(1)
            }
        }
        .buttonStyle(.plain)
        .accessibilityAddTraits(selected ? .isSelected : [])
    }
}

/// A few lines drawn by the real terminal renderer, so the preview matches the terminal exactly.
private struct Sample: NSViewRepresentable {
    let prefs: TerminalPrefs
    let theme: ThemeSpec

    func makeNSView(context: Context) -> SampleView { SampleView() }
    func updateNSView(_ v: SampleView, context: Context) {
        v.prefs = prefs
        v.theme = theme
        v.needsDisplay = true
    }

    final class SampleView: NSView {
        var prefs = TerminalPrefs()
        var theme = Themes.all[0]
        override var isFlipped: Bool { true }

        override func draw(_ dirtyRect: NSRect) {
            guard let ctx = NSGraphicsContext.current?.cgContext else { return }
            let r = TerminalRenderer(fontSize: prefs.fontSize)
            r.setAppearance(fontId: prefs.fontId, lineHeight: prefs.lineHeight)
            let ansi = theme.ansi ?? Themes.all[1].ansi!
            let fg = 0xFF00_0000 | theme.foreground
            func run(_ col: Int, _ text: String, _ color: UInt32, bold: Bool = false) -> Run {
                let widths = text.unicodeScalars.map { UInt8($0.value >= 0x2E80 ? 2 : 1) }
                return Run(col: UInt16(col), cells: UInt16(widths.reduce(0) { $0 + UInt16($1) }), text: text, widths: Data(widths),
                           fg: 0xFF00_0000 | color, bg: nil, underlineColor: nil, style: bold ? 1 : 0)
            }
            let lines = [
                Line(runs: [run(0, "~/src/raptor", ansi[4], bold: true), run(13, "main", ansi[5]), run(18, "$ ", fg), run(20, "cargo test", fg)]),
                Line(runs: [run(0, "   Compiling", ansi[2], bold: true), run(12, " raptor-core v0.1.0", fg)]),
                Line(runs: [run(0, "test result: ", fg), run(13, "ok", ansi[2]), run(15, ". 42 passed; ", fg), run(28, "0 failed", ansi[1])]),
                Line(runs: [run(0, "╭─ box ─╮ ", ansi[6]), run(10, "-> => != ", ansi[3]), run(19, "日本語", fg)]),
            ]
            let frame = Frame(cols: 60, rows: 4, background: 0xFF00_0000 | theme.background, foreground: fg, cursorColor: 0xFF00_0000 | theme.cursor,
                              cursor: Cursor(col: 30, row: 0, shape: prefs.cursorShape ?? .block, blinking: false, password: false, wide: false),
                              lines: lines, scrollTotal: 4, scrollOffset: 0, scrollLen: 4, alternateScreen: false, dark: true)
            var o = TerminalRenderer.Options()
            o.padding = 10
            o.cursorShape = prefs.cursorShape
            r.draw(frame, in: ctx, size: bounds.size, options: o)
        }
    }
}

private struct GeneralSettings: View {
    @Environment(AppModel.self) private var app
    @Environment(RaptorModel.self) private var model
    @State private var tick = 0

    var body: some View {
        let p = app.prefs
        Form {
            Section("Keyboard") {
                Picker("Use Option as Meta", selection: Binding(get: { p.optionAsMeta }, set: { v in app.update { $0.optionAsMeta = v } })) {
                    Text("Off (type special characters)").tag(TerminalPrefs.OptionKey.off)
                    Text("Left Option").tag(TerminalPrefs.OptionKey.left)
                    Text("Right Option").tag(TerminalPrefs.OptionKey.right)
                    Text("Both").tag(TerminalPrefs.OptionKey.both)
                }
                Toggle("Ask before closing a pane or tab with a running program", isOn: Binding(get: { app.confirmClose }, set: { app.setConfirmClose($0) }))
            }
            Section("Notifications") {
                flag("An agent needs you", Prefs.notifyBlocked)
                flag("An agent finishes", Prefs.notifyDone)
                flag("A program sends a notification", Prefs.notifyProgram)
                flag("A program exits", Prefs.notifyExit, default: false)
            }
        }
        .formStyle(.grouped)
        .id(tick)
    }

    private func flag(_ title: String, _ key: String, default d: Bool = true) -> some View {
        Toggle(title, isOn: Binding(get: { Prefs.flag(model, key, default: d) }, set: { v in
            model.core.setSetting(key: key, value: v ? "true" : "false")
            tick += 1
        }))
    }
}

private struct ServerSettings: View {
    @Environment(RaptorModel.self) private var model
    @State private var selection: String?
    @State private var adding = false
    @State private var editing: Server?

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            List(selection: $selection) {
                ForEach(model.state.servers, id: \.id) { s in
                    HStack(spacing: 10) {
                        Circle().fill(Palette.connectivity(s.connectivity)).frame(width: 8, height: 8)
                        VStack(alignment: .leading, spacing: 1) {
                            Text(serverTitle(s))
                            Text(s.address).font(.system(size: 11)).foregroundStyle(.secondary)
                        }
                        Spacer()
                        Text(s.connectivity.label(latency: s.latencyMs)).font(.system(size: 11)).foregroundStyle(.secondary).lineLimit(1)
                    }
                    .padding(.vertical, 3)
                    .tag(s.id)
                    .contextMenu {
                        Button("Edit…") { editing = s }
                        Button("Reconnect") { model.core.reconnect(serverId: s.id) }
                        Button("Remove", role: .destructive) { model.core.removeServer(serverId: s.id) }
                    }
                }
                .onMove { from, to in
                    if let f = from.first { model.core.moveServer(from: UInt32(f), to: UInt32(to > f ? to - 1 : to)) }
                }
            }
            .listStyle(.bordered(alternatesRowBackgrounds: true))
            .frame(height: 220)
            HStack(spacing: 0) {
                Button { adding = true } label: { Image(systemName: "plus").frame(width: 22, height: 18) }
                Button {
                    if let id = selection { model.core.removeServer(serverId: id) }
                } label: { Image(systemName: "minus").frame(width: 22, height: 18) }
                .disabled(selection == nil)
                Spacer()
                Button("Edit…") { editing = model.state.servers.first { $0.id == selection } }.disabled(selection == nil)
            }
            .buttonStyle(.borderless)
            .padding(.top, 6)
        }
        .padding(20)
        .sheet(isPresented: $adding) { ServerSheet(editing: nil) }
        .sheet(item: $editing) { ServerSheet(editing: $0) }
    }
}

/// Add or edit a server; checks the address before saving.
struct ServerSheet: View {
    let editing: Server?
    @Environment(RaptorModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    @State private var name = ""
    @State private var address = ""
    @State private var status: String?
    @State private var failed = false
    @State private var checking = false
    @FocusState private var addressFocused: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text(editing == nil ? "Add Server" : "Edit Server").font(.headline)
            Form {
                TextField("Address", text: $address, prompt: Text("studio.tailnet.ts.net:7979"))
                    .focused($addressFocused)
                TextField("Name", text: $name, prompt: Text("Optional"))
            }
            .formStyle(.columns)
            Text("A host and port, an http:// or https:// URL, or unix:///path/to/rex.sock.")
                .font(.system(size: 11)).foregroundStyle(.secondary)
            if let status {
                Label(status, systemImage: failed ? "exclamationmark.triangle.fill" : "checkmark.circle.fill")
                    .font(.system(size: 12))
                    .foregroundStyle(failed ? Palette.error : Palette.done)
            }
            HStack {
                if checking { ProgressView().controlSize(.small) }
                Button("Check") { Task { await check() } }.disabled(address.isEmpty || checking)
                Spacer()
                Button("Cancel", role: .cancel) { dismiss() }.keyboardShortcut(.cancelAction)
                Button(editing == nil ? "Add" : "Save") { Task { await save() } }
                    .keyboardShortcut(.defaultAction)
                    .disabled(address.trimmingCharacters(in: .whitespaces).isEmpty || checking)
            }
        }
        .padding(20)
        .frame(width: 440)
        .onAppear {
            if let e = editing {
                name = e.name
                address = e.address
            }
            addressFocused = true
        }
    }

    @discardableResult
    private func check() async -> Bool {
        checking = true
        defer { checking = false }
        do {
            let v = try await model.core.probe(address: address.trimmingCharacters(in: .whitespaces))
            status = "Rex \(v)"
            failed = false
            return true
        } catch {
            status = describe(error)
            failed = true
            return false
        }
    }

    private func save() async {
        let addr = address.trimmingCharacters(in: .whitespaces)
        guard await check() else { return }
        do {
            if let e = editing {
                try model.core.updateServer(serverId: e.id, name: name, address: addr)
            } else {
                _ = try model.core.addServer(name: name, address: addr)
            }
            dismiss()
        } catch {
            status = describe(error)
            failed = true
        }
    }
}
