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

/// Navigation: home → terminal. iPad gets a sidebar.
struct RootView: View {
    @Environment(ShrewModel.self) private var model
    @State private var path: [Route] = []
    @Environment(\.horizontalSizeClass) private var hsize

    struct Route: Hashable {
        var server: String
        var session: String
        var block: String?
    }

    var body: some View {
        Group {
            if hsize == .regular {
                NavigationSplitView {
                    HomeView { s, ses, b in path = [Route(server: s, session: ses, block: b)] }
                } detail: {
                    if let r = path.last {
                        NavigationStack { TerminalScreen(serverId: r.server, sessionId: r.session, blockId: r.block).id(r) }
                    } else {
                        ContentUnavailableView("Pick a session", systemImage: "terminal", description: Text("Your sessions keep running on the server."))
                    }
                }
            } else {
                NavigationStack(path: $path) {
                    HomeView { s, ses, b in path.append(Route(server: s, session: ses, block: b)) }
                        .navigationDestination(for: Route.self) { r in
                            TerminalScreen(serverId: r.server, sessionId: r.session, blockId: r.block)
                        }
                }
            }
        }
        .onChange(of: model.pendingOpen) { _, f in
            guard let f else { return }
            model.pendingOpen = nil
            path = [Route(server: f.serverId, session: f.sessionId, block: f.blockId)]
        }
        .onChange(of: model.state.servers.count) { _, n in
            if n > 0 { Notifier.requestAuthorizationIfNeeded() }
        }
        // Test/automation hook: SHREW_OPEN=<session label> opens it once it loads.
        .task {
            guard let want = ProcessInfo.processInfo.environment["SHREW_OPEN"] else { return }
            for _ in 0..<100 {
                if let url = URL(string: "shrew://open?session=\(want)"), open(url) { return }
                try? await Task.sleep(for: .milliseconds(100))
            }
        }
        // shrew://open?session=<label or id>[&server=<name or id>]
        .onOpenURL { url in _ = open(url) }
    }

    @discardableResult
    private func open(_ url: URL) -> Bool {
        guard url.scheme == "shrew", url.host == "open",
              let q = URLComponents(url: url, resolvingAgainstBaseURL: false)?.queryItems else { return false }
        let want = q.first { $0.name == "session" }?.value
        let srvWant = q.first { $0.name == "server" }?.value
        for srv in model.state.servers where srvWant == nil || srv.id == srvWant || srv.name == srvWant {
            if let s = srv.sessions.first(where: { $0.id == want || $0.label == want }) {
                path = [Route(server: srv.id, session: s.id, block: nil)]
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
