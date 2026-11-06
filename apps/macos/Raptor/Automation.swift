#if DEBUG
import AppKit
import Carbon.HIToolbox
import RaptorKit

/// Debug builds only: drive the app with synthesized events without it being
/// the active app, so UI tests can run beside other work. Commands arrive as
/// `dev.raptor.automation` distributed notifications (object = command line);
/// results are appended to the file named by RAPTOR_AUTOMATION_LOG.
@MainActor
enum Automation {
    static func start() {
        DistributedNotificationCenter.default().addObserver(forName: Foundation.Notification.Name("dev.raptor.automation"), object: nil, queue: .main) { n in
            let line = n.object as? String ?? ""
            MainActor.assumeIsolated { log(run(line)) }
        }
    }

    private static func log(_ s: String) {
        guard let path = ProcessInfo.processInfo.environment["RAPTOR_AUTOMATION_LOG"] else { return }
        let data = (s + "\n").data(using: .utf8)!
        if let h = FileHandle(forWritingAtPath: path) {
            h.seekToEndOfFile()
            h.write(data)
            h.closeFile()
        } else {
            FileManager.default.createFile(atPath: path, contents: data)
        }
    }

    private static var window: NSWindow? {
        AppModel.shared.current?.window ?? NSApp.windows.first { $0.isVisible && $0.contentView != nil && !($0 is NSPanel) }
    }

    private static func run(_ line: String) -> String {
        let args = line.split(separator: " ", maxSplits: 1).map(String.init)
        guard let cmd = args.first else { return "err empty" }
        let rest = args.count > 1 ? args[1] : ""
        guard let w = window else { return "err no window" }
        switch cmd {
        case "key":
            for spec in rest.split(separator: " ") { key(String(spec), in: w) }
            return "ok key \(rest)"
        case "type":
            for ch in rest { key(String(ch), in: w) }
            return "ok type"
        case "text":
            (w.firstResponder as? NSTextInputClient)?.insertText(rest, replacementRange: NSRange(location: NSNotFound, length: 0))
            return "ok text"
        case "click", "rclick", "dclick", "tclick":
            let n = rest.split(separator: " ").compactMap { Double($0) }
            guard n.count >= 2 else { return "err click x y" }
            let p = point(n[0], n[1], w)
            let count = cmd == "dclick" ? 2 : cmd == "tclick" ? 3 : 1
            let mods: NSEvent.ModifierFlags = n.count > 2 && n[2] == 1 ? [.shift] : []
            if cmd == "rclick" {
                mouse(.rightMouseDown, p, w, mods: mods)
                mouse(.rightMouseUp, p, w, mods: mods)
            } else {
                for c in 1...count {
                    mouse(.leftMouseDown, p, w, clicks: c, mods: mods)
                    mouse(.leftMouseUp, p, w, clicks: c, mods: mods)
                }
            }
            return "ok \(cmd)"
        case "drag":
            let n = rest.split(separator: " ").compactMap { Double($0) }
            guard n.count == 4 else { return "err drag x1 y1 x2 y2" }
            let a = point(n[0], n[1], w), b = point(n[2], n[3], w)
            mouse(.leftMouseDown, a, w)
            for i in 1...12 {
                let t = CGFloat(i) / 12
                mouse(.leftMouseDragged, NSPoint(x: a.x + (b.x - a.x) * t, y: a.y + (b.y - a.y) * t), w)
            }
            mouse(.leftMouseUp, b, w)
            return "ok drag"
        case "scroll":
            let n = rest.split(separator: " ").compactMap { Double($0) }
            guard n.count >= 3 else { return "err scroll x y dy" }
            scroll(point(n[0], n[1], w), dy: n[2], w)
            return "ok scroll"
        case "menu":
            return menu(rest) ? "ok menu \(rest)" : "err no menu item \(rest)"
        case "size":
            let n = rest.split(separator: " ").compactMap { Double($0) }
            guard n.count == 2 else { return "err size w h" }
            w.setContentSize(NSSize(width: n[0], height: n[1]))
            return "ok size"
        case "open":
            let m = AppModel.shared.model
            for srv in m.state.servers {
                if let s = srv.sessions.first(where: { $0.label == rest }) {
                    AppModel.shared.current?.open(serverId: srv.id, sessionId: s.id)
                    return "ok open \(rest)"
                }
            }
            return "err no session \(rest)"
        case "dump":
            return dump(w)
        case "snapself":
            // Offscreen render of the window's views, for when the screen can't be captured.
            guard let v = w.contentView?.superview ?? w.contentView, let rep = v.bitmapImageRepForCachingDisplay(in: v.bounds) else { return "err snapself" }
            v.cacheDisplay(in: v.bounds, to: rep)
            try? rep.representation(using: .png, properties: [:])?.write(to: URL(fileURLWithPath: rest))
            return "ok snapself \(rest)"
        case "views":
            guard let t = AppModel.shared.current?.focusedTerminal else { return "err" }
            return t.subviews.map { "\(type(of: $0)) \($0.frame) alpha=\($0.alphaValue) hidden=\($0.isHidden)" }.joined(separator: "\n") + "\nterm \(t.frame) window \(String(describing: t.window?.frame))"
        case "row":
            guard let r = Int(rest), let f = AppModel.shared.current?.focusedTerminal?.frameCache, f.lines.indices.contains(r) else { return "err row n" }
            return f.lines[r].runs.map { run in
                "[\(run.col)+\(run.cells) \(run.text.unicodeScalars.map { String($0.value, radix: 16) }.joined(separator: " ")) w=\([UInt8](run.widths))]"
            }.joined(separator: "\n")
        case "hit":
            let n = rest.split(separator: " ").compactMap { Double($0) }
            guard n.count == 2, let cv = w.contentView else { return "err hit x y" }
            let p = cv.superview?.convert(point(n[0], n[1], w), from: nil) ?? .zero
            var v = cv.superview?.hitTest(p)
            var chain: [String] = []
            while let x = v { chain.append("\(type(of: x))\(x === v ? "" : "")"); v = x.superview }
            return "hit " + chain.prefix(6).joined(separator: " < ")
        case "menus":
            func walk(_ m: NSMenu, _ depth: Int) -> [String] {
                m.items.flatMap { i -> [String] in
                    let mask = i.keyEquivalentModifierMask
                    let k = i.keyEquivalent.isEmpty ? "" : " [\(mask.contains(.command) ? "⌘" : "")\(mask.contains(.shift) ? "⇧" : "")\(mask.contains(.option) ? "⌥" : "")\(mask.contains(.control) ? "⌃" : "")\(i.keyEquivalent.debugDescription)]"
                    let me = i.isSeparatorItem ? [] : [String(repeating: "  ", count: depth) + i.title + k]
                    return me + (i.submenu.map { walk($0, depth + 1) } ?? [])
                }
            }
            return NSApp.mainMenu.map { walk($0, 0).joined(separator: "\n") } ?? "no menu"
        default:
            return "err unknown \(cmd)"
        }
    }

