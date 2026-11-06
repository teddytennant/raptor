import AppKit
import Carbon.HIToolbox
import RaptorKit

/// One terminal: draws frames with Core Text and turns keys, IME, mouse and
/// trackpad input into terminal input.
final class TerminalView: NSView, NSTextInputClient, NSMenuItemValidation {
    let controller: TerminalController
    let renderer: TerminalRenderer
    var terminal: Terminal? { controller.terminal }

    /// The user clicked or typed here; the layout should focus this pane.
    var onActivate: (() -> Void)?
    var onFontSize: ((CGFloat) -> Void)?
    var onAction: ((PaneAction) -> Void)?
    /// The pane is the window's focused block (draws a solid cursor when the window is key).
    var isFocusedPane = false { didSet { if isFocusedPane != oldValue { needsDisplay = true } } }

    var prefs = TerminalPrefs() {
        didSet {
            guard prefs != oldValue else { return }
            renderer.setAppearance(fontId: prefs.fontId, lineHeight: prefs.lineHeight)
            renderer.setFontSize(prefs.fontSize)
            pushGrid()
            updateBlink()
            needsDisplay = true
        }
    }

    private(set) var frameCache: Frame?
    private let padding: CGFloat = 8
    private var marked = NSMutableAttributedString()
    private var markedSelection = NSRange(location: 0, length: 0)
    private var scrollAccumulator: CGFloat = 0
    private var blinkOn = true
    private var blinkTimer: Timer?
    private var anchor: (col: Int, row: Int)?
    private var selectionMode = 1
    private var dragging = false
    private var mouseButtonsDown: Set<String> = []
    private var lastMotionCell: (Int, Int)?
    private var autoscrollTimer: Timer?
    private var lastDragPoint = NSPoint.zero
    private let scroller = HistoryScroller()
    private var trackingArea: NSTrackingArea?
    private var lastGrid: (Int, Int)?
    /// Where the last find matched: absolute row (history offset + row) and column.
    var findPos: (row: Int, col: Int)?

    init(controller: TerminalController, prefs: TerminalPrefs) {
        self.controller = controller
        self.prefs = prefs
        renderer = TerminalRenderer(fontSize: prefs.fontSize)
        renderer.setAppearance(fontId: prefs.fontId, lineHeight: prefs.lineHeight)
        super.init(frame: .zero)
        wantsLayer = true
        layerContentsRedrawPolicy = .onSetNeedsDisplay
        layer?.drawsAsynchronously = false
        controller.onFrame = { [weak self] in self?.refresh() }
        controller.onClipboard = { text in
            NSPasteboard.general.clearContents()
            NSPasteboard.general.setString(text, forType: .string)
        }
        scroller.onScroll = { [weak self] fraction in self?.scrollTo(fraction: fraction) }
        addSubview(scroller)
        setAccessibilityRole(.textArea)
        setAccessibilityLabel("Terminal")
        refresh()
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError() }

    deinit {
        blinkTimer?.invalidate()
        autoscrollTimer?.invalidate()
    }

