#if os(iOS)
import GameController
import SwiftUI
import UIKit

/// The iOS terminal surface: draws frames with Core Text and turns touches,
/// the software keyboard, and hardware keyboards into terminal input.
public final class TerminalUIView: UIView, UITextInputTraits, UIGestureRecognizerDelegate, UIEditMenuInteractionDelegate, UIPointerInteractionDelegate {
    public let controller: TerminalController
    public let renderer: TerminalRenderer
    public var onFontSizeChanged: ((CGFloat) -> Void)?
    /// Horizontal swipe: +1 next pane, -1 previous.
    public var onSwipePane: ((Int) -> Void)?
    /// Took the keyboard (a tap, a shortcut): the pane the user is working in.
    public var onFocus: (() -> Void)?

    private var frameCache: Frame?
    private let padding: CGFloat = 6
    private var zoom: CGFloat = 1
    private var panX: CGFloat = 0
    private var scrollRemainder: CGFloat = 0
    private var velocity: CGFloat = 0
    private var link: CADisplayLink?
    private var blinkOn = true
    private var blinkTimer: Timer?
    private var selecting = false
    private var anchor: (Int, Int)?
    private var editMenu: UIEditMenuInteraction?
    private let keyBar: KeyBar
    let modifiers = StickyModifiers()
    var kb = KeyboardDocument()
    private var repeatTimer: Timer?
    private var repeatKey: UIKeyboardHIDUsage?
    public weak var inputDelegate: UITextInputDelegate?
    public lazy var tokenizer: UITextInputTokenizer = UITextInputStringTokenizer(textInput: self)
    public var markedTextStyle: [NSAttributedString.Key: Any]?

    /// Settings: forced cursor shape (nil follows the program) and blinking.
    public var cursorShape: CursorShape?
    public var cursorBlinks = true

    /// Apply font, spacing and cursor from Settings to this open terminal.
    public func applyAppearance(fontId: String?, lineHeight: CGFloat, cursor: String?, blink: Bool) {
        renderer.setAppearance(fontId: fontId, lineHeight: lineHeight)
        cursorShape = switch cursor { case "block": .block; case "bar": .bar; case "underline": .underline; default: nil }
        cursorBlinks = blink
        pushGrid()
        setNeedsDisplay()
    }

