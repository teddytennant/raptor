import ShrewKit
import SwiftUI
import UserNotifications

@main
struct ShrewApp: App {
    @UIApplicationDelegateAdaptor(AppDelegate.self) private var delegate
    @State private var model: ShrewModel
    @Environment(\.scenePhase) private var phase

    init() {
        let dir = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0].appendingPathComponent("Shrew")
        let m = ShrewModel(dataDirectory: dir, platform: "ios", deviceLabel: UIDevice.current.name)
        m.onNotify = Notifier.post
        _model = State(initialValue: m)
        Notifier.model = m
    }

    var body: some Scene {
        WindowGroup {
            RootView()
                .environment(model)
                .tint(Palette.accent)
        }
        .onChange(of: phase) { _, p in
            model.setForeground(p != .background)
            // Only ask once there's something to be notified about.
            if p == .active, !model.state.servers.isEmpty { Notifier.requestAuthorizationIfNeeded() }
        }
    }
}

/// The app opens straight into your last session. Other sessions, servers and
/// settings are one tap away in the switcher. iPad keeps a sessions sidebar.
struct RootView: View {
    @Environment(ShrewModel.self) private var model
    @Environment(\.horizontalSizeClass) private var hsize
    @State private var current: Focus?
    @State private var switcher = false
    @State private var settings = false
    @State private var addingServer = false
    @State private var editing: Server?
    @State private var loaded = false

    var body: some View {
        Group {
            if hsize == .regular, !model.state.servers.isEmpty {
                NavigationSplitView {
                    ScrollView {
                        HomeList(current: current, onOpen: { s, ses, b in open(Focus(serverId: s, sessionId: ses, blockId: b)) },
                                 onAdd: { addingServer = true }, onEdit: { editing = $0 })
                    }
                    .background(Chrome(model.theme).bg.ignoresSafeArea())
                    .navigationTitle("Sessions")
                    .toolbar {
                        ToolbarItem(placement: .topBarLeading) { Button { settings = true } label: { Image(systemName: "gearshape") } }
                        ToolbarItem(placement: .topBarTrailing) { Button { addingServer = true } label: { Image(systemName: "plus") } }
                    }
                } detail: {
                    detail
                }
            } else {
                detail
            }
        }
        .sheet(isPresented: $switcher) {
            SessionSheet(current: current, onOpen: open, onAddServer: { addingServer = true }, onEditServer: { editing = $0 }, onSettings: { settings = true })
        }
        .sheet(isPresented: $settings) {
            SettingsView(onAddServer: { addingServer = true }, onEditServer: { editing = $0 })
        }
        .sheet(isPresented: $addingServer) { AddServerView(editing: nil) { addingServer = false } }
        .sheet(item: $editing) { srv in AddServerView(editing: srv) { editing = nil } }
        .onAppear {
            guard !loaded else { return }
            loaded = true
            current = lastOpen()
        }
        .onChange(of: model.state) { _, state in
            // A saved session that no longer exists is forgotten. Never open one the
            // user didn't pick: opening fits it to the phone, resizing it for others.
            guard let c = current, !state.servers.isEmpty else { return }
            guard let srv = state.servers.first(where: { $0.id == c.serverId }) else { forget(); return }
            if srv.connectivity.isOnline, !srv.sessions.contains(where: { $0.id == c.sessionId }) { forget() }
        }
        .onChange(of: model.pendingOpen) { _, f in
            guard let f else { return }
            model.pendingOpen = nil
            open(f)
        }
        .onChange(of: model.state.servers.count) { _, n in
            if n > 0 { Notifier.requestAuthorizationIfNeeded() }
        }
        // Test/automation hook: SHREW_OPEN=<session label> opens it once it loads.
        .task {
            guard let want = ProcessInfo.processInfo.environment["SHREW_OPEN"] else { return }
            for _ in 0..<100 {
                if let url = URL(string: "shrew://open?session=\(want)"), openURL(url) { return }
                try? await Task.sleep(for: .milliseconds(100))
            }
        }
        // shrew://open?session=<label or id>[&server=<name or id>]
        .onOpenURL { url in _ = openURL(url) }
    }

    @ViewBuilder private var detail: some View {
        if model.state.servers.isEmpty {
            WelcomeView { addingServer = true }
        } else if let c = current {
            TerminalScreen(serverId: c.serverId, sessionId: c.sessionId, blockId: c.blockId, onSessions: { switcher = true }, onOpen: open)
                .id("\(c.serverId)/\(c.sessionId)")
        } else {
            PickSessionView { switcher = true }
        }
    }

    private func open(_ f: Focus) {
        current = f
        model.core.setSetting(key: "last_open", value: "\(f.serverId)|\(f.sessionId)")
    }

    private func forget() {
        current = nil
        model.core.setSetting(key: "last_open", value: "")
        switcher = true
    }

