import Foundation

/// A run's text split into one string per cell. The core joins cell texts with
/// no separator, and Swift's grapheme rules can merge neighbouring cells (a flag
/// whose halves sit in two cells, a skin tone in a cell of its own), which would
/// shift every later character. The cell count is the truth; when Swift's split
/// disagrees, split by scalars the way the terminal attaches them.
func cellTexts(_ text: String, count: Int) -> [String] {
    let chars = text.map(String.init)
    if chars.count == count { return chars }
    // Without grapheme clustering a terminal keeps a ZWJ, variation selector or combining
    // mark with the cell before it and starts a new cell for everything else.
    for (joinAfterZWJ, attachModifiers) in [(false, false), (true, true), (true, false), (false, true)] {
        var cells: [String] = []
        var afterJoiner = false
        for s in text.unicodeScalars {
            let v = s.value
            let mark = [.nonspacingMark, .enclosingMark, .spacingMark].contains(s.properties.generalCategory)
            let attach = (joinAfterZWJ && afterJoiner) || mark || v == 0x200D || (0xFE00...0xFE0F).contains(v) || (0xE0020...0xE007F).contains(v)
                || (attachModifiers && (0x1F3FB...0x1F3FF).contains(v))
            afterJoiner = v == 0x200D
            if attach, !cells.isEmpty { cells[cells.count - 1].unicodeScalars.append(s) } else { cells.append(String(s)) }
        }
        if cells.count == count { return cells }
    }
    return Array((chars + Array(repeating: " ", count: max(0, count - chars.count))).prefix(count))
}