    public init(controller: TerminalController, fontSize: CGFloat) {
        self.controller = controller
        self.renderer = TerminalRenderer(fontSize: fontSize)
        self.keyBar = KeyBar(frame: CGRect(x: 0, y: 0, width: 320, height: 48))
        super.init(frame: .zero)
        isOpaque = true
        backgroundColor = UIColor(red: 0.05, green: 0.07, blue: 0.09, alpha: 1)
        contentMode = .redraw
        isMultipleTouchEnabled = true
        keyBar.terminal = self
        controller.onFrame = { [weak self] in self?.refresh() }
        controller.onClipboard = { UIPasteboard.general.string = $0 }
        controller.onFontStep = { [weak self] in self?.stepFont($0) }

        let tap = UITapGestureRecognizer(target: self, action: #selector(onTap(_:)))
        let doubleTap = UITapGestureRecognizer(target: self, action: #selector(onDoubleTap(_:)))
        doubleTap.numberOfTapsRequired = 2
        tap.require(toFail: doubleTap)
        let pan = UIPanGestureRecognizer(target: self, action: #selector(onPan(_:)))
        pan.delegate = self
        pan.allowedScrollTypesMask = .all // trackpads and mouse wheels scroll too
        let rightClick = UITapGestureRecognizer(target: self, action: #selector(onRightClick(_:)))
        rightClick.buttonMaskRequired = .secondary
        rightClick.delegate = self
        let pinch = UIPinchGestureRecognizer(target: self, action: #selector(onPinch(_:)))
        let press = UILongPressGestureRecognizer(target: self, action: #selector(onLongPress(_:)))
        press.minimumPressDuration = 0.35
        [tap, doubleTap, pan, pinch, press, rightClick].forEach(addGestureRecognizer)
        addInteraction(UIPointerInteraction(delegate: self))

        let menu = UIEditMenuInteraction(delegate: self)
        addInteraction(menu)
        editMenu = menu

        isAccessibilityElement = true
        accessibilityTraits = .allowsDirectInteraction
        accessibilityLabel = "Terminal"
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError() }

    deinit {
        blinkTimer?.invalidate()
    }

    public override func didMoveToWindow() {
        super.didMoveToWindow()
        if window == nil {
            link?.invalidate()
            link = nil
            stopRepeat()
        } else if GCKeyboard.coalesced != nil, !Self.simulator {
            // With a hardware keyboard there's no on-screen keyboard to cover the
            // screen, so be ready to type (and for ⌘ shortcuts) right away.
            DispatchQueue.main.async { [weak self] in _ = self?.becomeFirstResponder() }
        }
    }

    /// The simulator always reports the Mac's keyboard, even while showing the on-screen one.
    #if targetEnvironment(simulator)
    private static let simulator = true
    #else
    private static let simulator = false
    #endif

    // MARK: drawing

    /// Space covered by floating chrome; the grid is sized to what's left.
    public var insetTop: CGFloat = 0 { didSet { if insetTop != oldValue { pushGrid(); setNeedsDisplay() } } }
    public var insetBottom: CGFloat = 0 { didSet { if insetBottom != oldValue { pushGrid(); setNeedsDisplay() } } }
    /// The background runs under the home indicator; the rows stop above it.
    private var bottomInset: CGFloat { insetBottom + safeAreaInsets.bottom }

    public override func safeAreaInsetsDidChange() {
        super.safeAreaInsetsDidChange()
        pushGrid()
        setNeedsDisplay()
    }
    /// Called when the terminal's default colors change, so chrome can match.
    public var onColors: ((UInt32, UInt32) -> Void)?
    /// Chrome colors for the key bar and keyboard appearance.
    public var chrome = Chrome(Themes.all[0]) {
        didSet {
            guard chrome != oldValue else { return }
            keyBar.chrome = chrome
            keyboardAppearance = chrome.light ? .light : .dark
        }
    }

    func refresh() {
        let prev = frameCache
        frameCache = controller.frame()
        if let f = frameCache, prev?.background != f.background || prev?.foreground != f.foreground {
            backgroundColor = UIColor(cgColor: cg(f.background))
            onColors?(f.background, f.foreground)
        }
        blinkOn = true
        invalidate(from: prev, to: frameCache)
        if UIAccessibility.isVoiceOverRunning, let f = frameCache {
            accessibilityValue = f.lines.suffix(3).map { $0.runs.map(\.text).joined() }.joined(separator: "\n")
        }
    }

    /// Redraw only the rows that changed (and the cursor's old and new rows);
    /// anything that moves the whole grid redraws everything.
    private func invalidate(from p: Frame?, to f: Frame?) {
        guard let p, let f, p.cols == f.cols, p.rows == f.rows, p.lines.count == f.lines.count,
              p.background == f.background, p.foreground == f.foreground, p.alternateScreen == f.alternateScreen,
              p.scrollOffset == f.scrollOffset, p.scrollTotal == f.scrollTotal, offsetY() == drawnOffsetY
        else { return setNeedsDisplay() }
        var dirty = CGRect.null
        for i in f.lines.indices where p.lines[i] != f.lines[i] { dirty = dirty.union(rowRect(i)) }
        if p.cursor != f.cursor || p.cursorColor != f.cursorColor {
            for c in [p.cursor, f.cursor].compactMap({ $0 }) { dirty = dirty.union(rowRect(Int(c.row))) }
        }
        if !dirty.isNull { setNeedsDisplay(dirty) }
    }

    private func rowRect(_ row: Int) -> CGRect {
        let h = renderer.metrics.height * scale
        return CGRect(x: 0, y: insetTop - offsetY() + padding + CGFloat(row) * h - 2, width: bounds.width, height: h + 4)
    }

    private var drawnOffsetY: CGFloat = 0

    /// The blink timer only runs while the terminal has the keyboard.
    private func blink(_ on: Bool) {
        blinkTimer?.invalidate()
        blinkTimer = nil
        blinkOn = true
        guard on else { return }
        let t = Timer(timeInterval: 0.53, repeats: true) { [weak self] _ in
            MainActor.assumeIsolated {
                guard let self, self.cursorBlinks, self.frameCache?.cursor?.blinking == true else { return }
                self.blinkOn.toggle()
                self.setNeedsDisplay(self.cursorRect().insetBy(dx: -3, dy: -3))
            }
        }
        t.tolerance = 0.05
        RunLoop.main.add(t, forMode: .common)
        blinkTimer = t
    }

    private var scale: CGFloat {
        guard let f = frameCache, !controller.hasControl else { return zoom }
        let need = CGFloat(f.cols) * renderer.metrics.width + padding * 2
        return min(1, bounds.width / need) * zoom
    }

    public override func draw(_ rect: CGRect) {
        guard let ctx = UIGraphicsGetCurrentContext() else { return }
        guard let f = frameCache else {
            ctx.setFillColor(backgroundColor!.cgColor)
            ctx.fill(bounds)
            return
        }
        ctx.setFillColor(cg(f.background))
        ctx.fill(bounds)
        // Keep the cursor row visible when the grid is taller than the space
        // between the chrome (e.g. the keyboard just opened).
        drawnOffsetY = offsetY()
        ctx.translateBy(x: 0, y: insetTop - drawnOffsetY)
        var o = TerminalRenderer.Options()
        o.padding = padding
        o.scale = scale
        o.panX = panX
        o.focused = isFirstResponder
        o.cursorVisible = blinkOn || !isFirstResponder || !cursorBlinks
        o.cursorShape = cursorShape
        o.preedit = kb.marked
        renderer.draw(f, in: ctx, size: bounds.size, options: o)
    }

    private func offsetY() -> CGFloat {
        guard let f = frameCache else { return 0 }
        let visible = bounds.height - insetTop - bottomInset
        let content = CGFloat(f.rows) * renderer.metrics.height * scale + padding * 2
        guard content > visible else { return 0 }
        let anchor = CGFloat((f.cursor.map { Int($0.row) } ?? Int(f.rows) - 1) + 1)
        let need = anchor * renderer.metrics.height * scale + padding * 2
        return min(max(0, need - visible), content - visible)
    }

    public override func layoutSubviews() {
        super.layoutSubviews()
        pushGrid()
    }

    private func pushGrid() {
        guard bounds.width > 0, let t = controller.terminal else { return }
        let g = renderer.grid(for: CGSize(width: bounds.width, height: max(40, bounds.height - insetTop - bottomInset)), padding: padding)
        let s = window?.screen.scale ?? 3
        t.setDeviceGrid(cols: UInt16(g.cols), rows: UInt16(g.rows),
                        cellWidthPx: UInt32((renderer.metrics.width * s).rounded()), cellHeightPx: UInt32((renderer.metrics.height * s).rounded()))
    }

    /// The cursor cell in view coordinates.
    func cursorRect() -> CGRect {
        let s = scale, m = renderer.metrics
        let c = frameCache?.cursor
        let col = CGFloat(c?.col ?? 0), row = CGFloat(c?.row ?? 0)
        return CGRect(x: padding - panX + col * m.width * s, y: insetTop - offsetY() + padding + row * m.height * s,
                      width: m.width * s, height: m.height * s)
    }

    private func cell(at p: CGPoint) -> (Int, Int) {
        let s = scale
        let col = Int((p.x - padding + panX) / s / renderer.metrics.width)
        let row = Int((p.y - insetTop + offsetY() - padding) / s / renderer.metrics.height)
        let f = frameCache
        return (min(max(col, 0), Int(f?.cols ?? 1) - 1), min(max(row, 0), Int(f?.rows ?? 1) - 1))
    }

    // MARK: gestures

    @objc private func onTap(_ g: UITapGestureRecognizer) {
        resetKeyboardDocument()
        if selecting || controller.terminal?.selectionText() != nil {
            controller.terminal?.clearSelection()
            selecting = false
            return
        }
        if controller.terminal?.mouseTracking() == true {
            let (c, r) = cell(at: g.location(in: self))
            controller.terminal?.tap(col: UInt16(c), row: UInt16(r))
        }
        if !isFirstResponder { _ = becomeFirstResponder() }
    }

    @objc private func onDoubleTap(_ g: UITapGestureRecognizer) {
        let p = g.location(in: self)
        let (c, r) = cell(at: p)
        if controller.terminal?.selectWord(col: UInt16(c), row: UInt16(r)) == true {
            Haptics.select()
            anchor = (c, r)
            showMenu(at: p)
        }
    }

    @objc private func onLongPress(_ g: UILongPressGestureRecognizer) {
        let p = g.location(in: self)
        let (c, r) = cell(at: p)
        switch g.state {
        case .began:
            // A word under the finger, or the whole line when pressing blank space.
            let t = controller.terminal
            let word = t?.selectWord(col: UInt16(c), row: UInt16(r)) == true && t?.selectionText() != nil
            if word || t?.selectLine(row: UInt16(r)) == true {
                UIImpactFeedbackGenerator(style: .medium).impactOccurred()
                selecting = true
                anchor = (c, r)
            }
        case .changed:
            if let a = anchor {
                _ = controller.terminal?.selectRange(fromCol: UInt16(a.0), fromRow: UInt16(a.1), toCol: UInt16(c), toRow: UInt16(r))
            }
        case .ended:
            showMenu(at: p)
        default: break
        }
    }

    private func showMenu(at p: CGPoint) {
        editMenu?.presentEditMenu(with: UIEditMenuConfiguration(identifier: nil, sourcePoint: p))
    }

    public func editMenuInteraction(_ interaction: UIEditMenuInteraction, menuFor configuration: UIEditMenuConfiguration, suggestedActions: [UIMenuElement]) -> UIMenu? {
        let t = controller.terminal
        var items: [UIMenuElement] = []
        if let text = t?.selectionText() {
            items.append(UIAction(title: "Copy", image: UIImage(systemName: "doc.on.doc")) { _ in
                UIPasteboard.general.string = text
                t?.clearSelection()
            })
        }
        if UIPasteboard.general.hasStrings {
            items.append(UIAction(title: "Paste", image: UIImage(systemName: "doc.on.clipboard")) { [weak self] _ in self?.paste(nil) })
        }
        items.append(UIAction(title: "Copy All", image: UIImage(systemName: "text.page")) { _ in
            UIPasteboard.general.string = t?.allText()
            t?.clearSelection()
        })
        return UIMenu(children: items)
    }

    public func editMenuInteraction(_ interaction: UIEditMenuInteraction, willDismissMenuFor configuration: UIEditMenuConfiguration, animator: any UIEditMenuInteractionAnimating) {
        selecting = false
    }

    @objc private func onRightClick(_ g: UITapGestureRecognizer) {
        showMenu(at: g.location(in: self))
    }

    public func pointerInteraction(_ interaction: UIPointerInteraction, styleFor region: UIPointerRegion) -> UIPointerStyle? {
        UIPointerStyle(shape: .verticalBeam(length: renderer.metrics.height * scale), constrainedAxes: [])
    }

    private enum PanAxis { case undecided, vertical, horizontal }
    private var panAxis = PanAxis.undecided
    private var panTotalX: CGFloat = 0
    /// The pan started with a mouse or trackpad click: drag selects, like a desktop.
    private var pointerTouch = false
    private var dragSelecting = false

    public func gestureRecognizer(_ g: UIGestureRecognizer, shouldReceive touch: UITouch) -> Bool {
        if g is UIPanGestureRecognizer { pointerTouch = touch.type == .indirectPointer }
        // A finger tap can satisfy a secondary-button mask; only a real click should.
        if let t = g as? UITapGestureRecognizer, t.buttonMaskRequired == .secondary { return touch.type == .indirectPointer }
        return true
    }

    private func dragSelect(_ g: UIPanGestureRecognizer, moved d: CGPoint) {
        let p = g.location(in: self)
        switch g.state {
        case .began:
            let (c, r) = cell(at: CGPoint(x: p.x - d.x, y: p.y - d.y))
            anchor = (c, r)
        case .changed:
            let (c, r) = cell(at: p)
            if let a = anchor { _ = controller.terminal?.selectRange(fromCol: UInt16(a.0), fromRow: UInt16(a.1), toCol: UInt16(c), toRow: UInt16(r)) }
        default:
            dragSelecting = false
        }
    }

    @objc private func onPan(_ g: UIPanGestureRecognizer) {
        if selecting { return }
        let d = g.translation(in: self)
        g.setTranslation(.zero, in: self)
        if g.state == .began { dragSelecting = pointerTouch && g.numberOfTouches > 0 && controller.terminal?.mouseTracking() != true }
        if dragSelecting { return dragSelect(g, moved: d) }
        let canPan = (!controller.hasControl && scale < 1) || zoom > 1
        switch g.state {
        case .began:
            link?.invalidate()
            link = nil
            panAxis = .undecided
            panTotalX = 0
        case .changed:
            if panAxis == .undecided, abs(d.x) + abs(d.y) > 2 {
                panAxis = abs(d.x) > abs(d.y) * 1.3 ? .horizontal : .vertical
            }
            switch panAxis {
            case .horizontal where canPan: panBy(-d.x)
            case .horizontal: panTotalX += d.x // a pane swipe, decided on release
            default: scroll(by: -d.y)
            }
        case .ended, .cancelled:
            if panAxis == .horizontal, !canPan {
                let vx = g.velocity(in: self).x
                if abs(panTotalX) > 60 || abs(vx) > 600 {
                    // Swipe left for the next pane, right for the previous one.
                    onSwipePane?((panTotalX + vx * 0.1) < 0 ? 1 : -1)
                }
            } else if panAxis != .horizontal {
                velocity = -g.velocity(in: self).y
                if abs(velocity) > 200 { startMomentum() }
            }
            panAxis = .undecided
        default: break
        }
    }

    private func panBy(_ dx: CGFloat) {
        guard let f = frameCache else { return }
        let content = CGFloat(f.cols) * renderer.metrics.width * scale + padding * 2
        panX = min(max(0, panX + dx), max(0, content - bounds.width))
        setNeedsDisplay()
    }

    private func scroll(by dy: CGFloat) {
        guard let t = controller.terminal else { return }
        scrollRemainder += dy / (renderer.metrics.height * scale)
        let rows = Int(scrollRemainder)
        guard rows != 0 else { return }
        scrollRemainder -= CGFloat(rows)
        if t.wantsWheel() || frameCache?.alternateScreen == true {
            let (c, r) = cell(at: CGPoint(x: bounds.midX, y: bounds.midY))
            t.wheel(col: UInt16(c), row: UInt16(r), lines: Float(rows))
        } else {
            t.scroll(rows: Int32(rows))
        }
    }

    private func startMomentum() {
        link?.invalidate()
        let l = CADisplayLink(target: self, selector: #selector(stepMomentum(_:)))
        l.preferredFrameRateRange = CAFrameRateRange(minimum: 60, maximum: 120, preferred: 120)
        l.add(to: .main, forMode: .common)
        link = l
    }

    @objc private func stepMomentum(_ l: CADisplayLink) {
        let dt = CGFloat(l.targetTimestamp - l.timestamp)
        scroll(by: velocity * dt)
        velocity *= pow(0.135, dt) // ~UIScrollView's normal deceleration
        if abs(velocity) < 30 { l.invalidate(); link = nil }
    }

    @objc private func onPinch(_ g: UIPinchGestureRecognizer) {
        if controller.hasControl {
            renderer.setFontSize(renderer.fontSize * g.scale)
            g.scale = 1
            pushGrid()
            setNeedsDisplay()
            if g.state == .ended { onFontSizeChanged?(renderer.fontSize) }
        } else {
            zoom = min(max(1, zoom * g.scale), 4)
            g.scale = 1
            panBy(0)
            setNeedsDisplay()
        }
    }

    public func gestureRecognizer(_ g: UIGestureRecognizer, shouldRecognizeSimultaneouslyWith other: UIGestureRecognizer) -> Bool {
        g is UIPinchGestureRecognizer || other is UIPinchGestureRecognizer
    }

    // MARK: keyboard

    public override var canBecomeFirstResponder: Bool { true }
    /// The key bar rides on the keyboard unless turned off in Settings.
    public var showsKeyBar = true
    public override var inputAccessoryView: UIView? { showsKeyBar ? keyBar : nil }

    public override func becomeFirstResponder() -> Bool {
        let r = super.becomeFirstResponder()
        if r { onFocus?() }
        blink(r)
        setNeedsDisplay()
        return r
    }

    public override func resignFirstResponder() -> Bool {
        resetKeyboardDocument()
        let r = super.resignFirstResponder()
        blink(false)
        setNeedsDisplay()
        return r
    }

    public var hasText: Bool { true }
    public var autocorrectionType: UITextAutocorrectionType = .no
    public var autocapitalizationType: UITextAutocapitalizationType = .none
    public var spellCheckingType: UITextSpellCheckingType = .no
    public var smartQuotesType: UITextSmartQuotesType = .no
    public var smartDashesType: UITextSmartDashesType = .no
    public var smartInsertDeleteType: UITextSmartInsertDeleteType = .no
    public var keyboardAppearance: UIKeyboardAppearance = .dark
    public var keyboardType: UIKeyboardType = .default
    public var returnKeyType: UIReturnKeyType = .default
    public var inlinePredictionType: UITextInlinePredictionType = .no

    public func insertText(_ text: String) {
        // An IME's chosen candidate replaces its composition.
        kb.marked = ""
        commit(kb.committed + text)
    }

    public func deleteBackward() {
        if !kb.committed.isEmpty { kb.committed.removeLast() }
        sendKey(.backspace)
    }

    /// Typed text to the terminal: newlines become Enter, and a sticky ctrl or
    /// alt from the key bar applies to the first character.
    func type(_ text: String) {
        guard let t = controller.terminal else { return }
        for (i, part) in text.split(separator: "\n", omittingEmptySubsequences: false).enumerated() {
            if i > 0 {
                sendKey(.enter)
                DispatchQueue.main.async { [weak self] in self?.resetKeyboardDocument() }
            }
            guard !part.isEmpty else { continue }
            let (ctrl, alt) = modifiers.consume()
            if ctrl || alt {
                sendKey(.char(c: String(part.prefix(1))), ctrl: ctrl, alt: alt)
                if part.count > 1 { t.sendText(text: String(part.dropFirst())) }
                DispatchQueue.main.async { [weak self] in self?.resetKeyboardDocument() }
            } else {
                t.sendText(text: String(part))
            }
            keyBar.refresh()
        }
    }

    /// A key from the key bar or a hardware keyboard: the keyboard's document
    /// no longer matches the line, so start a new one.
    func send(_ k: Key, ctrl: Bool = false, alt: Bool = false, shift: Bool = false, meta: Bool = false) {
        resetKeyboardDocument()
        sendKey(k, ctrl: ctrl, alt: alt, shift: shift, meta: meta)
    }

    private func sendKey(_ k: Key, ctrl: Bool = false, alt: Bool = false, shift: Bool = false, meta: Bool = false) {
        let (sc, sa) = (ctrl || alt) ? (false, false) : modifiers.consume()
        controller.terminal?.sendKey(key: k, ctrl: ctrl || sc, alt: alt || sa, shift: shift, meta: meta)
        keyBar.refresh()
    }

    func sendLiteral(_ s: String) {
        resetKeyboardDocument()
        let (c, a) = modifiers.consume()
        if c || a { controller.terminal?.sendKey(key: .char(c: s), ctrl: c, alt: a, shift: false, meta: false) } else { controller.terminal?.sendText(text: s) }
        keyBar.refresh()
    }

    public override func paste(_ sender: Any?) {
        resetKeyboardDocument()
        if let s = UIPasteboard.general.string { controller.terminal?.paste(text: s) }
    }

    public override func copy(_ sender: Any?) {
        if let s = controller.terminal?.selectionText() { UIPasteboard.general.string = s }
    }

    private func stepFont(_ d: Int) {
        renderer.setFontSize(d == 0 ? 13 : renderer.fontSize + CGFloat(d))
        pushGrid()
        setNeedsDisplay()
        onFontSizeChanged?(renderer.fontSize)
    }

    /// Hardware keyboards: arrows, Esc, function keys, and Ctrl/Alt chords. UIKit
    /// doesn't repeat presses it hands us, so held keys repeat here.
    public override func pressesBegan(_ presses: Set<UIPress>, with event: UIPressesEvent?) {
        // An input method is composing: arrows, Esc and Return belong to it.
        guard kb.marked.isEmpty else { return super.pressesBegan(presses, with: event) }
        var unhandled = Set<UIPress>()
        for press in presses {
            guard let key = press.key else { unhandled.insert(press); continue }
            let f = key.modifierFlags
            let ctrl = f.contains(.control), alt = f.contains(.alternate), shift = f.contains(.shift)
            if f.contains(.command) { unhandled.insert(press); continue }
            let k: Key
            if let named = Self.named(key.keyCode) {
                k = named
            } else if ctrl || alt, let ch = key.charactersIgnoringModifiers.first {
                k = .char(c: String(ch))
            } else {
                unhandled.insert(press)
                continue
            }
            send(k, ctrl: ctrl, alt: alt, shift: shift)
            startRepeat(key.keyCode) { [weak self] in self?.send(k, ctrl: ctrl, alt: alt, shift: shift) }
        }
        if !unhandled.isEmpty { super.pressesBegan(unhandled, with: event) }
    }

    public override func pressesEnded(_ presses: Set<UIPress>, with event: UIPressesEvent?) {
        if presses.contains(where: { $0.key?.keyCode == repeatKey }) { stopRepeat() }
        super.pressesEnded(presses, with: event)
    }

    public override func pressesCancelled(_ presses: Set<UIPress>, with event: UIPressesEvent?) {
        stopRepeat()
        super.pressesCancelled(presses, with: event)
    }

    private func startRepeat(_ code: UIKeyboardHIDUsage, _ action: @escaping () -> Void) {
        stopRepeat()
        repeatKey = code
        repeatTimer = Timer.scheduledTimer(withTimeInterval: 0.4, repeats: false) { [weak self] _ in
            MainActor.assumeIsolated {
                self?.repeatTimer = Timer.scheduledTimer(withTimeInterval: 1.0 / 30, repeats: true) { _ in
                    MainActor.assumeIsolated { action() }
                }
            }
        }
    }

    private func stopRepeat() {
        repeatTimer?.invalidate()
        repeatTimer = nil
        repeatKey = nil
    }

    private static func named(_ code: UIKeyboardHIDUsage) -> Key? {
        switch code {
        case .keyboardUpArrow: return .up
        case .keyboardDownArrow: return .down
        case .keyboardLeftArrow: return .left
        case .keyboardRightArrow: return .right
        case .keyboardEscape: return .escape
        case .keypadEnter: return .enter
        case .keyboardTab: return .tab
        case .keyboardHome: return .home
        case .keyboardEnd: return .end
        case .keyboardPageUp: return .pageUp
        case .keyboardPageDown: return .pageDown
        case .keyboardDeleteForward: return .delete
        case .keyboardInsert: return .insert
        case .keyboardF1: return .f(n: 1)
        case .keyboardF2: return .f(n: 2)
        case .keyboardF3: return .f(n: 3)
        case .keyboardF4: return .f(n: 4)
        case .keyboardF5: return .f(n: 5)
        case .keyboardF6: return .f(n: 6)
        case .keyboardF7: return .f(n: 7)
        case .keyboardF8: return .f(n: 8)
        case .keyboardF9: return .f(n: 9)
        case .keyboardF10: return .f(n: 10)
        case .keyboardF11: return .f(n: 11)
        case .keyboardF12: return .f(n: 12)
        default: return nil
        }
    }
}

/// Ctrl/Alt from the key bar: tap for the next key, tap again to lock.
final class StickyModifiers {
    var ctrl = false, alt = false, ctrlLocked = false, altLocked = false

    func consume() -> (Bool, Bool) {
        let r = (ctrl, alt)
        if !ctrlLocked { ctrl = false }
        if !altLocked { alt = false }
        return r
    }

    func cycle(ctrl isCtrl: Bool) {
        if isCtrl {
            if ctrlLocked { ctrlLocked = false; ctrl = false } else if ctrl { ctrlLocked = true } else { ctrl = true }
        } else {
            if altLocked { altLocked = false; alt = false } else if alt { altLocked = true } else { alt = true }
        }
    }
}

/// Terminal keys on a glass strip that rides on the software keyboard.
/// One row of equal-width keys, two pages, no scrolling; arrows repeat when held.
final class KeyBar: UIInputView {
    weak var terminal: TerminalUIView?
    private let blur = UIVisualEffectView(effect: UIBlurEffect(style: .systemUltraThinMaterial))
    private let tint = UIView()
    private let stack = UIStackView()
    private var page = 0
    private var ctrlButton: UIButton?
    private var altButton: UIButton?
    private var repeatTimer: Timer?
    var chrome = Chrome(Themes.all[0]) { didSet { restyle() } }

    private enum Item { case key(String, Key, repeats: Bool), text(String), ctrl, alt, more, hide }
    private let pages: [[Item]] = [
        [.key("esc", .escape, repeats: false), .key("tab", .tab, repeats: false), .ctrl, .alt,
         .key("←", .left, repeats: true), .key("↓", .down, repeats: true), .key("↑", .up, repeats: true), .key("→", .right, repeats: true), .more, .hide],
        [.text("|"), .text("~"), .text("/"), .text("-"), .text("`"),
         .key("home", .home, repeats: false), .key("end", .end, repeats: false), .key("pgup", .pageUp, repeats: true), .key("pgdn", .pageDown, repeats: true), .more, .hide],
    ]

    override init(frame: CGRect, inputViewStyle: UIInputView.Style = .default) {
        super.init(frame: CGRect(x: 0, y: 0, width: frame.width, height: 56), inputViewStyle: inputViewStyle)
        allowsSelfSizing = true
        backgroundColor = .clear
        let card = UIView()
        card.translatesAutoresizingMaskIntoConstraints = false
        card.layer.cornerRadius = 14
        card.layer.cornerCurve = .continuous
        card.clipsToBounds = true
        card.layer.borderWidth = 1
        addSubview(card)
        blur.translatesAutoresizingMaskIntoConstraints = false
        tint.translatesAutoresizingMaskIntoConstraints = false
        card.addSubview(blur)
        card.addSubview(tint)
        stack.axis = .horizontal
        stack.distribution = .fillEqually
        stack.spacing = 3
        stack.translatesAutoresizingMaskIntoConstraints = false
        card.addSubview(stack)
        NSLayoutConstraint.activate([
            card.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 6),
            card.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -6),
            card.topAnchor.constraint(equalTo: topAnchor, constant: 4),
            card.bottomAnchor.constraint(equalTo: bottomAnchor, constant: -6),
            card.heightAnchor.constraint(equalToConstant: 44),
            blur.leadingAnchor.constraint(equalTo: card.leadingAnchor), blur.trailingAnchor.constraint(equalTo: card.trailingAnchor),
            blur.topAnchor.constraint(equalTo: card.topAnchor), blur.bottomAnchor.constraint(equalTo: card.bottomAnchor),
            tint.leadingAnchor.constraint(equalTo: card.leadingAnchor), tint.trailingAnchor.constraint(equalTo: card.trailingAnchor),
            tint.topAnchor.constraint(equalTo: card.topAnchor), tint.bottomAnchor.constraint(equalTo: card.bottomAnchor),
            stack.leadingAnchor.constraint(equalTo: card.leadingAnchor, constant: 4),
            stack.trailingAnchor.constraint(equalTo: card.trailingAnchor, constant: -4),
            stack.topAnchor.constraint(equalTo: card.topAnchor, constant: 4),
            stack.bottomAnchor.constraint(equalTo: card.bottomAnchor, constant: -4),
        ])
        self.card = card
        rebuild()
    }

    private var card: UIView?

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError() }

    private func ui(_ c: Color) -> UIColor { UIColor(c) }

    private func restyle() {
        tint.backgroundColor = ui(chrome.glass)
        card?.layer.borderColor = ui(chrome.glassStroke).cgColor
        blur.effect = UIBlurEffect(style: chrome.light ? .systemUltraThinMaterialLight : .systemUltraThinMaterialDark)
        rebuild()
    }

    /// Wide keyboards (iPad, landscape) fit both pages in one row.
    private var wide = false

    override func layoutSubviews() {
        super.layoutSubviews()
        if (bounds.width >= 640) != wide {
            wide.toggle()
            rebuild()
        }
    }

    private func rebuild() {
        stack.arrangedSubviews.forEach { $0.removeFromSuperview() }
        ctrlButton = nil
        altButton = nil
        let items = wide ? pages[0].dropLast(2) + pages[1].dropLast(2) + [.hide] : pages[page]
        for item in items { stack.addArrangedSubview(button(for: item)) }
        refresh()
    }

    private func button(for item: Item) -> UIButton {
        var cfg = UIButton.Configuration.plain()
        cfg.baseForegroundColor = ui(chrome.text)
        cfg.background.cornerRadius = 9
        cfg.contentInsets = .zero
        let title: String
        switch item {
        case .key(let t, _, _): title = t
        case .text(let t): title = t
        case .ctrl: title = "ctrl"
        case .alt: title = "alt"
        case .more: title = page == 0 ? "•••" : "abc"
        case .hide: title = ""
        }
        var attr = AttributedString(title)
        attr.font = UIFont(name: "JetBrainsMono-Regular", size: title.count > 2 ? 13 : 16) ?? .monospacedSystemFont(ofSize: 14, weight: .regular)
        cfg.attributedTitle = attr
        if case .more = item { cfg.baseForegroundColor = ui(chrome.secondary) }
        let b = UIButton(configuration: cfg)
        b.configurationUpdateHandler = { [weak self] btn in
            guard let self else { return }
            if btn.isHighlighted { btn.configuration?.background.backgroundColor = self.ui(self.chrome.key) }
            else if btn !== self.ctrlButton && btn !== self.altButton { btn.configuration?.background.backgroundColor = .clear }
        }
        b.accessibilityLabel = title
        if case .hide = item {
            b.configuration?.image = UIImage(systemName: "keyboard.chevron.compact.down",
                                             withConfiguration: UIImage.SymbolConfiguration(pointSize: 15, weight: .medium))
            b.configuration?.baseForegroundColor = ui(chrome.secondary)
            b.accessibilityLabel = "Hide keyboard"
        }
        b.addAction(UIAction { [weak self] _ in self?.down(item) }, for: .touchDown)
        b.addAction(UIAction { [weak self] _ in self?.stopRepeat() }, for: [.touchUpInside, .touchUpOutside, .touchCancel, .touchDragExit])
        if case .ctrl = item { ctrlButton = b }
        if case .alt = item { altButton = b }
        return b
    }

    private func down(_ item: Item) {
        guard let t = terminal else { return }
        UIDevice.current.playInputClick()
        UIImpactFeedbackGenerator(style: .soft).impactOccurred(intensity: 0.6)
        switch item {
        case .key(_, let k, let repeats):
            t.send(k)
            if repeats {
                stopRepeat()
                repeatTimer = Timer.scheduledTimer(withTimeInterval: 0.38, repeats: false) { [weak self] _ in
                    MainActor.assumeIsolated {
                        self?.repeatTimer = Timer.scheduledTimer(withTimeInterval: 0.045, repeats: true) { [weak t] _ in
                            MainActor.assumeIsolated { t?.send(k) }
                        }
                    }
                }
            }
        case .text(let s): t.sendLiteral(s)
        case .ctrl: t.modifiers.cycle(ctrl: true); refresh()
        case .alt: t.modifiers.cycle(ctrl: false); refresh()
        case .more: page = 1 - page; rebuild()
        case .hide: _ = t.resignFirstResponder()
        }
    }

    private func stopRepeat() {
        repeatTimer?.invalidate()
        repeatTimer = nil
    }

    func refresh() {
        guard let m = terminal?.modifiers else { return }
        func style(_ b: UIButton?, on: Bool, locked: Bool) {
            b?.configuration?.background.backgroundColor = locked ? ui(chrome.accent) : on ? ui(chrome.accent).withAlphaComponent(0.3) : .clear
            b?.configuration?.baseForegroundColor = locked ? ui(chrome.bg) : ui(chrome.text)
        }
        style(ctrlButton, on: m.ctrl, locked: m.ctrlLocked)
        style(altButton, on: m.alt, locked: m.altLocked)
    }
}

extension KeyBar: UIInputViewAudioFeedback {
    var enableInputClicksWhenVisible: Bool { true }
}
#endif
