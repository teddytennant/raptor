import AppKit
import RaptorKit
import SwiftUI

/// The panes of the active Rex window, laid out from the server's layout.
struct PaneArea: NSViewRepresentable {
    let ws: Workspace
    let window: RexWindow
    let focusedId: String?
    let chrome: Chrome
    let version: Int

    func makeNSView(context: Context) -> PaneContainerView {
        PaneContainerView(ws: ws)
    }

    func updateNSView(_ v: PaneContainerView, context: Context) {
        v.update(window: window, focusedId: focusedId, chrome: chrome)
    }
}

final class PaneContainerView: NSView {
    let ws: Workspace
    private var panes: [String: PaneView] = [:]
    private var dividers: [DividerView] = []
    private var window_: RexWindow?
    private var focusedId: String?
    private var chrome = Chrome(Themes.all[0])
    /// A divider being dragged: its split and the ratio shown until the server agrees.
    private var dragging: (split: Split, ratio: Double)?

    init(ws: Workspace) {
        self.ws = ws
        super.init(frame: .zero)
        wantsLayer = true
        layer?.cornerRadius = 10
        layer?.cornerCurve = .continuous
        layer?.masksToBounds = true
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError() }

    override var isFlipped: Bool { true }

    func update(window w: RexWindow, focusedId: String?, chrome: Chrome) {
        let focusChanged = focusedId != self.focusedId
        window_ = w
        self.focusedId = focusedId
        self.chrome = chrome
        layer?.backgroundColor = cgColor(0xFF00_0000 | chrome.bgHex)
        if let d = dragging, !w.splits.contains(where: { $0.before == d.split.before && $0.after == d.split.after }) { dragging = nil }

        var seen = Set<String>()
        for b in w.blocks {
            guard let term = ws.terminal(for: b) else { continue }
            seen.insert(b.id)
            let pane = panes[b.id] ?? {
                let p = PaneView(terminal: term, ws: ws)
                panes[b.id] = p
                addSubview(p, positioned: .below, relativeTo: dividers.first)
                return p
            }()
            pane.update(block: b, focused: b.id == focusedId, multiple: w.blocks.count > 1, zoomed: w.zoomedBlockId == b.id, chrome: chrome)
        }
        for (id, p) in panes where !seen.contains(id) {
            p.removeFromSuperview()
            panes[id] = nil
        }
        syncDividers(w)
        needsLayout = true
        if focusChanged || window?.firstResponder === window || window?.firstResponder == nil { focusTerminal() }
    }

