import CoreGraphics
import CoreText
import Foundation

private let styleBold: UInt16 = 1
private let styleItalic: UInt16 = 1 << 1
private let styleFaint: UInt16 = 1 << 2
private let styleStrike: UInt16 = 1 << 3
private let styleOverline: UInt16 = 1 << 4
private let styleSelected: UInt16 = 1 << 5

/// Cell geometry for a font size.
public struct CellMetrics: Equatable, Sendable {
    public var width: CGFloat
    public var height: CGFloat
    public var ascent: CGFloat
    public var descent: CGFloat

    public init(fontSize: CGFloat, font: String = "JetBrainsMono-Regular", lineHeight: CGFloat = 1.06) {
        Fonts.register()
        let f = CTFontCreateWithName(font as CFString, fontSize, nil)
        var glyph = CGGlyph()
        var ch = UniChar(UInt8(ascii: "M"))
        CTFontGetGlyphsForCharacters(f, &ch, &glyph, 1)
        var adv = CGSize.zero
        CTFontGetAdvancesForGlyphs(f, .horizontal, &glyph, &adv, 1)
        width = adv.width
        ascent = CTFontGetAscent(f)
        descent = CTFontGetDescent(f)
        height = ceil((ascent + descent + CTFontGetLeading(f)) * lineHeight)
    }
}

/// Draws a core `Frame` into a CoreGraphics context with Core Text.
/// The context must be flipped (origin top-left), as UIKit's is.
public final class TerminalRenderer {
    public private(set) var fontSize: CGFloat
    public private(set) var metrics: CellMetrics
    private var fonts: [CTFont] = []
    private var lineCache: [String: CTLine] = [:]

    public private(set) var font: TermFont = Fonts.byId(nil)
    public private(set) var lineHeight: CGFloat = 1.06

    public init(fontSize: CGFloat) {
        self.fontSize = fontSize
        self.metrics = CellMetrics(fontSize: fontSize)
        loadFonts()
    }

    /// Font family and line spacing from Settings.
    public func setAppearance(fontId: String?, lineHeight: CGFloat) {
        font = Fonts.byId(fontId)
        self.lineHeight = min(max(lineHeight, 1.0), 1.6)
        metrics = CellMetrics(fontSize: fontSize, font: font.faces[0], lineHeight: self.lineHeight)
        loadFonts()
    }

    public func setFontSize(_ s: CGFloat) {
        let s = min(max(s, 7), 30)
        guard s != fontSize else { return }
        fontSize = s
        metrics = CellMetrics(fontSize: s, font: font.faces[0], lineHeight: lineHeight)
        loadFonts()
    }

    private func loadFonts() {
        fonts = font.faces.map { CTFontCreateWithName($0 as CFString, fontSize, nil) }
        lineCache.removeAll(keepingCapacity: true)
    }

    private func font(bold: Bool, italic: Bool) -> CTFont { fonts[(bold ? 1 : 0) + (italic ? 2 : 0)] }

    /// Grid that fits `size` points.
    public func grid(for size: CGSize, padding: CGFloat) -> (cols: Int, rows: Int) {
        (max(10, Int((size.width - padding * 2) / metrics.width)), max(4, Int((size.height - padding * 2) / metrics.height)))
    }

    public struct Options {
        public var padding: CGFloat = 6
        public var scale: CGFloat = 1
        public var panX: CGFloat = 0
        public var focused = true
        public var cursorVisible = true
        /// Forced cursor shape from Settings; nil follows the program.
        public var cursorShape: CursorShape?
        /// Draw the thin history position bar (off where the platform has its own scroller).
        public var historyBar = true
        /// Text an input method is still composing, drawn at the cursor.
        public var preedit = ""
        public init() {}
    }

