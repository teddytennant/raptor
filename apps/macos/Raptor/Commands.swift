import AppKit
import RaptorKit
import SwiftUI

/// The menu bar. Every command acts on the front Raptor window's workspace.
struct RaptorCommands: Commands {
    let app: AppModel
    @Environment(\.openWindow) private var openWindow
    private var ws: Workspace? { app.current }

    var body: some Commands {
        SidebarCommands()
        CommandGroup(replacing: .appInfo) {
            Button("About Raptor") { showAbout() }
        }
        CommandGroup(after: .newItem) {
            Button("New Tab") { ws?.newTab() }
                .keyboardShortcut("t")
            Button("New Session") { ws?.newSession() }
                .keyboardShortcut("n", modifiers: [.command, .shift])
            Divider()
            Button("Split Right") { ws?.split(vertical: false) }
                .keyboardShortcut("d")
            Button("Split Down") { ws?.split(vertical: true) }
                .keyboardShortcut("d", modifiers: [.command, .shift])
            Divider()
            Button("Close Pane") {
                if let w = NSApp.keyWindow, w !== ws?.window { w.performClose(nil) } else { ws?.closePane() }
            }
                .keyboardShortcut("w")
            Button("Close Tab") { ws?.closeTab() }
                .keyboardShortcut("w", modifiers: [.command, .option])
            Button("Close Window") { NSApp.keyWindow?.performClose(nil) }
                .keyboardShortcut("w", modifiers: [.command, .shift])
        }
        CommandGroup(replacing: .pasteboard) {
            Button("Copy") { NSApp.sendAction(#selector(NSText.copy(_:)), to: nil, from: nil) }
                .keyboardShortcut("c")
            Button("Paste") { NSApp.sendAction(#selector(NSText.paste(_:)), to: nil, from: nil) }
                .keyboardShortcut("v")
            Button("Select All") { NSApp.sendAction(#selector(NSText.selectAll(_:)), to: nil, from: nil) }
                .keyboardShortcut("a")
            Divider()
            Button("Find…") { ws?.findShown = true }
                .keyboardShortcut("f")
        }
        CommandGroup(replacing: .saveItem) {}
        CommandGroup(replacing: .textFormatting) {}
        CommandGroup(after: .toolbar) {
            Button("Command Palette…") { ws?.paletteShown.toggle() }
                .keyboardShortcut("p", modifiers: [.command, .shift])
            Button("Zoom Pane") { ws?.toggleZoom() }
                .keyboardShortcut(.return, modifiers: [.command, .shift])
            Divider()
            Button("Bigger") { app.biggerText() }
                .keyboardShortcut("+")
            Button("Bigger") { app.biggerText() }
                .keyboardShortcut("=")
            Button("Smaller") { app.smallerText() }
                .keyboardShortcut("-")
            Button("Actual Size") { app.actualSize() }
                .keyboardShortcut("0")
            Divider()
        }
        CommandMenu("Session") {
            Button("Rename Session…") { ws?.renameSession() }
            Button("Rename Tab…") { ws?.renameTab() }
            Divider()
            Button("Show Next Tab") { ws?.cycleTab(1) }
                .keyboardShortcut("]", modifiers: [.command, .shift])
            Button("Show Previous Tab") { ws?.cycleTab(-1) }
                .keyboardShortcut("[", modifiers: [.command, .shift])
            ForEach(1..<10) { i in
                Button(i == 9 ? "Show Last Tab" : "Show Tab \(i)") { ws?.selectTab(index: i == 9 ? -1 : i - 1) }
                    .keyboardShortcut(KeyEquivalent(Character("\(i)")))
            }
            Divider()
            Button("Next Pane") { ws?.cyclePane(1) }
                .keyboardShortcut("]")
            Button("Previous Pane") { ws?.cyclePane(-1) }
                .keyboardShortcut("[")
            Button("Pane Left") { ws?.focusPane(.left) }
                .keyboardShortcut(.leftArrow, modifiers: [.command, .option])
            Button("Pane Right") { ws?.focusPane(.right) }
                .keyboardShortcut(.rightArrow, modifiers: [.command, .option])
            Button("Pane Above") { ws?.focusPane(.up) }
                .keyboardShortcut(.upArrow, modifiers: [.command, .option])
            Button("Pane Below") { ws?.focusPane(.down) }
                .keyboardShortcut(.downArrow, modifiers: [.command, .option])
            Divider()
            Button("Reconnect All Servers") { app.reconnectAll() }
                .keyboardShortcut("r", modifiers: [.command, .shift])
        }
    }

    private func showAbout() {
        let credits = NSMutableAttributedString(
            string: "A native client for Rex terminal servers.\n",
            attributes: [.font: NSFont.systemFont(ofSize: 11), .foregroundColor: NSColor.secondaryLabelColor]
        )
        credits.append(NSAttributedString(string: "github.com/teddytennant/raptor", attributes: [
            .font: NSFont.systemFont(ofSize: 11), .link: URL(string: "https://github.com/teddytennant/raptor")!,
        ]))
        let p = NSMutableParagraphStyle()
        p.alignment = .center
        credits.addAttribute(.paragraphStyle, value: p, range: NSRange(location: 0, length: credits.length))
        NSApp.orderFrontStandardAboutPanel(options: [.credits: credits])
        NSApp.activate(ignoringOtherApps: true)
    }
}