    override var isFlipped: Bool { true }
    override var isOpaque: Bool { true }
    override var acceptsFirstResponder: Bool { true }
    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }

    // MARK: frames and drawing

    func refresh() {
        let prev = frameCache
        frameCache = controller.frame()
        guard let f = frameCache else { return }
        if prev?.background != f.background {
            layer?.backgroundColor = cgColor(f.background)
            scroller.color = NSColor(cgColor: cgColor(f.foreground)) ?? .white
        }
        if prev?.cols != f.cols || prev?.rows != f.rows { invalidateIntrinsicContentSize() }
        blinkOn = true
        scroller.update(total: f.scrollTotal, offset: f.scrollOffset, len: f.scrollLen)
        updateBlink()
        updateCursorRects()
        needsDisplay = true
    }

    /// Scale used when another client holds the size and its grid is bigger than this pane.
    var scale: CGFloat {
        guard let f = frameCache, !controller.hasControl else { return 1 }
        let m = renderer.metrics
        let needW = CGFloat(f.cols) * m.width + padding * 2
        let needH = CGFloat(f.rows) * m.height + padding * 2
        return min(1, bounds.width / needW, bounds.height / needH)
    }

    override func draw(_ dirtyRect: NSRect) {
        guard let ctx = NSGraphicsContext.current?.cgContext else { return }
        guard let f = frameCache else {
            ctx.setFillColor(cgColor(0xFF1A1B26))
            ctx.fill(bounds)
            return
        }
        var o = TerminalRenderer.Options()
        o.padding = padding
        o.scale = scale
        o.focused = isFocusedPane && window?.isKeyWindow == true && window?.firstResponder === self
        o.cursorVisible = blinkOn || !o.focused || !prefs.blink || hasMarkedText()
        o.cursorShape = prefs.cursorShape
        o.historyBar = false
        renderer.draw(f, in: ctx, size: bounds.size, options: o)
        drawMarkedText(in: ctx, frame: f)
    }

    private func drawMarkedText(in ctx: CGContext, frame f: Frame) {
        guard marked.length > 0, let c = f.cursor else { return }
        let m = renderer.metrics
        let s = scale
        let origin = CGPoint(x: padding + CGFloat(c.col) * m.width * s, y: padding + CGFloat(c.row) * m.height * s)
        let font = NSFont(name: renderer.font.faces[0], size: renderer.fontSize * s) ?? .monospacedSystemFont(ofSize: renderer.fontSize * s, weight: .regular)
        let attrs: [NSAttributedString.Key: Any] = [
            .font: font,
            .foregroundColor: NSColor(cgColor: cgColor(f.foreground)) ?? .white,
            .underlineStyle: NSUnderlineStyle.single.rawValue,
        ]
        let str = NSAttributedString(string: marked.string, attributes: attrs)
        let width = str.size().width
        ctx.setFillColor(cgColor(f.background))
        ctx.fill(CGRect(x: origin.x, y: origin.y, width: width + 2, height: m.height * s))
        NSGraphicsContext.saveGraphicsState()
        str.draw(at: NSPoint(x: origin.x, y: origin.y + (m.height * s - font.ascender + font.descender) / 2))
        NSGraphicsContext.restoreGraphicsState()
    }

    override func layout() {
        super.layout()
        scroller.frame = NSRect(x: bounds.width - 16, y: 0, width: 16, height: bounds.height)
        pushGrid()
    }

    override func viewDidChangeBackingProperties() {
        super.viewDidChangeBackingProperties()
        lastGrid = nil
        pushGrid()
    }

    func pushGrid() {
        guard bounds.width > 20, bounds.height > 20, let t = terminal else { return }
        let g = renderer.grid(for: bounds.size, padding: padding)
        let px = window?.backingScaleFactor ?? 2
        t.setDeviceGrid(cols: UInt16(g.cols), rows: UInt16(g.rows),
                        cellWidthPx: UInt32((renderer.metrics.width * px).rounded()),
                        cellHeightPx: UInt32((renderer.metrics.height * px).rounded()))
        lastGrid = (g.cols, g.rows)
    }

    // MARK: focus and blink

    override func becomeFirstResponder() -> Bool {
        blinkOn = true
        needsDisplay = true
        updateBlink()
        sendFocus(true)
        return true
    }

    override func resignFirstResponder() -> Bool {
        needsDisplay = true
        updateBlink()
        sendFocus(false)
        return true
    }

    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        NotificationCenter.default.removeObserver(self)
        guard let window else {
            blinkTimer?.invalidate()
            blinkTimer = nil
            return
        }
        for name in [NSWindow.didBecomeKeyNotification, NSWindow.didResignKeyNotification] {
            NotificationCenter.default.addObserver(self, selector: #selector(keyChanged), name: name, object: window)
        }
        pushGrid()
    }

    @objc private func keyChanged() {
        blinkOn = true
        updateBlink()
        needsDisplay = true
    }

    private func sendFocus(_ inside: Bool) {
        terminal?.sendEvent(kind: "focus", payloadJson: inside ? #"{"action":"in"}"# : #"{"action":"out"}"#)
    }

    /// The blink timer only runs while a blinking cursor is on screen in the key window.
    private func updateBlink() {
        let wants = prefs.blink && frameCache?.cursor?.blinking != false && isFocusedPane && window?.isKeyWindow == true
            && window?.firstResponder === self && frameCache?.cursor != nil
        if wants, blinkTimer == nil {
            let t = Timer(timeInterval: 0.53, repeats: true) { [weak self] _ in
                MainActor.assumeIsolated {
                    guard let self else { return }
                    self.blinkOn.toggle()
                    self.setNeedsDisplay(self.cursorRect().insetBy(dx: -2, dy: -2))
                }
            }
            t.tolerance = 0.05
            RunLoop.main.add(t, forMode: .common)
            blinkTimer = t
        } else if !wants, let t = blinkTimer {
            t.invalidate()
            blinkTimer = nil
            blinkOn = true
        }
    }

    private func cursorRect() -> NSRect {
        guard let c = frameCache?.cursor else { return .zero }
        let m = renderer.metrics, s = scale
        return NSRect(x: padding + CGFloat(c.col) * m.width * s, y: padding + CGFloat(c.row) * m.height * s,
                      width: m.width * s * (c.wide ? 2 : 1), height: m.height * s)
    }

    // MARK: geometry

    func cell(at p: NSPoint) -> (col: Int, row: Int) {
        let m = renderer.metrics, s = scale
        let col = Int(floor((p.x - padding) / (m.width * s)))
        let row = Int(floor((p.y - padding) / (m.height * s)))
        let f = frameCache
        return (min(max(col, 0), Int(f?.cols ?? 1) - 1), min(max(row, 0), Int(f?.rows ?? 1) - 1))
    }

    private var programWantsMouse: Bool { terminal?.mouseTracking() == true }

    // MARK: keyboard

    override func performKeyEquivalent(with e: NSEvent) -> Bool {
        guard window?.firstResponder === self else { return false }
        let mods = e.modifierFlags.intersection(.deviceIndependentFlagsMask)
        if mods.contains(.command) {
            switch Int(e.keyCode) {
            case kVK_UpArrow where mods == [.command]: terminal?.jumpPrompt(delta: -1); return true
            case kVK_DownArrow where mods == [.command]: terminal?.jumpPrompt(delta: 1); return true
            case kVK_Home, kVK_UpArrow where mods == [.command, .shift]: scrollTo(fraction: 0); return true
            case kVK_End, kVK_DownArrow where mods == [.command, .shift]: terminal?.scrollToBottom(); return true
            case kVK_PageUp: page(-1); return true
            case kVK_PageDown: page(1); return true
            default: return false
            }
        }
        // Ctrl+Tab and friends reach keyDown only through here.
        if mods.contains(.control), [kVK_Tab, kVK_Return, kVK_Escape].contains(Int(e.keyCode)) {
            keyDown(with: e)
            return true
        }
        return false
    }

    override func keyDown(with e: NSEvent) {
        NSCursor.setHiddenUntilMouseMoves(true)
        onActivate?()
        if controller.status.isTerminalEnded { return }
        if hasMarkedText() {
            interpretKeyEvents([e])
            return
        }
        let mods = e.modifierFlags.intersection(.deviceIndependentFlagsMask)
        if mods.contains(.command) {
            super.keyDown(with: e)
            return
        }
        let shift = mods.contains(.shift), ctrl = mods.contains(.control)
        let meta = optionIsMeta(e)
        let alt = mods.contains(.option) && meta
        if let key = KeyMap.special(e) {
            if mods == [.shift], key == .pageUp || key == .pageDown, frameCache?.alternateScreen == false {
                page(key == .pageUp ? -1 : 1)
                return
            }
            send(key, ctrl: ctrl, alt: mods.contains(.option), shift: shift)
            return
        }
        if ctrl || alt {
            // Ctrl and Meta chords use the key's character without Option's symbol layer.
            let base = alt ? (e.charactersIgnoringModifiers ?? "") : (e.characters(byApplyingModifiers: shift ? [.shift] : []) ?? e.charactersIgnoringModifiers ?? "")
            if let ch = base.first {
                if ctrl, !alt, let c0 = KeyMap.controlCharacter(ch) {
                    terminal?.sendText(text: c0)
                } else {
                    send(.char(c: String(ch)), ctrl: ctrl, alt: alt, shift: shift)
                }
                return
            }
        }
        interpretKeyEvents([e])
    }

    private func optionIsMeta(_ e: NSEvent) -> Bool {
        let raw = e.modifierFlags.rawValue
        switch prefs.optionAsMeta {
        case .off: return false
        case .both: return e.modifierFlags.contains(.option)
        case .left: return raw & 0x20 != 0
        case .right: return raw & 0x40 != 0
        }
    }

    private func send(_ key: Key, ctrl: Bool = false, alt: Bool = false, shift: Bool = false) {
        terminal?.sendKey(key: key, ctrl: ctrl, alt: alt, shift: shift, meta: false)
    }

    override func doCommand(by selector: Selector) {
        switch selector {
        case #selector(insertNewline(_:)): send(.enter)
        case #selector(insertTab(_:)): send(.tab)
        case #selector(insertBacktab(_:)): send(.tab, shift: true)
        case #selector(deleteBackward(_:)): send(.backspace)
        case #selector(deleteForward(_:)): send(.delete)
        case #selector(cancelOperation(_:)): send(.escape)
        case #selector(moveUp(_:)): send(.up)
        case #selector(moveDown(_:)): send(.down)
        case #selector(moveLeft(_:)): send(.left)
        case #selector(moveRight(_:)): send(.right)
        default: break
        }
    }

    // MARK: NSTextInputClient

    func insertText(_ string: Any, replacementRange: NSRange) {
        let text = (string as? NSAttributedString)?.string ?? (string as? String) ?? ""
        unmarkText()
        guard !text.isEmpty else { return }
        terminal?.sendText(text: text)
    }

    func setMarkedText(_ string: Any, selectedRange: NSRange, replacementRange: NSRange) {
        let s = (string as? NSAttributedString)?.string ?? (string as? String) ?? ""
        marked = NSMutableAttributedString(string: s)
        markedSelection = selectedRange
        needsDisplay = true
    }

    func unmarkText() {
        if marked.length > 0 {
            marked = NSMutableAttributedString()
            needsDisplay = true
        }
        inputContext?.discardMarkedText()
    }

    func selectedRange() -> NSRange { NSRange(location: NSNotFound, length: 0) }
    func markedRange() -> NSRange { marked.length > 0 ? NSRange(location: 0, length: marked.length) : NSRange(location: NSNotFound, length: 0) }
    func hasMarkedText() -> Bool { marked.length > 0 }
    func attributedSubstring(forProposedRange range: NSRange, actualRange: NSRangePointer?) -> NSAttributedString? { nil }
    func validAttributesForMarkedText() -> [NSAttributedString.Key] { [] }
    func characterIndex(for point: NSPoint) -> Int { NSNotFound }

    func firstRect(forCharacterRange range: NSRange, actualRange: NSRangePointer?) -> NSRect {
        guard let window else { return .zero }
        return window.convertToScreen(convert(cursorRect(), to: nil))
    }

    // MARK: clipboard and menu commands

    @objc func copy(_ sender: Any?) {
        guard let text = terminal?.selectionText(), !text.isEmpty else { return }
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(text, forType: .string)
    }

    @objc func paste(_ sender: Any?) {
        guard let text = NSPasteboard.general.string(forType: .string), !text.isEmpty else { return }
        onActivate?()
        terminal?.paste(text: text)
    }

    @objc override func selectAll(_ sender: Any?) {
        guard let f = frameCache, f.rows > 0, f.cols > 0 else { return }
        _ = terminal?.selectRange(fromCol: 0, fromRow: 0, toCol: f.cols - 1, toRow: f.rows - 1)
    }

    @objc func clearSelection(_ sender: Any?) { terminal?.clearSelection() }

    func validateMenuItem(_ item: NSMenuItem) -> Bool {
        switch item.action {
        case #selector(copy(_:)): return terminal?.selectionText()?.isEmpty == false
        case #selector(paste(_:)): return NSPasteboard.general.string(forType: .string) != nil
        default: return true
        }
    }

    override func menu(for event: NSEvent) -> NSMenu? {
        let m = NSMenu()
        let hasSelection = terminal?.selectionText()?.isEmpty == false
        m.addItem(withTitle: "Copy", action: hasSelection ? #selector(copy(_:)) : nil, keyEquivalent: "")
        m.addItem(withTitle: "Paste", action: #selector(paste(_:)), keyEquivalent: "")
        m.addItem(withTitle: "Select All", action: #selector(selectAll(_:)), keyEquivalent: "")
        m.addItem(.separator())
        for a in [PaneAction.splitRight, .splitDown] {
            m.addItem(menuItem(a))
        }
        m.addItem(.separator())
        m.addItem(menuItem(.zoom))
        if !controller.hasControl { m.addItem(menuItem(.takeSize)) }
        m.addItem(.separator())
        m.addItem(menuItem(.close))
        return m
    }

    private func menuItem(_ a: PaneAction) -> NSMenuItem {
        let item = NSMenuItem(title: a.title, action: #selector(runPaneAction(_:)), keyEquivalent: "")
        item.target = self
        item.representedObject = a.rawValue
        item.image = NSImage(systemSymbolName: a.symbol, accessibilityDescription: nil)
        return item
    }

    @objc private func runPaneAction(_ sender: NSMenuItem) {
        guard let raw = sender.representedObject as? String, let a = PaneAction(rawValue: raw) else { return }
        onAction?(a)
    }

    // MARK: mouse

    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        if let t = trackingArea { removeTrackingArea(t) }
        let t = NSTrackingArea(rect: bounds, options: [.mouseMoved, .activeInKeyWindow, .inVisibleRect, .cursorUpdate], owner: self)
        addTrackingArea(t)
        trackingArea = t
    }

    override func cursorUpdate(with event: NSEvent) {
        (programWantsMouse ? NSCursor.arrow : NSCursor.iBeam).set()
    }

    private func updateCursorRects() {
        window?.invalidateCursorRects(for: self)
    }

    override func resetCursorRects() {
        addCursorRect(bounds, cursor: programWantsMouse ? .arrow : .iBeam)
    }

    private func mods(_ e: NSEvent) -> String {
        let f = e.modifierFlags
        var parts: [String] = []
        if f.contains(.shift) { parts.append("shift") }
        if f.contains(.control) { parts.append("ctrl") }
        if f.contains(.option) { parts.append("alt") }
        if f.contains(.command) { parts.append("super") }
        return parts.joined(separator: "+")
    }

    private func sendMouse(_ action: String, button: String?, _ e: NSEvent) {
        let p = convert(e.locationInWindow, from: nil)
        let (c, r) = cell(at: p)
        var payload: [String: Any] = ["action": action, "x": c, "y": r, "mods": mods(e)]
        if let button { payload["button"] = button }
        payload["buttons"] = Dictionary(uniqueKeysWithValues: mouseButtonsDown.map { ($0, true) })
        guard let data = try? JSONSerialization.data(withJSONObject: payload), let json = String(data: data, encoding: .utf8) else { return }
        terminal?.sendEvent(kind: "mouse", payloadJson: json)
        lastMotionCell = (c, r)
    }

    /// Mouse goes to the program when it asks for it, unless Shift is held (local selection).
    private func reportsMouse(_ e: NSEvent) -> Bool { programWantsMouse && !e.modifierFlags.contains(.shift) }

    override func mouseDown(with e: NSEvent) {
        window?.makeFirstResponder(self)
        onActivate?()
        if reportsMouse(e) {
            mouseButtonsDown.insert("left")
            sendMouse("press", button: "left", e)
            return
        }
        let p = convert(e.locationInWindow, from: nil)
        let (c, r) = cell(at: p)
        dragging = true
        lastDragPoint = p
        if e.modifierFlags.contains(.shift), let a = anchor {
            _ = terminal?.selectRange(fromCol: UInt16(a.col), fromRow: UInt16(a.row), toCol: UInt16(c), toRow: UInt16(r))
            return
        }
        selectionMode = min(e.clickCount, 3)
        anchor = (c, r)
        switch selectionMode {
        case 2: _ = terminal?.selectWord(col: UInt16(c), row: UInt16(r))
        case 3: _ = terminal?.selectLine(row: UInt16(r))
        default: terminal?.clearSelection()
        }
    }

    override func mouseDragged(with e: NSEvent) {
        if reportsMouse(e), mouseButtonsDown.contains("left") {
            let (c, r) = cell(at: convert(e.locationInWindow, from: nil))
            if lastMotionCell.map({ $0 != (c, r) }) ?? true { sendMouse("motion", button: nil, e) }
            return
        }
        guard dragging else { return }
        lastDragPoint = convert(e.locationInWindow, from: nil)
        extendSelection(to: lastDragPoint)
        let outside = lastDragPoint.y < 0 || lastDragPoint.y > bounds.height
        if outside, autoscrollTimer == nil {
            let t = Timer(timeInterval: 0.06, repeats: true) { [weak self] _ in
                MainActor.assumeIsolated { self?.autoscrollStep() }
            }
            RunLoop.main.add(t, forMode: .common)
            autoscrollTimer = t
        } else if !outside {
            autoscrollTimer?.invalidate()
            autoscrollTimer = nil
        }
    }

    private func autoscrollStep() {
        guard dragging else { return }
        if lastDragPoint.y < 0 {
            terminal?.scroll(rows: -1)
            anchor.map { anchor = ($0.col, $0.row + 1) }
        } else if lastDragPoint.y > bounds.height {
            terminal?.scroll(rows: 1)
            anchor.map { anchor = ($0.col, $0.row - 1) }
        }
        extendSelection(to: lastDragPoint)
    }

    private func extendSelection(to p: NSPoint) {
        guard let a = anchor, let f = frameCache else { return }
        let (c, r) = cell(at: p)
        let ar = min(max(a.row, 0), Int(f.rows) - 1)
        switch selectionMode {
        case 3:
            let (top, bottom) = r < ar ? (r, ar) : (ar, r)
            _ = terminal?.selectRange(fromCol: 0, fromRow: UInt16(top), toCol: f.cols - 1, toRow: UInt16(bottom))
        default:
            if (c, r) == (a.col, ar), selectionMode == 1 { return }
            _ = terminal?.selectRange(fromCol: UInt16(a.col), fromRow: UInt16(ar), toCol: UInt16(c), toRow: UInt16(r))
        }
    }

    override func mouseUp(with e: NSEvent) {
        autoscrollTimer?.invalidate()
        autoscrollTimer = nil
        if mouseButtonsDown.remove("left") != nil {
            sendMouse("release", button: "left", e)
            return
        }
        dragging = false
    }

    override func mouseMoved(with e: NSEvent) {
        guard programWantsMouse, mouseButtonsDown.isEmpty else { return }
        let (c, r) = cell(at: convert(e.locationInWindow, from: nil))
        if lastMotionCell.map({ $0 != (c, r) }) ?? true { sendMouse("motion", button: nil, e) }
    }

    override func rightMouseDown(with e: NSEvent) {
        if reportsMouse(e), !e.modifierFlags.contains(.control) {
            mouseButtonsDown.insert("right")
            sendMouse("press", button: "right", e)
            return
        }
        super.rightMouseDown(with: e)
    }

    override func rightMouseUp(with e: NSEvent) {
        if mouseButtonsDown.remove("right") != nil { sendMouse("release", button: "right", e) }
    }

    override func otherMouseDown(with e: NSEvent) {
        guard e.buttonNumber == 2 else { return }
        if reportsMouse(e) {
            mouseButtonsDown.insert("middle")
            sendMouse("press", button: "middle", e)
        } else {
            paste(nil)
        }
    }

    override func otherMouseUp(with e: NSEvent) {
        if mouseButtonsDown.remove("middle") != nil { sendMouse("release", button: "middle", e) }
    }

    // MARK: scrolling

    override func scrollWheel(with e: NSEvent) {
        guard let f = frameCache else { return }
        let cellH = renderer.metrics.height * scale
        let lines: CGFloat
        if e.hasPreciseScrollingDeltas {
            if e.phase == .began { scrollAccumulator = 0 }
            scrollAccumulator += e.scrollingDeltaY
            let whole = (scrollAccumulator / cellH).rounded(.towardZero)
            scrollAccumulator -= whole * cellH
            lines = whole
        } else {
            lines = e.scrollingDeltaY.rounded(.awayFromZero) * 3
        }
        guard lines != 0 else { return }
        if f.alternateScreen || programWantsMouse {
            let (c, r) = cell(at: convert(e.locationInWindow, from: nil))
            terminal?.wheel(col: UInt16(c), row: UInt16(r), lines: Float(-lines))
        } else {
            terminal?.scroll(rows: Int32(-lines))
            scroller.flash()
        }
    }

    private func page(_ dir: Int) {
        guard let f = frameCache else { return }
        terminal?.scroll(rows: Int32(dir * max(1, Int(f.rows) - 2)))
        scroller.flash()
    }

    private func scrollTo(fraction: Double) {
        guard let f = frameCache, f.scrollTotal > f.scrollLen else { return }
        let target = Int64((Double(f.scrollTotal - f.scrollLen) * fraction).rounded())
        terminal?.scroll(rows: Int32(target - Int64(f.scrollOffset)))
        // Several drag events can arrive before the next frame; keep the offset current.
        refresh()
    }

    override func magnify(with e: NSEvent) {
        onFontSize?(min(max(renderer.fontSize * (1 + e.magnification), 8), 32))
    }
}

/// What a pane's context menu and header buttons can ask for.
enum PaneAction: String {
    case splitRight, splitDown, zoom, close, takeSize

    var title: String {
        switch self {
        case .splitRight: return "Split Right"
        case .splitDown: return "Split Down"
        case .zoom: return "Zoom Pane"
        case .close: return "Close Pane"
        case .takeSize: return "Fit to This Window"
        }
    }

    var symbol: String {
        switch self {
        case .splitRight: return "rectangle.split.2x1"
        case .splitDown: return "rectangle.split.1x2"
        case .zoom: return "arrow.up.left.and.arrow.down.right"
        case .close: return "xmark"
        case .takeSize: return "arrow.up.left.and.down.right.magnifyingglass"
        }
    }
}

/// Settings that change how a terminal looks and reads keys.
struct TerminalPrefs: Equatable {
    enum OptionKey: String, CaseIterable { case off, left, right, both }
    var fontId = "jetbrains-mono"
    var fontSize: CGFloat = 13
    var lineHeight: CGFloat = 1.12
    var cursorShape: CursorShape?
    var blink = true
    var optionAsMeta = OptionKey.off
}

func cgColor(_ argb: UInt32) -> CGColor {
    CGColor(srgbRed: CGFloat((argb >> 16) & 0xFF) / 255, green: CGFloat((argb >> 8) & 0xFF) / 255, blue: CGFloat(argb & 0xFF) / 255, alpha: 1)
}

extension TerminalStatus {
    var isTerminalEnded: Bool { if case .ended = self { return true } else { return false } }
    var isExited: Bool { if case .exited = self { return true } else { return false } }
}

/// An overlay scroller for history: a slim knob that shows while scrolling or
/// hovered, fades when idle, and can be dragged or clicked to page.
final class HistoryScroller: NSView {
    var onScroll: ((Double) -> Void)?
    var color = NSColor.white { didSet { needsDisplay = true } }
    private var total: UInt64 = 0, offset: UInt64 = 0, len: UInt64 = 0
    private var hideWork: DispatchWorkItem?
    private var hovering = false { didSet { needsDisplay = true } }
    private var grab: CGFloat?

    override var isFlipped: Bool { true }
    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }

    init() {
        super.init(frame: .zero)
        alphaValue = 0
        addTrackingArea(NSTrackingArea(rect: .zero, options: [.mouseEnteredAndExited, .activeAlways, .inVisibleRect], owner: self))
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError() }

    private var hasHistory: Bool { total > len }

    func update(total: UInt64, offset: UInt64, len: UInt64) {
        guard (total, offset, len) != (self.total, self.offset, self.len) else { return }
        let moved = offset != self.offset && self.total > 0 && offset + len < total
        (self.total, self.offset, self.len) = (total, offset, len)
        needsDisplay = true
        if !hasHistory { alphaValue = 0 } else if moved { flash() }
    }

    private var knob: NSRect {
        guard hasHistory else { return .zero }
        let track = bounds.insetBy(dx: 0, dy: 3)
        let h = max(28, track.height * CGFloat(len) / CGFloat(total))
        let y = track.minY + (track.height - h) * CGFloat(offset) / CGFloat(total - len)
        let w: CGFloat = hovering || grab != nil ? 8 : 6
        return NSRect(x: bounds.maxX - w - 3, y: y, width: w, height: h)
    }

    override func draw(_ dirtyRect: NSRect) {
        guard hasHistory else { return }
        if hovering || grab != nil {
            color.withAlphaComponent(0.06).setFill()
            NSRect(x: bounds.maxX - 14, y: 0, width: 14, height: bounds.height).fill()
        }
        color.withAlphaComponent(hovering || grab != nil ? 0.55 : 0.4).setFill()
        NSBezierPath(roundedRect: knob, xRadius: knob.width / 2, yRadius: knob.width / 2).fill()
    }

    func flash() {
        guard hasHistory else { return }
        if alphaValue < 1 { alphaValue = 1 }
        hideWork?.cancel()
        let w = DispatchWorkItem { [weak self] in
            guard let self, !self.hovering, self.grab == nil else { return }
            NSAnimationContext.runAnimationGroup { $0.duration = 0.3; self.animator().alphaValue = 0 }
        }
        hideWork = w
        DispatchQueue.main.asyncAfter(deadline: .now() + 1.2, execute: w)
    }

    override func mouseEntered(with event: NSEvent) { hovering = true; flash() }
    override func mouseExited(with event: NSEvent) { hovering = false; flash() }

    override func mouseDown(with e: NSEvent) {
        let p = convert(e.locationInWindow, from: nil)
        let k = knob
        if k.insetBy(dx: -4, dy: 0).contains(p) {
            grab = p.y - k.minY
        } else {
            let page = Double(len) / Double(max(1, total - len))
            let cur = Double(offset) / Double(max(1, total - len))
            onScroll?(min(1, max(0, p.y < k.minY ? cur - page : cur + page)))
        }
        needsDisplay = true
    }

    override func mouseDragged(with e: NSEvent) {
        guard let grab, hasHistory else { return }
        let p = convert(e.locationInWindow, from: nil)
        let track = bounds.insetBy(dx: 0, dy: 3)
        let free = track.height - knob.height
        guard free > 0 else { return }
        onScroll?(Double(min(max((p.y - grab - track.minY) / free, 0), 1)))
    }

    override func mouseUp(with e: NSEvent) {
        grab = nil
        needsDisplay = true
        flash()
    }

    override func hitTest(_ point: NSPoint) -> NSView? {
        guard hasHistory, alphaValue > 0.1 || hovering else { return nil }
        let local = convert(point, from: superview)
        return local.x > bounds.maxX - 14 ? self : nil
    }
}