    public func draw(_ frame: Frame, in ctx: CGContext, size: CGSize, options o: Options) {
        let bg = cg(frame.background)
        ctx.setFillColor(bg)
        ctx.fill(CGRect(origin: .zero, size: size))
        ctx.saveGState()
        ctx.translateBy(x: o.padding - o.panX, y: o.padding)
        ctx.scaleBy(x: o.scale, y: o.scale)

        let m = metrics
        let fgDefault = frame.foreground
        let selection = blend(fgDefault, frame.background, 0.30)

        // Only rows the dirty rect touches (a blinking cursor redraws one cell).
        let clip = ctx.boundingBoxOfClipPath
        for (row, line) in frame.lines.enumerated() {
            let top = CGFloat(row) * m.height
            guard top + m.height * 2 >= clip.minY, top - m.height <= clip.maxY else { continue }
            // Unantialiased, so neighbouring backgrounds meet on a pixel edge with no seam.
            ctx.setShouldAntialias(false)
            for run in line.runs {
                let selected = run.style & styleSelected != 0
                guard let color = selected ? selection : run.bg else { continue }
                ctx.setFillColor(cg(color))
                ctx.fill(CGRect(x: CGFloat(run.col) * m.width, y: top, width: CGFloat(run.cells) * m.width, height: m.height + 0.5))
            }
            ctx.setShouldAntialias(true)
            for run in line.runs where !run.text.allSatisfy({ $0 == " " }) {
                var color = readable(run.fg, on: run.bg ?? frame.background, fallback: frame.foreground)
                if run.style & styleFaint != 0 { color = blend(color, frame.background, 0.5) }
                let f = font(bold: run.style & styleBold != 0, italic: run.style & styleItalic != 0)
                drawRun(run, font: f, color: cg(color), at: CGPoint(x: CGFloat(run.col) * m.width, y: top), in: ctx)
                decorate(run, color: cg(run.underlineColor ?? color), top: top, in: ctx)
            }
        }

        if let c = frame.cursor, !o.preedit.isEmpty {
            drawPreedit(o.preedit, at: c, frame: frame, in: ctx)
        } else if let c = frame.cursor, o.cursorVisible, frame.scrollOffset + frame.scrollLen >= frame.scrollTotal {
            drawCursor(c, frame: frame, in: ctx, focused: o.focused, shape: o.cursorShape ?? c.shape)
        }
        ctx.restoreGState()

        // History indicator.
        if o.historyBar, frame.scrollTotal > frame.scrollLen, frame.scrollOffset + frame.scrollLen < frame.scrollTotal {
            let track = size.height - o.padding * 2
            let thumb = max(24, track * CGFloat(frame.scrollLen) / CGFloat(frame.scrollTotal))
            let y = o.padding + (track - thumb) * CGFloat(frame.scrollOffset) / CGFloat(frame.scrollTotal - frame.scrollLen)
            ctx.setFillColor(cg(blend(fgDefault, frame.background, 0.55)))
            ctx.addPath(CGPath(roundedRect: CGRect(x: size.width - 5, y: y, width: 3, height: thumb), cornerWidth: 1.5, cornerHeight: 1.5, transform: nil))
            ctx.fillPath()
        }
    }

    private func drawRun(_ run: Run, font: CTFont, color: CGColor, at origin: CGPoint, in ctx: CGContext) {
        let m = metrics
        let baseline = origin.y + (m.height - (m.ascent + m.descent)) / 2 + m.ascent
        let widths = [UInt8](run.widths)
        let graphemes = cellTexts(run.text, count: widths.count)
        var col: CGFloat = 0
        // Group consecutive simple ASCII so Core Text shapes them in one pass;
        // everything else is placed on its own cell so it can't drift off the grid.
        var i = 0
        while i < graphemes.count {
            let w = CGFloat(i < widths.count ? widths[i] : 1)
            let g = graphemes[i]
            let x = origin.x + col * m.width
            if let scalar = g.unicodeScalars.first, g.unicodeScalars.count == 1, (0x2500...0x259F).contains(scalar.value) || (0xE0B0...0xE0BF).contains(scalar.value) {
                if drawBox(scalar.value, x: x, top: origin.y, cells: w, color: color, in: ctx) {
                    col += w
                    i += 1
                    continue
                }
            }
            if g.utf8.count == 1 && w == 1 {
                var j = i
                var s = ""
                while j < graphemes.count, graphemes[j].utf8.count == 1, (j < widths.count ? widths[j] : 1) == 1 {
                    s += graphemes[j]
                    j += 1
                }
                drawText(s, font: font, color: color, x: x, baseline: baseline, in: ctx)
                col += CGFloat(j - i)
                i = j
                continue
            }
            drawText(g, font: font, color: color, x: x, baseline: baseline, in: ctx, fitWidth: w * m.width)
            col += w
            i += 1
        }
    }

