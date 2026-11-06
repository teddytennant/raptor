import AppKit
import RaptorKit

extension TerminalView {
    /// Select the next match of `query` (case-insensitive), paging through history.
    /// Returns false, leaving the view where it was, when there is none.
    func find(_ query: String, older: Bool) -> Bool {
        guard let t = terminal else { return false }
        var f = t.frame()
        let home = f.scrollOffset
        let from = findPos
        while true {
            let rows = f.lines.map(Self.rowText)
            let order = older ? Array(rows.indices.reversed()) : Array(rows.indices)
            for r in order {
                let abs = Int(f.scrollOffset) + r
                if let from, older ? abs > from.row : abs < from.row { continue }
                var hits = Self.matches(query, in: rows[r])
                if let from, abs == from.row { hits = hits.filter { older ? $0.0 < from.col : $0.0 > from.col } }
                guard let hit = older ? hits.last : hits.first else { continue }
                findPos = (abs, hit.0)
                _ = t.selectRange(fromCol: UInt16(hit.0), fromRow: UInt16(r), toCol: UInt16(hit.1), toRow: UInt16(r))
                refresh()
                return true
            }
            let before = f.scrollOffset
            t.scroll(rows: Int32(older ? -(Int(f.rows) - 1) : Int(f.rows) - 1))
            f = t.frame()
            if f.scrollOffset == before {
                t.scroll(rows: Int32(Int64(home) - Int64(f.scrollOffset)))
                refresh()
                NSSound.beep()
                return false
            }
        }
    }

    func resetFind() {
        findPos = nil
        terminal?.clearSelection()
    }

    /// A row's text and the column each character starts at.
    private static func rowText(_ line: Line) -> (text: [Character], cols: [Int], widths: [Int]) {
        var text: [Character] = [], cols: [Int] = [], widths: [Int] = []
        var next = 0
        for run in line.runs {
            if Int(run.col) > next {
                for c in next..<Int(run.col) { text.append(" "); cols.append(c); widths.append(1) }
            }
            var col = Int(run.col)
            let w = [UInt8](run.widths)
            for (i, ch) in run.text.enumerated() {
                let cw = i < w.count ? Int(w[i]) : 1
                text.append(ch); cols.append(col); widths.append(cw)
                col += cw
            }
            next = col
        }
        return (text, cols, widths)
    }

    /// Start and end columns of every match in a row.
    private static func matches(_ query: String, in row: (text: [Character], cols: [Int], widths: [Int])) -> [(Int, Int)] {
        let needle = query.map { $0.lowercased() }
        let hay = row.text.map { $0.lowercased() }
        guard !needle.isEmpty, hay.count >= needle.count else { return [] }
        var out: [(Int, Int)] = []
        for i in 0...(hay.count - needle.count) where Array(hay[i..<i + needle.count]) == needle {
            let last = i + needle.count - 1
            out.append((row.cols[i], row.cols[last] + row.widths[last] - 1))
        }
        return out
    }
}
