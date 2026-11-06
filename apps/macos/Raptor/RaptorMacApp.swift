import AppKit
import RaptorKit
import SwiftUI
@preconcurrency import UserNotifications

@main
struct RaptorMacApp: App {
    @NSApplicationDelegateAdaptor(AppDelegate.self) private var delegate
    private let app = AppModel.shared

    var body: some Scene {
        WindowGroup("Raptor", id: "main") {
            MainView()
                .environment(app)
                .environment(app.model)
                .frame(minWidth: 520, minHeight: 320)
        }
        .defaultSize(width: 1180, height: 760)
        .windowToolbarStyle(.unified(showsTitle: true))
        .commands { RaptorCommands(app: app) }

        Settings {
            SettingsWindow()
                .environment(app)
                .environment(app.model)
        }
    }
}

@MainActor
final class AppDelegate: NSObject, NSApplicationDelegate, UNUserNotificationCenterDelegate {
    func applicationDidFinishLaunching(_ note: Foundation.Notification) {
        let app = AppModel.shared
        NSWindow.allowsAutomaticWindowTabbing = false
        app.model.onNotify = { n in Notifier.post(n) }
        app.startWatchingSystem()
        UNUserNotificationCenter.current().delegate = self
        Notifier.registerCategories()
        observeBadge()
        DispatchQueue.main.async {
            // Raptor has no text formatting; drop the empty Format menu SwiftUI leaves behind.
            if let m = NSApp.mainMenu, let i = m.items.firstIndex(where: { $0.title == "Format" }) { m.removeItem(at: i) }
            // NavigationSplitView files a second sidebar toggle under Help; View already has one.
            if let help = NSApp.mainMenu?.items.last?.submenu, let i = help.items.firstIndex(where: { $0.title == "Toggle Sidebar" }) { help.removeItem(at: i) }
        }
        #if DEBUG
        Automation.start()
        #endif
        // Quitting with every window closed would otherwise relaunch to an empty menu bar.
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) {
            if !NSApp.windows.contains(where: { $0.isVisible && $0.identifier?.rawValue.hasPrefix("main") == true }) {
                AppModel.shared.openMainWindow?()
            }
        }
    }

    func applicationDidBecomeActive(_ note: Foundation.Notification) {
        AppModel.shared.model.setForeground(true)
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { false }

    /// Keep the Dock badge in step with how many agents are waiting.
    private func observeBadge() {
        withObservationTracking {
            let n = AppModel.shared.blockedCount
            NSApp.dockTile.badgeLabel = n > 0 ? "\(n)" : nil
        } onChange: {
            DispatchQueue.main.async { self.observeBadge() }
        }
    }

    // MARK: notifications

    func userNotificationCenter(_ center: UNUserNotificationCenter, willPresent notification: UNNotification) async -> UNNotificationPresentationOptions {
        [.banner, .sound, .list]
    }

    func userNotificationCenter(_ center: UNUserNotificationCenter, didReceive response: UNNotificationResponse) async {
        let info = response.notification.request.content.userInfo
        guard let srv = info["server"] as? String, let ses = info["session"] as? String, let block = info["block"] as? String else { return }
        await MainActor.run {
            let app = AppModel.shared
            if let reply = (response as? UNTextInputNotificationResponse)?.userText {
                Task { try? await app.model.core.quickReply(serverId: srv, sessionId: ses, blockId: block, text: reply + "\r") }
                return
            }
            NSApp.activate(ignoringOtherApps: true)
            app.show(serverId: srv, sessionId: ses, blockId: block)
        }
    }
}

/// Posts agent and program notifications through Notification Center.
enum Notifier {
    static let category = "dev.raptor.agent"

    static func registerCategories() {
        let reply = UNTextInputNotificationAction(identifier: "reply", title: "Reply", options: [], textInputButtonTitle: "Send", textInputPlaceholder: "y, 1, or a message")
        let open = UNNotificationAction(identifier: "open", title: "Show", options: [.foreground])
        UNUserNotificationCenter.current().setNotificationCategories([
            UNNotificationCategory(identifier: category, actions: [reply, open], intentIdentifiers: []),
            UNNotificationCategory(identifier: "dev.raptor.info", actions: [open], intentIdentifiers: []),
        ])
    }

    @MainActor
    static func post(_ n: RaptorKit.Notification) {
        let model = AppModel.shared.model
        let wanted: Bool = switch n.kind {
        case .agentBlocked, .agentError: Prefs.flag(model, Prefs.notifyBlocked)
        case .agentDone: Prefs.flag(model, Prefs.notifyDone)
        case .processExited: Prefs.flag(model, Prefs.notifyExit, default: false)
        case .program: Prefs.flag(model, Prefs.notifyProgram)
        case .bell: false
        }
        // The pane in front of a focused window already shows it.
        if NSApp.isActive, let ws = AppModel.shared.current, ws.sessionId == n.sessionId, ws.focusedBlockId == n.blockId { return }
        guard wanted else { return }
        let content = UNMutableNotificationContent()
        content.title = n.title
        content.body = n.body
        if let s = model.session(n.serverId, n.sessionId) { content.subtitle = s.label }
        content.sound = n.kind == .agentBlocked ? .default : nil
        content.categoryIdentifier = n.kind == .agentBlocked ? category : "dev.raptor.info"
        content.threadIdentifier = n.blockId
        content.userInfo = ["server": n.serverId, "session": n.sessionId, "block": n.blockId]
        let center = UNUserNotificationCenter.current()
        center.requestAuthorization(options: [.alert, .sound, .badge]) { granted, _ in
            guard granted else { return }
            center.add(UNNotificationRequest(identifier: "\(n.blockId)-\(n.kind)", content: content, trigger: nil))
        }
    }
}