    private func drawText(_ s: String, font: CTFont, color: CGColor, x: CGFloat, baseline: CGFloat, in ctx: CGContext, fitWidth: CGFloat? = nil) {
        // Lines take their color from the context, so one cached line serves every color.
        let key = "\(CFHash(font))\u{0}\(s)"
        let line: CTLine
        if let cached = lineCache[key] {
            line = cached
        } else {
            let attrs: [CFString: Any] = [kCTFontAttributeName: font, kCTForegroundColorFromContextAttributeName: true]
            line = CTLineCreateWithAttributedString(CFAttributedStringCreate(nil, s as CFString, attrs as CFDictionary))
            if lineCache.count > 8192 { lineCache.removeAll(keepingCapacity: true) }
            lineCache[key] = line
        }
        ctx.setFillColor(color)
        var dx: CGFloat = 0
        if let fit = fitWidth {
            let w = CGFloat(CTLineGetTypographicBounds(line, nil, nil, nil))
            if w > fit * 1.05 {
                // Fallback glyph wider than its cells (some emoji): shrink to fit.
                ctx.saveGState()
                ctx.translateBy(x: x, y: baseline)
                ctx.scaleBy(x: fit / w, y: -fit / w)
                ctx.textPosition = .zero
                CTLineDraw(line, ctx)
                ctx.restoreGState()
                return
            }
            dx = (fit - w) / 2
        }
        ctx.saveGState()
        ctx.textMatrix = .identity
        ctx.translateBy(x: x + dx, y: baseline)
        ctx.scaleBy(x: 1, y: -1)
        ctx.textPosition = .zero
        CTLineDraw(line, ctx)
        ctx.restoreGState()
    }

    private func decorate(_ run: Run, color: CGColor, top: CGFloat, in ctx: CGContext) {
        let ul = Int((run.style >> 8) & 0xF)
        let x0 = CGFloat(run.col) * metrics.width
        let x1 = x0 + CGFloat(run.cells) * metrics.width
        let h = metrics.height
        guard ul != 0 || run.style & (styleStrike | styleOverline) != 0 else { return }
        ctx.setStrokeColor(color)
        ctx.setLineWidth(1)
        let uy = top + h - 1.5
        switch ul {
        case 0: break
        case 2:
            stroke(ctx, x0, uy, x1, uy)
            stroke(ctx, x0, uy - 2.5, x1, uy - 2.5)
        case 3:
            ctx.beginPath()
            var x = x0
            ctx.move(to: CGPoint(x: x, y: uy))
            var up = true
            while x < x1 {
                let nx = min(x + 2.5, x1)
                ctx.addQuadCurve(to: CGPoint(x: nx, y: uy), control: CGPoint(x: (x + nx) / 2, y: uy + (up ? -2 : 2)))
                x = nx
                up.toggle()
            }
            ctx.strokePath()
        case 4, 5:
            ctx.setLineDash(phase: 0, lengths: ul == 4 ? [1, 2] : [3, 3])
            stroke(ctx, x0, uy, x1, uy)
            ctx.setLineDash(phase: 0, lengths: [])
        default: stroke(ctx, x0, uy, x1, uy)
        }
        if run.style & styleStrike != 0 { stroke(ctx, x0, top + h / 2, x1, top + h / 2) }
        if run.style & styleOverline != 0 { stroke(ctx, x0, top + 0.5, x1, top + 0.5) }
    }