    private func lastOpen() -> Focus? {
        let parts = (model.core.setting(key: "last_open") ?? "").split(separator: "|").map(String.init)
        return parts.count >= 2 ? Focus(serverId: parts[0], sessionId: parts[1], blockId: nil) : nil
    }

    @discardableResult
    private func openURL(_ url: URL) -> Bool {
        guard url.scheme == "shrew", url.host == "open",
              let q = URLComponents(url: url, resolvingAgainstBaseURL: false)?.queryItems else { return false }
        let want = q.first { $0.name == "session" }?.value
        let srvWant = q.first { $0.name == "server" }?.value
        for srv in model.state.servers where srvWant == nil || srv.id == srvWant || srv.name == srvWant {
            if let s = srv.sessions.first(where: { $0.id == want || $0.label == want }) {
                open(Focus(serverId: srv.id, sessionId: s.id, blockId: nil))
                return true
            }
        }
        return false
    }
}

final class AppDelegate: NSObject, UIApplicationDelegate, UNUserNotificationCenterDelegate {
    func application(_ application: UIApplication, didFinishLaunchingWithOptions launchOptions: [UIApplication.LaunchOptionsKey: Any]? = nil) -> Bool {
        UNUserNotificationCenter.current().delegate = self
        Notifier.registerCategories()
        return true
    }

    func userNotificationCenter(_ center: UNUserNotificationCenter, willPresent notification: UNNotification) async -> UNNotificationPresentationOptions {
        [.banner, .sound, .list]
    }

    func userNotificationCenter(_ center: UNUserNotificationCenter, didReceive response: UNNotificationResponse) async {
        let info = response.notification.request.content.userInfo
        guard let server = info["server"] as? String, let session = info["session"] as? String else { return }
        let block = info["block"] as? String
        if let r = response as? UNTextInputNotificationResponse, let block {
            // Reply straight from the notification, without opening the app's UI.
            try? await Notifier.model?.core.quickReply(serverId: server, sessionId: session, blockId: block, text: r.userText + "\r")
            return
        }
        await MainActor.run { Notifier.model?.pendingOpen = Focus(serverId: server, sessionId: session, blockId: block) }
    }
}

/// Local notifications for agents that need you or finish.
@MainActor
enum Notifier {
    static weak var model: ShrewModel?
    private static var asked = false

    static func registerCategories() {
        let reply = UNTextInputNotificationAction(identifier: "reply", title: "Reply", options: [], textInputButtonTitle: "Send", textInputPlaceholder: "Type a reply")
        let open = UNNotificationAction(identifier: "open", title: "Open", options: [.foreground])
        UNUserNotificationCenter.current().setNotificationCategories([
            UNNotificationCategory(identifier: "agent", actions: [reply, open], intentIdentifiers: []),
        ])
    }

    static func requestAuthorizationIfNeeded() {
        guard !asked, ProcessInfo.processInfo.environment["SHREW_NO_PROMPTS"] == nil else { return }
        asked = true
        UNUserNotificationCenter.current().requestAuthorization(options: [.alert, .sound, .badge]) { _, _ in }
    }

    static func post(_ n: ShrewKit.Notification) {
        if let m = model {
            let wanted: Bool
            switch n.kind {
            case .agentBlocked, .agentError: wanted = Prefs.flag(m, Prefs.notifyBlocked)
            case .agentDone: wanted = Prefs.flag(m, Prefs.notifyDone)
            case .processExited: wanted = Prefs.flag(m, Prefs.notifyExit, default: false)
            default: wanted = Prefs.flag(m, Prefs.notifyProgram)
            }
            guard wanted else { return }
        }
        let c = UNMutableNotificationContent()
        switch n.kind {
        case .agentBlocked: c.title = "\(n.title) needs you"; c.interruptionLevel = .timeSensitive; c.categoryIdentifier = "agent"
        case .agentDone: c.title = "\(n.title) finished"; c.categoryIdentifier = "agent"
        case .agentError: c.title = "\(n.title) hit an error"; c.categoryIdentifier = "agent"
        default: c.title = n.title
        }
        c.body = n.body
        c.sound = .default
        c.threadIdentifier = n.sessionId
        c.userInfo = ["server": n.serverId, "session": n.sessionId, "block": n.blockId]
        UNUserNotificationCenter.current().add(UNNotificationRequest(identifier: n.serverId + n.blockId, content: c, trigger: nil))
    }
}

/// Keep the edge-swipe back gesture when a screen hides the navigation bar
/// (the terminal is full-bleed).
extension UINavigationController: @retroactive UIGestureRecognizerDelegate {
    override open func viewDidLoad() {
        super.viewDidLoad()
        interactivePopGestureRecognizer?.delegate = self
    }

    public func gestureRecognizerShouldBegin(_ gestureRecognizer: UIGestureRecognizer) -> Bool {
        viewControllers.count > 1
    }
}
