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
            .task(id: address) { await test() }
            .onAppear { field = .address }
        }
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