    private func stroke(_ ctx: CGContext, _ x0: CGFloat, _ y0: CGFloat, _ x1: CGFloat, _ y1: CGFloat) {
        ctx.beginPath()
        ctx.move(to: CGPoint(x: x0, y: y0))
        ctx.addLine(to: CGPoint(x: x1, y: y1))
        ctx.strokePath()
    }

    /// Box drawing and blocks as geometry so lines meet exactly at cell edges.
    private func drawBox(_ cp: UInt32, x: CGFloat, top: CGFloat, cells: CGFloat, color: CGColor, in ctx: CGContext) -> Bool {
        // Edges on device pixels, so blocks and lines meet their neighbours with no seam.
        func snap(_ p: CGPoint) -> CGPoint {
            let d = ctx.convertToDeviceSpace(p)
            return ctx.convertToUserSpace(CGPoint(x: d.x.rounded(), y: d.y.rounded()))
        }
        let px = abs(ctx.convertToUserSpace(CGSize(width: 1, height: 1)).width)
        func fill(_ rect: CGRect) {
            let a = snap(rect.origin), z = snap(CGPoint(x: rect.maxX, y: rect.maxY))
            ctx.fill(CGRect(x: a.x, y: a.y, width: max(z.x - a.x, px), height: max(z.y - a.y, px)))
        }
        let tl = snap(CGPoint(x: x, y: top)), br = snap(CGPoint(x: x + metrics.width * cells, y: top + metrics.height))
        let x = tl.x, top = tl.y, r = br.x, b = br.y
        let w = r - x, h = b - top
        let mid = snap(CGPoint(x: x + w / 2, y: top + h / 2))
        let cx = mid.x, cy = mid.y
        let t: CGFloat = 1, T: CGFloat = 2
        ctx.setFillColor(color)
        func hl(_ a: CGFloat, _ z: CGFloat, _ th: CGFloat = t) { fill(CGRect(x: a, y: cy - th / 2, width: z - a, height: th)) }
        func vl(_ a: CGFloat, _ z: CGFloat, _ th: CGFloat = t) { fill(CGRect(x: cx - th / 2, y: a, width: th, height: z - a)) }
        switch cp {
        case 0x2500: hl(x, r)
        case 0x2501: hl(x, r, T)
        case 0x2502: vl(top, b)
        case 0x2503: vl(top, b, T)
        case 0x250C: hl(cx - t / 2, r); vl(cy - t / 2, b)
        case 0x2510: hl(x, cx + t / 2); vl(cy - t / 2, b)
        case 0x2514: hl(cx - t / 2, r); vl(top, cy + t / 2)
        case 0x2518: hl(x, cx + t / 2); vl(top, cy + t / 2)
        case 0x250F: hl(cx - T / 2, r, T); vl(cy - T / 2, b, T)
        case 0x2513: hl(x, cx + T / 2, T); vl(cy - T / 2, b, T)
        case 0x2517: hl(cx - T / 2, r, T); vl(top, cy + T / 2, T)
        case 0x251B: hl(x, cx + T / 2, T); vl(top, cy + T / 2, T)
        case 0x256D, 0x256E, 0x2570, 0x256F:
            // Rounded corners: a quarter circle into the cell's center lines.
            let right = cp == 0x256D || cp == 0x2570, down = cp == 0x256D || cp == 0x256E
            let rad = min(w, h) / 2
            ctx.setStrokeColor(color)
            ctx.setLineWidth(t)
            ctx.beginPath()
            ctx.move(to: CGPoint(x: right ? r : x, y: cy))
            ctx.addArc(tangent1End: CGPoint(x: cx, y: cy), tangent2End: CGPoint(x: cx, y: down ? b : top), radius: rad)
            ctx.addLine(to: CGPoint(x: cx, y: down ? b : top))
            ctx.strokePath()
        case 0x251C: vl(top, b); hl(cx, r)
        case 0x2524: vl(top, b); hl(x, cx)
        case 0x252C: hl(x, r); vl(cy, b)
        case 0x2534: hl(x, r); vl(top, cy)
        case 0x253C: hl(x, r); vl(top, b)
        case 0x2523: vl(top, b, T); hl(cx, r, T)
        case 0x252B: vl(top, b, T); hl(x, cx, T)
        case 0x2533: hl(x, r, T); vl(cy, b, T)
        case 0x253B: hl(x, r, T); vl(top, cy, T)
        case 0x254B: hl(x, r, T); vl(top, b, T)
        case 0x2574: hl(x, cx + t / 2)
        case 0x2575: vl(top, cy + t / 2)
        case 0x2576: hl(cx - t / 2, r)
        case 0x2577: vl(cy - t / 2, b)
        case 0x2550: fill(CGRect(x: x, y: cy - T, width: w, height: t)); fill(CGRect(x: x, y: cy + T - t, width: w, height: t))
        case 0x2551: fill(CGRect(x: cx - T, y: top, width: t, height: h)); fill(CGRect(x: cx + T - t, y: top, width: t, height: h))
        case 0x2554, 0x2557, 0x255A, 0x255D:
            // Double corners: two nested corners on the double lines' tracks.
            let right = cp == 0x2554 || cp == 0x255A, down = cp == 0x2554 || cp == 0x2557
            let outer = (right ? cx - T : cx + T - t, down ? cy - T : cy + T - t)
            let inner = (right ? cx + T - t : cx - T, down ? cy + T - t : cy - T)
            for (vx, hy) in [outer, inner] {
                ctx.fill(right ? CGRect(x: vx, y: hy, width: r - vx, height: t) : CGRect(x: x, y: hy, width: vx + t - x, height: t))
                ctx.fill(down ? CGRect(x: vx, y: hy, width: t, height: b - hy) : CGRect(x: vx, y: top, width: t, height: hy + t - top))
            }
        case 0xE0B0, 0xE0B2, 0xE0B8, 0xE0BA, 0xE0BC, 0xE0BE:
            // Powerline solid shapes, drawn to the cell edges so segments join cleanly.
            let pts: [CGPoint] = switch cp {
            case 0xE0B0: [CGPoint(x: x, y: top), CGPoint(x: r, y: cy), CGPoint(x: x, y: b)]
            case 0xE0B2: [CGPoint(x: r, y: top), CGPoint(x: x, y: cy), CGPoint(x: r, y: b)]
            case 0xE0B8: [CGPoint(x: x, y: top), CGPoint(x: r, y: b), CGPoint(x: x, y: b)]
            case 0xE0BA: [CGPoint(x: r, y: top), CGPoint(x: r, y: b), CGPoint(x: x, y: b)]
            case 0xE0BC: [CGPoint(x: x, y: top), CGPoint(x: r, y: top), CGPoint(x: x, y: b)]
            default: [CGPoint(x: x, y: top), CGPoint(x: r, y: top), CGPoint(x: r, y: b)]
            }
            ctx.addLines(between: pts)
            ctx.closePath()
            ctx.fillPath()
        case 0xE0B1, 0xE0B3:
            ctx.setStrokeColor(color)
            ctx.setLineWidth(t)
            ctx.addLines(between: cp == 0xE0B1
                ? [CGPoint(x: x, y: top), CGPoint(x: r - t / 2, y: cy), CGPoint(x: x, y: b)]
                : [CGPoint(x: r, y: top), CGPoint(x: x + t / 2, y: cy), CGPoint(x: r, y: b)])
            ctx.strokePath()
        case 0xE0B4, 0xE0B6:
            ctx.saveGState()
            ctx.clip(to: CGRect(x: x, y: top, width: w, height: h))
            ctx.fillEllipse(in: CGRect(x: cp == 0xE0B4 ? x - w : x, y: top, width: w * 2, height: h))
            ctx.restoreGState()
        case 0x2580: fill(CGRect(x: x, y: top, width: w, height: h / 2))
        case 0x2584: fill(CGRect(x: x, y: cy, width: w, height: h / 2))
        case 0x2588: fill(CGRect(x: x, y: top, width: w, height: h))
        case 0x258C: fill(CGRect(x: x, y: top, width: w / 2, height: h))
        case 0x2590: fill(CGRect(x: cx, y: top, width: w / 2, height: h))
        case 0x2591, 0x2592, 0x2593:
            ctx.saveGState()
            ctx.setAlpha(cp == 0x2591 ? 0.25 : cp == 0x2592 ? 0.5 : 0.75)
            fill(CGRect(x: x, y: top, width: w, height: h))
            ctx.restoreGState()
        case 0x2581...0x2587:
            let f = CGFloat(cp - 0x2580) / 8
            fill(CGRect(x: x, y: b - h * f, width: w, height: h * f))
        case 0x2589...0x258F:
            fill(CGRect(x: x, y: top, width: w * CGFloat(0x2590 - cp) / 8, height: h))
        default: return false
        }
        return true
    }