    /// Hand keyboard focus to the focused pane, unless a text field (palette, rename) has it.
    func focusTerminal() {
        guard let window, let id = focusedId, let t = panes[id]?.terminal else { return }
        if let r = window.firstResponder as? NSView, r is NSTextView || r is NSTextField, r.isDescendant(of: window.contentView!) { return }
        if window.firstResponder !== t { window.makeFirstResponder(t) }
    }

    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        DispatchQueue.main.async { self.focusTerminal() }
    }

    private func visibleBlocks(_ w: RexWindow) -> [Block] {
        if let z = w.zoomedBlockId, let b = w.blocks.first(where: { $0.id == z }) { return [b] }
        return w.blocks
    }

    private func syncDividers(_ w: RexWindow) {
        let splits = w.zoomedBlockId == nil ? w.splits : []
        while dividers.count > splits.count { dividers.removeLast().removeFromSuperview() }
        while dividers.count < splits.count {
            let d = DividerView()
            d.onDrag = { [weak self] split, ratio, done in self?.drag(split, ratio: ratio, done: done) }
            addSubview(d)
            dividers.append(d)
        }
        for (d, s) in zip(dividers, splits) {
            d.split = s
            d.color = chrome.hairlineNS
        }
    }

    private func drag(_ s: Split, ratio: Double, done: Bool) {
        dragging = (s, ratio)
        needsLayout = true
        layoutSubtreeIfNeeded()
        ws.resizeSplit(s, ratio: ratio)
        if done {
            // Keep the dragged position until the server's view arrives.
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.6) { [weak self] in
                guard let self, self.dragging?.split.before == s.before else { return }
                self.dragging = nil
                self.needsLayout = true
            }
        }
    }

    /// A block's rect with any in-flight divider drag applied.
    private func rect(_ b: Block) -> (x: Double, y: Double, w: Double, h: Double) {
        var r = (x: b.x, y: b.y, w: b.w, h: b.h)
        guard let (s, ratio) = dragging else { return r }
        func remap(_ v: Double, _ start: Double, _ len: Double) -> Double {
            let old = start + len * s.ratio, new = start + len * ratio
            if v <= old { return start + (v - start) * (new - start) / max(old - start, 1e-6) }
            return new + (v - old) * (start + len - new) / max(start + len - old, 1e-6)
        }
        let inside = r.x >= s.x - 1e-6 && r.x + r.w <= s.x + s.w + 1e-6 && r.y >= s.y - 1e-6 && r.y + r.h <= s.y + s.h + 1e-6
        guard inside else { return r }
        if s.horizontal {
            let x0 = remap(r.x, s.x, s.w), x1 = remap(r.x + r.w, s.x, s.w)
            r.x = x0
            r.w = x1 - x0
        } else {
            let y0 = remap(r.y, s.y, s.h), y1 = remap(r.y + r.h, s.y, s.h)
            r.y = y0
            r.h = y1 - y0
        }
        return r
    }

    override func layout() {
        super.layout()
        guard let w = window_ else { return }
        let W = bounds.width, H = bounds.height
        let visible = Set(visibleBlocks(w).map(\.id))
        let gap: CGFloat = 1
        for b in w.blocks {
            guard let p = panes[b.id] else { continue }
            p.isHidden = !visible.contains(b.id)
            guard !p.isHidden else { continue }
            let r = w.zoomedBlockId == nil ? rect(b) : (x: 0.0, y: 0.0, w: 1.0, h: 1.0)
            var f = NSRect(x: (r.x * W).rounded(), y: (r.y * H).rounded(), width: 0, height: 0)
            f.size.width = ((r.x + r.w) * W).rounded() - f.minX
            f.size.height = ((r.y + r.h) * H).rounded() - f.minY
            // Leave a hairline gap where panes meet; the divider draws in it.
            if f.minX > 0.5 { f.origin.x += gap; f.size.width -= gap }
            if f.minY > 0.5 { f.origin.y += gap; f.size.height -= gap }
            if b.floating { f = f.insetBy(dx: 6, dy: 6) }
            if p.frame != f { p.frame = f }
        }
        for d in dividers {
            guard let s = d.split else { continue }
            let ratio = dragging.flatMap { $0.split.before == s.before && $0.split.after == s.after ? $0.ratio : nil } ?? s.ratio
            if s.horizontal {
                let x = ((s.x + s.w * ratio) * W).rounded()
                d.frame = NSRect(x: x - 3, y: s.y * H, width: 7, height: s.h * H)
            } else {
                let y = ((s.y + s.h * ratio) * H).rounded()
                d.frame = NSRect(x: s.x * W, y: y - 3, width: s.w * W, height: 7)
            }
            d.containerSize = bounds.size
        }
    }
}

/// A hairline between panes with a wider grab area.
final class DividerView: NSView {
    var split: Split? { didSet { needsDisplay = true; window?.invalidateCursorRects(for: self) } }
    var color: NSColor = .separatorColor { didSet { needsDisplay = true } }
    var containerSize: CGSize = .zero
    var onDrag: ((Split, Double, Bool) -> Void)?
    private var lastSent = Date.distantPast

    override var isFlipped: Bool { true }

    override func draw(_ dirtyRect: NSRect) {
        color.setFill()
        guard let s = split else { return }
        if s.horizontal {
            NSRect(x: 3, y: 0, width: 1, height: bounds.height).fill()
        } else {
            NSRect(x: 0, y: 3, width: bounds.width, height: 1).fill()
        }
    }