    /// Window coordinates from the top-left of the content area.
    private static func point(_ x: Double, _ y: Double, _ w: NSWindow) -> NSPoint {
        NSPoint(x: x, y: (w.contentView?.bounds.height ?? 0) - y)
    }

    private static func mouse(_ type: NSEvent.EventType, _ p: NSPoint, _ w: NSWindow, clicks: Int = 1, mods: NSEvent.ModifierFlags = []) {
        guard let e = NSEvent.mouseEvent(with: type, location: p, modifierFlags: mods, timestamp: ProcessInfo.processInfo.systemUptime,
                                         windowNumber: w.windowNumber, context: nil, eventNumber: 0, clickCount: clicks, pressure: 1) else { return }
        if type == .rightMouseDown, let v = w.contentView?.hitTest(w.contentView!.convert(p, from: nil)), let m = v.menu(for: e) {
            // A real right click would track the menu; report its items instead.
            log("menu: " + m.items.map { $0.isSeparatorItem ? "|" : $0.title + ($0.isEnabled ? "" : "(off)") }.joined(separator: ", "))
            return
        }
        NSApp.postEvent(e, atStart: false)
    }

    private static func scroll(_ p: NSPoint, dy: Double, _ w: NSWindow) {
        guard let cg = CGEvent(scrollWheelEvent2Source: nil, units: .pixel, wheelCount: 1, wheel1: Int32(dy), wheel2: 0, wheel3: 0) else { return }
        let screen = w.convertPoint(toScreen: p)
        let h = NSScreen.screens.first?.frame.height ?? 0
        cg.location = CGPoint(x: screen.x, y: h - screen.y)
        cg.setIntegerValueField(.scrollWheelEventIsContinuous, value: 1)
        if let e = NSEvent(cgEvent: cg) {
            let v = w.contentView?.hitTest(w.contentView!.convert(p, from: nil))
            v?.scrollWheel(with: e)
        }
    }