    private func drawCursor(_ c: Cursor, frame: Frame, in ctx: CGContext, focused: Bool, shape: CursorShape) {
        let m = metrics
        let rect = CGRect(x: CGFloat(c.col) * m.width, y: CGFloat(c.row) * m.height, width: m.width * (c.wide ? 2 : 1), height: m.height)
        let color = cg(frame.cursorColor ?? frame.foreground)
        ctx.setFillColor(color)
        ctx.setStrokeColor(color)
        if c.password {
            // A small lock reads better than a caret while a password is typed.
            let body = CGRect(x: rect.minX + rect.width * 0.15, y: rect.minY + rect.height * 0.45, width: rect.width * 0.7, height: rect.height * 0.42)
            ctx.addPath(CGPath(roundedRect: body, cornerWidth: 1.5, cornerHeight: 1.5, transform: nil))
            ctx.fillPath()
            ctx.setLineWidth(1.2)
            ctx.addArc(center: CGPoint(x: rect.midX, y: body.minY), radius: rect.width * 0.22, startAngle: .pi, endAngle: 0, clockwise: false)
            ctx.strokePath()
            return
        }
        if !focused || shape == .hollowBlock {
            ctx.setLineWidth(1)
            ctx.stroke(rect.insetBy(dx: 0.5, dy: 0.5))
            return
        }
        switch shape {
        case .bar: ctx.fill(CGRect(x: rect.minX, y: rect.minY, width: 2, height: rect.height))
        case .underline: ctx.fill(CGRect(x: rect.minX, y: rect.maxY - 2, width: rect.width, height: 2))
        default:
            ctx.fill(rect)
            // Redraw the character under the block in the background color.
            if let line = frame.lines[safe: Int(c.row)], let run = line.runs.first(where: { c.col >= $0.col && c.col < $0.col + $0.cells }) {
                let idx = graphemeIndex(run.widths, Int(c.col - run.col))
                let chars = cellTexts(run.text, count: run.widths.count)
                if idx < chars.count, chars[idx] != " " {
                    let f = font(bold: run.style & styleBold != 0, italic: run.style & styleItalic != 0)
                    let baseline = rect.minY + (m.height - (m.ascent + m.descent)) / 2 + m.ascent
                    drawText(chars[idx], font: f, color: cg(frame.background), x: rect.minX, baseline: baseline, in: ctx)
                }
            }
        }
    }