    override func resetCursorRects() {
        guard let s = split else { return }
        addCursorRect(bounds, cursor: s.horizontal ? .resizeLeftRight : .resizeUpDown)
    }

    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }

    override func mouseDown(with e: NSEvent) {
        window?.trackEvents(matching: [.leftMouseDragged, .leftMouseUp], timeout: .infinity, mode: .eventTracking) { ev, stop in
            guard let ev else { return }
            let up = ev.type == .leftMouseUp
            send(ev, done: up)
            if up { stop.pointee = true }
        }
    }

    private func send(_ e: NSEvent, done: Bool) {
        guard let s = split, let sv = superview, containerSize.width > 0 else { return }
        let p = sv.convert(e.locationInWindow, from: nil)
        let ratio: Double = s.horizontal
            ? (p.x / containerSize.width - s.x) / s.w
            : (p.y / containerSize.height - s.y) / s.h
        // The server gets at most ~30 updates a second; the local layout follows every event.
        let now = Date()
        guard done || now.timeIntervalSince(lastSent) > 0.033 else { return }
        lastSent = now
        onDrag?(s, min(max(ratio, 0.05), 0.95), done)
    }
}

/// One pane: a slim header when the window has several panes, the terminal,
/// and a status banner while it connects or after its program exits.
final class PaneView: NSView {
    let terminal: TerminalView
    private let ws: Workspace
    private let header = NSHostingView(rootView: PaneHeader(title: "", detail: "", agent: nil, focused: false, chrome: Chrome(Themes.all[0]), onAction: { _ in }))
    private let banner = NSHostingView(rootView: StatusBanner(text: nil, busy: false, chrome: Chrome(Themes.all[0])))
    private let dim = PassthroughView()
    private var showsHeader = false
    private var bannerText: String?
    static let headerHeight: CGFloat = 24

    init(terminal: TerminalView, ws: Workspace) {
        self.terminal = terminal
        self.ws = ws
        super.init(frame: .zero)
        wantsLayer = true
        addSubview(terminal)
        addSubview(header)
        dim.wantsLayer = true
        addSubview(dim)
        addSubview(banner)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError() }

    override var isFlipped: Bool { true }

    func update(block b: Block, focused: Bool, multiple: Bool, zoomed: Bool, chrome: Chrome) {
        terminal.isFocusedPane = focused
        showsHeader = multiple
        header.isHidden = !multiple
        let id = b.id
        header.rootView = PaneHeader(
            title: runningName(b) ?? b.name, detail: b.shortPwd, agent: b.agent, focused: focused, zoomed: zoomed, chrome: chrome,
            onAction: { [weak ws] a in ws?.run(a, blockId: id) }
        )
        // Unfocused panes recede a little so the focused one reads first.
        dim.layer?.backgroundColor = cgColor(0xFF00_0000 | chrome.bgHex).copy(alpha: multiple && !focused ? 0.28 : 0)
        let status = terminal.controller.status
        let mirrored = !terminal.controller.hasControl && b.ownedByOther
        // While the whole server is unreachable, one banner over the panes says so.
        let serverDown = ws.server?.connectivity.isOnline == false && status.isBusy
        let text = serverDown ? nil : status.banner ?? (mirrored ? "Sized by another device" : nil)
        bannerText = text
        banner.isHidden = text == nil
        banner.rootView = StatusBanner(
            text: text, busy: status.isBusy, chrome: chrome,
            onTake: mirrored && status.banner == nil ? { [weak ws] in ws?.run(.takeSize, blockId: id) } : nil,
            onClose: status.isExited ? { [weak ws] in ws?.closePane(id, confirmed: true) } : nil
        )
        needsLayout = true
    }

    override func layout() {
        super.layout()
        let top = showsHeader ? Self.headerHeight : 0
        header.frame = NSRect(x: 0, y: 0, width: bounds.width, height: top)
        let tf = NSRect(x: 0, y: top, width: bounds.width, height: max(0, bounds.height - top))
        if terminal.frame != tf { terminal.frame = tf }
        dim.frame = tf
        let size = banner.fittingSize
        banner.frame = NSRect(x: (bounds.width - size.width) / 2, y: bounds.height - size.height - 12, width: size.width, height: size.height)
    }

}

/// Draws over other views without taking their clicks.
final class PassthroughView: NSView {
    override func hitTest(_ point: NSPoint) -> NSView? { nil }
}