    private static let names: [String: (Int, String)] = [
        "return": (kVK_Return, "\r"), "enter": (kVK_Return, "\r"), "tab": (kVK_Tab, "\t"), "esc": (kVK_Escape, "\u{1b}"),
        "space": (kVK_Space, " "), "backspace": (kVK_Delete, "\u{7f}"), "delete": (kVK_ForwardDelete, "\u{f728}"),
        "up": (kVK_UpArrow, "\u{f700}"), "down": (kVK_DownArrow, "\u{f701}"), "left": (kVK_LeftArrow, "\u{f702}"), "right": (kVK_RightArrow, "\u{f703}"),
        "home": (kVK_Home, "\u{f729}"), "end": (kVK_End, "\u{f72b}"), "pageup": (kVK_PageUp, "\u{f72c}"), "pagedown": (kVK_PageDown, "\u{f72d}"),
        "f1": (kVK_F1, "\u{f704}"), "f2": (kVK_F2, "\u{f705}"), "f5": (kVK_F5, "\u{f708}"), "f10": (kVK_F10, "\u{f70d}"),
    ]

    private static let letterCodes: [Character: Int] = [
        "a": kVK_ANSI_A, "b": kVK_ANSI_B, "c": kVK_ANSI_C, "d": kVK_ANSI_D, "e": kVK_ANSI_E, "f": kVK_ANSI_F, "g": kVK_ANSI_G, "h": kVK_ANSI_H,
        "i": kVK_ANSI_I, "j": kVK_ANSI_J, "k": kVK_ANSI_K, "l": kVK_ANSI_L, "m": kVK_ANSI_M, "n": kVK_ANSI_N, "o": kVK_ANSI_O, "p": kVK_ANSI_P,
        "q": kVK_ANSI_Q, "r": kVK_ANSI_R, "s": kVK_ANSI_S, "t": kVK_ANSI_T, "u": kVK_ANSI_U, "v": kVK_ANSI_V, "w": kVK_ANSI_W, "x": kVK_ANSI_X,
        "y": kVK_ANSI_Y, "z": kVK_ANSI_Z, "0": kVK_ANSI_0, "1": kVK_ANSI_1, "2": kVK_ANSI_2, "3": kVK_ANSI_3, "4": kVK_ANSI_4, "5": kVK_ANSI_5,
        "6": kVK_ANSI_6, "7": kVK_ANSI_7, "8": kVK_ANSI_8, "9": kVK_ANSI_9, "[": kVK_ANSI_LeftBracket, "]": kVK_ANSI_RightBracket,
        "-": kVK_ANSI_Minus, "=": kVK_ANSI_Equal, ";": kVK_ANSI_Semicolon, "'": kVK_ANSI_Quote, ",": kVK_ANSI_Comma, ".": kVK_ANSI_Period,
        "/": kVK_ANSI_Slash, "\\": kVK_ANSI_Backslash, "`": kVK_ANSI_Grave, "+": kVK_ANSI_Equal, " ": kVK_Space,
    ]