    /// An IME composition: underlined over the cells it will take, with a caret
    /// after it. Shifted left if it would run past the right edge.
    private func drawPreedit(_ text: String, at c: Cursor, frame: Frame, in ctx: CGContext) {
        let m = metrics
        let cells = cellCount(text)
        let col = max(0, min(Int(c.col), Int(frame.cols) - cells))
        let top = CGFloat(c.row) * m.height
        var x = CGFloat(col) * m.width
        let width = CGFloat(cells) * m.width
        ctx.setFillColor(cg(frame.background))
        ctx.fill(CGRect(x: x, y: top, width: width, height: m.height))
        let fg = cg(frame.foreground)
        let baseline = top + (m.height - (m.ascent + m.descent)) / 2 + m.ascent
        for g in text {
            let w = CGFloat(cellWidth(g)) * m.width
            drawText(String(g), font: fonts[0], color: fg, x: x, baseline: baseline, in: ctx, fitWidth: w)
            x += w
        }
        ctx.setFillColor(fg)
        ctx.fill(CGRect(x: CGFloat(col) * m.width, y: top + m.height - 1.5, width: width, height: 1))
        ctx.fill(CGRect(x: x, y: top, width: 2, height: m.height))
    }

    private func graphemeIndex(_ widths: Data, _ cellOffset: Int) -> Int {
        var c = 0
        for (i, w) in widths.enumerated() {
            if cellOffset < c + Int(w) { return i }
            c += Int(w)
        }
        return max(0, widths.count - 1)
    }
}