struct PaneHeader: View {
    var title: String
    var detail: String
    var agent: AgentStatus?
    var focused: Bool
    var zoomed = false
    var chrome: Chrome
    var onAction: (PaneAction) -> Void
    @State private var hover = false

    var body: some View {
        HStack(spacing: 6) {
            if let a = agent {
                Circle().fill(Palette.agent(a.state)).frame(width: 7, height: 7)
            }
            Text(title).font(.system(size: 11.5, weight: focused ? .semibold : .regular)).lineLimit(1)
                .foregroundStyle(focused ? chrome.text : chrome.secondary)
            if !detail.isEmpty, detail != title {
                Text(detail).font(.system(size: 11)).foregroundStyle(chrome.tertiary).lineLimit(1).truncationMode(.head)
            }
            if let a = agent, a.state == .blocked || a.state == .working {
                Text(a.label).font(.system(size: 10.5, weight: .medium)).foregroundStyle(Palette.agent(a.state))
                    .padding(.horizontal, 6).padding(.vertical, 1)
                    .background(Palette.agent(a.state).opacity(0.14), in: Capsule())
            }
            Spacer(minLength: 4)
            if zoomed {
                Button { onAction(.zoom) } label: {
                    Label("Zoomed", systemImage: "arrow.down.right.and.arrow.up.left").font(.system(size: 10.5, weight: .medium))
                }
                .buttonStyle(.borderless)
                .foregroundStyle(chrome.accent)
                .help("Show all panes (⇧⌘↩)")
            } else if hover || focused {
                HStack(spacing: 2) {
                    button(.splitRight, "Split right")
                    button(.splitDown, "Split down")
                    button(.zoom, "Zoom")
                    button(.close, "Close pane")
                }
                .opacity(hover ? 1 : 0.55)
            }
        }
        .padding(.horizontal, 8)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(focused ? chrome.raised : chrome.surface)
        .overlay(alignment: .bottom) {
            Rectangle().fill(focused ? chrome.accent.opacity(0.85) : chrome.hairline).frame(height: focused ? 1.5 : 1)
        }
        .onHover { hover = $0 }
        .contentShape(Rectangle())
    }

    private func button(_ a: PaneAction, _ help: String) -> some View {
        Button { onAction(a) } label: {
            Image(systemName: a.symbol).font(.system(size: 10, weight: .medium)).frame(width: 18, height: 16)
        }
        .buttonStyle(.borderless)
        .foregroundStyle(chrome.secondary)
        .help(help)
    }
}

struct StatusBanner: View {
    var text: String?
    var busy: Bool
    var chrome: Chrome
    var onTake: (() -> Void)?
    var onClose: (() -> Void)?

    var body: some View {
        if let text {
            HStack(spacing: 8) {
                if busy { ProgressView().controlSize(.small) }
                Text(text).font(.system(size: 12, weight: .medium)).foregroundStyle(chrome.text)
                if let onTake {
                    Button("Fit Here", action: onTake).controlSize(.small)
                }
                if let onClose {
                    Button("Close Pane", action: onClose).controlSize(.small)
                }
            }
            .padding(.horizontal, 12).padding(.vertical, 6)
            .background(.regularMaterial, in: Capsule())
            .overlay(Capsule().strokeBorder(chrome.glassStroke))
            .environment(\.colorScheme, chrome.scheme)
            .fixedSize()
        }
    }
}

extension Chrome {
    var hairlineNS: NSColor {
        NSColor(cgColor: cgColor(mix(bgHex, textHex, light ? 0.18 : 0.16))) ?? .separatorColor
    }
}

/// `a` moved toward `b` by `t`, as 0xFFRRGGBB.
func mix(_ a: UInt32, _ b: UInt32, _ t: Double) -> UInt32 {
    func ch(_ c: UInt32, _ s: UInt32) -> Double { Double((c >> s) & 0xFF) }
    func m(_ s: UInt32) -> UInt32 { UInt32((ch(a, s) * (1 - t) + ch(b, s) * t).rounded()) }
    return 0xFF00_0000 | m(16) << 16 | m(8) << 8 | m(0)
}