    /// "cmd+shift+d", "ctrl+c", "return", "x", "opt+e".
    private static func key(_ spec: String, in w: NSWindow) {
        var mods: NSEvent.ModifierFlags = []
        var parts = spec.count > 1 ? spec.split(separator: "+", omittingEmptySubsequences: false).map(String.init) : [spec]
        if spec.hasSuffix("++") { parts = Array(parts.dropLast(2)) + ["+"] }
        let base = parts.removeLast()
        for m in parts {
            switch m {
            case "cmd": mods.insert(.command)
            case "shift": mods.insert(.shift)
            case "ctrl": mods.insert(.control)
            case "opt", "alt": mods.insert(.option)
            default: break
            }
        }
        var code = 0
        var chars = base
        if let n = names[base.lowercased()] {
            code = n.0
            chars = n.1
        } else if let c = base.lowercased().first, let k = letterCodes[c] {
            code = k
            if base.first!.isUppercase { mods.insert(.shift) }
        }
        if mods.contains(.shift), chars.count == 1, chars.first!.isLetter { chars = chars.uppercased() }
        let plain = chars
        if mods.contains(.control), let c = chars.unicodeScalars.first, c.isASCII, c.properties.isAlphabetic {
            chars = String(UnicodeScalar(UInt8(c.value & 0x1f)))
        }
        if mods.contains(.option), base == "e" { chars = "" }
        guard let down = NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: mods, timestamp: ProcessInfo.processInfo.systemUptime,
                                          windowNumber: w.windowNumber, context: nil, characters: chars,
                                          charactersIgnoringModifiers: plain, isARepeat: false, keyCode: UInt16(code)) else { return }
        if mods.contains(.command) {
            // Find the item bound to this shortcut, as AppKit's menu matching would.
            let want = mods.intersection([.command, .shift, .option, .control])
            func find(_ m: NSMenu) -> (NSMenu, Int)? {
                for (i, item) in m.items.enumerated() {
                    if item.keyEquivalent.lowercased() == plain.lowercased(), item.keyEquivalentModifierMask == want { return (m, i) }
                    if let sub = item.submenu, let r = find(sub) { return r }
                }
                return nil
            }
            if let main = NSApp.mainMenu, let (m, i) = find(main) {
                m.update()
                if m.items[i].isEnabled { m.performActionForItem(at: i) }
                log("menu item: \(m.items[i].title)")
                return
            }
            if w.performKeyEquivalent(with: down) { return }
        } else if mods.contains(.control), w.performKeyEquivalent(with: down) {
            return
        }
        w.sendEvent(down)
    }

    private static func menu(_ path: String) -> Bool {
        func find(_ menu: NSMenu, _ title: String) -> (NSMenu, Int)? {
            for (i, item) in menu.items.enumerated() {
                if item.title == title { return (menu, i) }
                if let sub = item.submenu, let r = find(sub, title) { return r }
            }
            return nil
        }
        guard let main = NSApp.mainMenu, let (menu, i) = find(main, path) else { return false }
        menu.update()
        menu.performActionForItem(at: i)
        return true
    }

    private static func dump(_ w: NSWindow) -> String {
        let ws = AppModel.shared.current
        var out = ["window \(w.windowNumber) \(Int(w.frame.width))x\(Int(w.frame.height)) key=\(w.isKeyWindow) title=\(w.title)"]
        out.append("responder \(w.firstResponder.map { String(describing: type(of: $0)) } ?? "nil")")
        out.append("session \(ws?.session?.label ?? "-") tab \(ws?.activeWindow.map(tabTitle) ?? "-") tabs \(ws?.session?.windows.count ?? 0)")
        if let aw = ws?.activeWindow {
            for b in aw.blocks {
                let t = ws?.terminals[b.id]
                let f = t?.frameCache
                let mark = b.id == ws?.focusedBlockId ? "*" : " "
                out.append("\(mark) \(b.name) rect=\(String(format: "%.3f,%.3f,%.3f,%.3f", b.x, b.y, b.w, b.h)) grid=\(f.map { "\($0.cols)x\($0.rows)" } ?? "-") off=\(f.map { "\($0.scrollOffset)/\($0.scrollTotal)" } ?? "-") ctl=\(t?.controller.hasControl ?? false) status=\(t.map { "\($0.controller.status)" } ?? "-") sel=\(t?.terminal?.selectionText()?.prefix(60).debugDescription ?? "nil")")
            }
            for s in aw.splits { out.append("  split \(s.horizontal ? "h" : "v") ratio=\(String(format: "%.3f", s.ratio))") }
        }
        out.append("servers " + AppModel.shared.model.state.servers.map { "\($0.name)=\($0.connectivity)" }.joined(separator: " "))
        out.append("badge \(NSApp.dockTile.badgeLabel ?? "-") attention \(AppModel.shared.model.state.attention.count)")
        return out.joined(separator: "\n")
    }
}
#endif
