#if os(iOS)
import UIKit

/// What the keyboard sees as the "document". `committed` is text already sent to
/// the terminal since the last reset, kept so dictation can revise it (we send
/// backspaces and the new text). `marked` is an IME composition (Japanese,
/// Chinese, Korean) that reaches the terminal only once it is committed.
struct KeyboardDocument {
    var committed = ""
    var marked = ""
    var markedSelection = NSRange(location: 0, length: 0)

    var text: String { committed + marked }
    var length: Int { (text as NSString).length }
    var markedStart: Int { (committed as NSString).length }
}

final class TextPosition: UITextPosition {
    let offset: Int
    init(_ offset: Int) { self.offset = offset }
}

final class TextRange: UITextRange {
    let lo: Int
    let hi: Int
    init(_ a: Int, _ b: Int) {
        lo = min(a, b)
        hi = max(a, b)
    }
    override var start: UITextPosition { TextPosition(lo) }
    override var end: UITextPosition { TextPosition(hi) }
    override var isEmpty: Bool { lo == hi }
}

extension TerminalUIView: UITextInput {
    private func clamp(_ o: Int) -> Int { min(max(o, 0), kb.length) }
    private func off(_ p: UITextPosition) -> Int { clamp((p as? TextPosition)?.offset ?? kb.length) }

    public func text(in range: UITextRange) -> String? {
        guard let r = range as? TextRange else { return nil }
        let lo = clamp(r.lo), hi = clamp(r.hi)
        return (kb.text as NSString).substring(with: NSRange(location: lo, length: hi - lo))
    }

    public func replace(_ range: UITextRange, withText text: String) {
        guard let r = range as? TextRange else { return }
        let lo = clamp(r.lo), hi = clamp(r.hi)
        let new = (kb.text as NSString).replacingCharacters(in: NSRange(location: lo, length: hi - lo), with: text)
        kb.marked = ""
        commit(new)
    }

    public var selectedTextRange: UITextRange? {
        get {
            if kb.marked.isEmpty { return TextRange(kb.length, kb.length) }
            let s = kb.markedStart + kb.markedSelection.location
            return TextRange(s, s + kb.markedSelection.length)
        }
        set {
            // Only an IME moving within its own composition means anything here.
            guard let r = newValue as? TextRange, !kb.marked.isEmpty else { return }
            let lo = min(max(r.lo - kb.markedStart, 0), (kb.marked as NSString).length)
            kb.markedSelection = NSRange(location: lo, length: min(r.hi - r.lo, (kb.marked as NSString).length - lo))
        }
    }

    public var markedTextRange: UITextRange? {
        kb.marked.isEmpty ? nil : TextRange(kb.markedStart, kb.length)
    }

    public func setMarkedText(_ markedText: String?, selectedRange: NSRange) {
        kb.marked = markedText ?? ""
        kb.markedSelection = selectedRange
        setNeedsDisplay()
    }

    public func unmarkText() {
        guard !kb.marked.isEmpty else { return }
        let m = kb.marked
        kb.marked = ""
        commit(kb.committed + m)
    }

    public var beginningOfDocument: UITextPosition { TextPosition(0) }
    public var endOfDocument: UITextPosition { TextPosition(kb.length) }

    public func textRange(from fromPosition: UITextPosition, to toPosition: UITextPosition) -> UITextRange? {
        TextRange(off(fromPosition), off(toPosition))
    }

    public func position(from position: UITextPosition, offset: Int) -> UITextPosition? {
        let o = off(position) + offset
        return (0...kb.length).contains(o) ? TextPosition(o) : nil
    }

    public func position(from position: UITextPosition, in direction: UITextLayoutDirection, offset: Int) -> UITextPosition? {
        switch direction {
        case .left, .up: self.position(from: position, offset: -offset)
        default: self.position(from: position, offset: offset)
        }
    }

    public func compare(_ position: UITextPosition, to other: UITextPosition) -> ComparisonResult {
        let a = off(position), b = off(other)
        return a < b ? .orderedAscending : a > b ? .orderedDescending : .orderedSame
    }

    public func offset(from: UITextPosition, to toPosition: UITextPosition) -> Int { off(toPosition) - off(from) }

    public func position(within range: UITextRange, farthestIn direction: UITextLayoutDirection) -> UITextPosition? {
        switch direction {
        case .left, .up: range.start
        default: range.end
        }
    }

    public func characterRange(byExtending position: UITextPosition, in direction: UITextLayoutDirection) -> UITextRange? {
        switch direction {
        case .left, .up: TextRange(0, off(position))
        default: TextRange(off(position), kb.length)
        }
    }

    public func baseWritingDirection(for position: UITextPosition, in direction: UITextStorageDirection) -> NSWritingDirection { .leftToRight }
    public func setBaseWritingDirection(_ writingDirection: NSWritingDirection, for range: UITextRange) {}

    /// Where the composition sits, so candidate windows open next to it.
    public func firstRect(for range: UITextRange) -> CGRect {
        var r = cursorRect()
        r.size.width = max(r.width, CGFloat(cellCount(kb.marked)) * r.width)
        return r
    }

    public func caretRect(for position: UITextPosition) -> CGRect {
        let r = cursorRect()
        return CGRect(x: r.minX + CGFloat(cellCount(kb.marked)) * r.width, y: r.minY, width: 2, height: r.height)
    }

    public func selectionRects(for range: UITextRange) -> [UITextSelectionRect] { [] }
    public func closestPosition(to point: CGPoint) -> UITextPosition? { endOfDocument }
    public func closestPosition(to point: CGPoint, within range: UITextRange) -> UITextPosition? { range.end }
    public func characterRange(at point: CGPoint) -> UITextRange? { nil }

    /// Bring the terminal in line with a new committed text: backspace over what
    /// changed, then type the rest.
    func commit(_ new: String) {
        let old = kb.committed
        var i = old.startIndex, j = new.startIndex
        while i < old.endIndex, j < new.endIndex, old[i] == new[j] {
            i = old.index(after: i)
            j = new.index(after: j)
        }
        let back = old.distance(from: i, to: old.endIndex)
        kb.committed = new
        for _ in 0..<back { controller.terminal?.sendKey(key: .backspace, ctrl: false, alt: false, shift: false, meta: false) }
        let tail = String(new[j...])
        if !tail.isEmpty { type(tail) }
        setNeedsDisplay()
    }

    /// Forget the document after something the keyboard didn't do (an arrow key,
    /// Enter, a tap), so dictation never revises text that is no longer there.
    func resetKeyboardDocument() {
        guard !kb.committed.isEmpty || !kb.marked.isEmpty else { return }
        if !kb.marked.isEmpty { controller.terminal?.sendText(text: kb.marked) }
        inputDelegate?.textWillChange(self)
        kb = KeyboardDocument()
        inputDelegate?.textDidChange(self)
        setNeedsDisplay()
    }
}

#endif