func cg(_ argb: UInt32) -> CGColor {
    CGColor(srgbRed: CGFloat((argb >> 16) & 0xFF) / 255, green: CGFloat((argb >> 8) & 0xFF) / 255, blue: CGFloat(argb & 0xFF) / 255, alpha: 1)
}

/// Cells a string takes on the grid (East Asian wide characters and emoji take two).
func cellCount(_ s: String) -> Int {
    s.reduce(0) { $0 + cellWidth($1) }
}

func cellWidth(_ c: Character) -> Int {
    guard let v = c.unicodeScalars.first else { return 1 }
    if v.properties.isEmojiPresentation { return 2 }
    switch v.value {
    case 0x1100...0x115F, 0x2E80...0x303E, 0x3041...0x33FF, 0x3400...0x4DBF, 0x4E00...0x9FFF, 0xA000...0xA4CF,
         0xAC00...0xD7A3, 0xF900...0xFAFF, 0xFE30...0xFE4F, 0xFF00...0xFF60, 0xFFE0...0xFFE6, 0x20000...0x3FFFD:
        return 2
    default:
        return 1
    }
}

/// Text in a palette color that nearly matches its background (white on a light
/// theme, say) is drawn in the default foreground instead. Equal colors are kept:
/// that's a program hiding text on purpose.
func readable(_ fg: UInt32, on bg: UInt32, fallback: UInt32) -> UInt32 {
    guard fg & 0xFFFFFF != bg & 0xFFFFFF else { return fg }
    func lum(_ c: UInt32) -> Double {
        func ch(_ s: UInt32) -> Double {
            let v = Double((c >> s) & 0xFF) / 255
            return v <= 0.03928 ? v / 12.92 : pow((v + 0.055) / 1.055, 2.4)
        }
        return 0.2126 * ch(16) + 0.7152 * ch(8) + 0.0722 * ch(0)
    }
    let a = lum(fg), b = lum(bg)
    return (max(a, b) + 0.05) / (min(a, b) + 0.05) < 1.35 ? fallback : fg
}

func blend(_ a: UInt32, _ b: UInt32, _ t: Double) -> UInt32 {
    func ch(_ c: UInt32, _ s: UInt32) -> Double { Double((c >> s) & 0xFF) }
    func mix(_ s: UInt32) -> UInt32 { UInt32((ch(a, s) * (1 - t) + ch(b, s) * t).rounded()) }
    return 0xFF00_0000 | mix(16) << 16 | mix(8) << 8 | mix(0)
}

extension Array {
    subscript(safe i: Int) -> Element? { indices.contains(i) ? self[i] : nil }
}
