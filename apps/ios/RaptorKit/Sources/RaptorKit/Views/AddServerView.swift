import SwiftUI

/// Add or edit a server. Tests the address as you type so mistakes show early.
public struct AddServerView: View {
    @Environment(RaptorModel.self) private var model
    var editing: Server?
    var onDone: () -> Void
    @State private var address: String
    @State private var name: String
    @State private var probe: Probe = .idle
    @State private var error: String?
    @FocusState private var field: Field?
    @Environment(\.scenePhase) private var phase
    @State private var retest = 0

    enum Field { case address, name }
    enum Probe: Equatable {
        case idle, testing
        case ok(String)
        case failed(String)
    }

    public init(editing: Server?, onDone: @escaping () -> Void) {
        self.editing = editing
        self.onDone = onDone
        _address = State(initialValue: editing?.address ?? "")
        _name = State(initialValue: editing?.name ?? "")
    }

    public var body: some View {
        let c = Chrome(model.theme)
        NavigationStack {
            Form {
                Section {
                    TextField("my-box.tail1234.ts.net", text: $address)
                        .font(.mono(16))
                        .focused($field, equals: .address)
                        .autocorrectionDisabled()
                        #if os(iOS)
                        .textInputAutocapitalization(.never)
                        .keyboardType(.URL)
                        #endif
                        .submitLabel(.next)
                        .onSubmit { field = .name }
                    TextField("Name (optional)", text: $name)
                        .focused($field, equals: .name)
                        .submitLabel(.done)
                        .onSubmit(save)
                } header: {
                    Text("Address")
                } footer: {
                    Text("Anything you'd pass to `rex -S`: a tailnet name, an IP with port like 100.64.0.2:7878, or a hostname.")
                }
                Section {
                    probeRow
                }
                if let error {
                    Section { Text(error).foregroundStyle(Palette.error) }
                }
            }
            .scrollContentBackground(.hidden)
            .background(c.bg)
            .navigationTitle(editing == nil ? "Add Server" : "Edit Server")
            #if os(iOS)
            .navigationBarTitleDisplayMode(.inline)
            #endif
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Cancel", action: onDone) }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Save", action: save).bold().disabled(address.trimmingCharacters(in: .whitespaces).isEmpty)
                }
            }
            .task(id: "\(address)#\(retest)") { await test() }
            .onAppear { field = .address }
            // Coming back from the Local Network prompt or Settings: try again.
            .onChange(of: phase) { _, p in if p == .active, case .failed = probe { retest += 1 } }
        }
        .tint(c.accent)
        .preferredColorScheme(c.scheme)
        .presentationDetents([.medium, .large])
    }

    @ViewBuilder private var probeRow: some View {
        switch probe {
        case .idle:
            Label("We'll test the connection as you type.", systemImage: "antenna.radiowaves.left.and.right").foregroundStyle(.secondary)
        case .testing:
            HStack(spacing: 10) { ProgressView(); Text("Connecting…").foregroundStyle(.secondary) }
        case .ok(let v):
            Label("Connected · Rex \(v)", systemImage: "checkmark.circle.fill").foregroundStyle(Palette.accent)
        case .failed(let m):
            Label(m, systemImage: "exclamationmark.triangle.fill").foregroundStyle(Palette.error)
            if localNetworkBlocked(address: address, error: m) { LocalNetworkHelp() }
        }
    }

    private func test() async {
        let a = address.trimmingCharacters(in: .whitespaces)
        guard !a.isEmpty else { probe = .idle; return }
        try? await Task.sleep(for: .milliseconds(550))
        if Task.isCancelled { return }
        probe = .testing
        do {
            let v = try await model.core.probe(address: a)
            if !Task.isCancelled { probe = .ok(v) }
        } catch {
            if !Task.isCancelled { probe = .failed(describe(error)) }
        }
    }

    private func save() {
        let a = address.trimmingCharacters(in: .whitespaces)
        guard !a.isEmpty else { return }
        do {
            if let e = editing {
                try model.core.updateServer(serverId: e.id, name: name.isEmpty ? e.name : name, address: a)
            } else {
                _ = try model.core.addServer(name: name, address: a)
            }
            Haptics.tap()
            onDone()
        } catch {
            self.error = describe(error)
        }
    }
}

func describe(_ error: Error) -> String {
    if let e = error as? RaptorError {
        switch e {
        case .UnknownServer: return "Unknown server"
        case .NotConnected: return "Not connected"
        case .InvalidAddress(let d), .Server(let d): return d
        }
    }
    return error.localizedDescription
}

/// iOS blocks connections to the local network until the user allows it, and a
/// refusal looks like an unreachable host. Say what to do instead.
func localNetworkBlocked(address: String, error: String) -> Bool {
    #if os(iOS)
    let unreachable = ["No route to host", "os error 65", "Network is unreachable", "os error 51"].contains { error.contains($0) }
    return unreachable && isLocalAddress(address)
    #else
    return false
    #endif
}

func isLocalAddress(_ address: String) -> Bool {
    var host = address.lowercased()
    if let r = host.range(of: "://") { host = String(host[r.upperBound...]) }
    if let at = host.lastIndex(of: "@") { host = String(host[host.index(after: at)...]) }
    host = String(host.prefix { $0 != ":" && $0 != "/" })
    if host.hasSuffix(".local") || !host.contains(".") { return true }
    let o = host.split(separator: ".").compactMap { Int($0) }
    guard o.count == 4 else { return false }
    return o[0] == 10 || (o[0] == 192 && o[1] == 168) || (o[0] == 172 && (16...31).contains(o[1])) || (o[0] == 169 && o[1] == 254)
}

struct LocalNetworkHelp: View {
    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("Raptor needs Local Network access to reach this address. Allow it in Settings.")
                .font(.footnote)
                .foregroundStyle(.secondary)
            #if os(iOS)
            Button("Open Settings") {
                if let url = URL(string: UIApplication.openSettingsURLString) { UIApplication.shared.open(url) }
            }
            .font(.footnote.weight(.semibold))
            #endif
        }
    }
}
