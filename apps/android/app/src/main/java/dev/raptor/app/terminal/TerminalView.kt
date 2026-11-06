package dev.raptor.app.terminal

import android.annotation.SuppressLint
import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.graphics.Canvas
import android.graphics.Paint
import android.graphics.RectF
import android.graphics.Typeface
import android.text.InputType
import android.view.ActionMode
import android.view.GestureDetector
import android.view.HapticFeedbackConstants
import android.view.KeyCharacterMap
import android.view.KeyEvent
import android.view.Menu
import android.view.MenuItem
import android.view.MotionEvent
import android.view.ScaleGestureDetector
import android.view.View
import android.view.inputmethod.BaseInputConnection
import android.view.inputmethod.EditorInfo
import android.view.inputmethod.InputConnection
import android.view.inputmethod.InputMethodManager
import android.widget.OverScroller
import androidx.core.content.res.ResourcesCompat
import dev.raptor.app.R
import uniffi.raptor_ffi.CursorShape
import uniffi.raptor_ffi.Frame
import uniffi.raptor_ffi.Key
import uniffi.raptor_ffi.Terminal
import kotlin.math.abs
import kotlin.math.floor
import kotlin.math.max
import kotlin.math.min
import kotlin.math.roundToInt

private const val STYLE_BOLD = 1
private const val STYLE_ITALIC = 1 shl 1
private const val STYLE_FAINT = 1 shl 2
private const val STYLE_STRIKE = 1 shl 3
private const val STYLE_OVERLINE = 1 shl 4
private const val STYLE_SELECTED = 1 shl 5

/**
 * Lines for U+2500..U+254F and U+2574..U+257F, four chars per code point: up,
 * right, down, left, each 0 (none), 1 (light) or 2 (heavy). Dots mean "use the font".
 */
private const val BOX_LINES =
    "0101020210102020................................0110021001200220001100120021002211001200210022001001100220012002111012102110112021202210122022201011101220111021202120121022202201110112021102120121012202210222110111021201120221012102220122021111111212111212211111212121211222111122122122121222212222212222................"
private const val BOX_HALF = "000110000100001000022000020000200201102001022010"
private const val QUADRANTS = "481D97B26E"

private fun boxLines(cp: Int): String? {
    val s = when (cp) {
        in 0x2500..0x254F -> BOX_LINES.substring((cp - 0x2500) * 4, (cp - 0x2500) * 4 + 4)
        in 0x2574..0x257F -> BOX_HALF.substring((cp - 0x2574) * 4, (cp - 0x2574) * 4 + 4)
        else -> return null
    }
    return s.takeIf { it[0] != '.' }
}

/** Whether the cell ending at [end] and the next one are parts of one emoji. */
private fun joinsNext(s: String, start: Int, end: Int): Boolean {
    val next = s.codePointAt(end)
    if (next in 0x1F3FB..0x1F3FF || s.codePointBefore(end) == 0x200D) return true
    return end - start == 2 && s.codePointAt(start) in 0x1F1E6..0x1F1FF && next in 0x1F1E6..0x1F1FF
}

/**
 * End of the cell that starts at [i]: one code point plus whatever the terminal
 * attaches to it (combining marks, joiners, variation selectors). The core
 * gives each other code point its own cell, emoji modifiers and flags included.
 */
private fun cellEnd(s: String, i: Int): Int {
    var end = i + Character.charCount(s.codePointAt(i))
    while (end < s.length) {
        val cp = s.codePointAt(end)
        val t = Character.getType(cp)
        val attaches = t == Character.NON_SPACING_MARK.toInt() || t == Character.ENCLOSING_MARK.toInt() ||
            t == Character.COMBINING_SPACING_MARK.toInt() || cp == 0x200D || cp in 0xFE00..0xFE0F || cp in 0xE0100..0xE01EF
        if (!attaches) break
        end += Character.charCount(cp)
    }
    return end
}

/** Sticky modifiers from the on-screen key bar, applied to the next key typed. */
class Modifiers {
    var ctrl = false
    var alt = false
    var ctrlLocked = false
    var altLocked = false
    var onChange: (() -> Unit)? = null

    fun consume(): Pair<Boolean, Boolean> {
        val r = ctrl to alt
        if (!ctrlLocked) ctrl = false
        if (!altLocked) alt = false
        if (r.first || r.second) onChange?.invoke()
        return r
    }
}

/**
 * Draws a terminal frame and turns touches and keys into terminal input.
 *
 * Two size modes:
 *  - **control**: this phone owns the terminal; its grid is sized to the view.
 *  - **mirror**: someone else (the desktop) owns the size; we scale to fit the
 *    width and let you pinch to zoom and pan.
 */
@SuppressLint("ViewConstructor")
class TerminalView(context: Context, val modifiers: Modifiers) : View(context) {
    var terminal: Terminal? = null
        set(value) {
            field = value
            frame = null
            requestLayout()
            invalidate()
        }

    /** Points; the user changes it by pinching. */
    var fontSize: Float = 13f
        set(value) {
            field = value.coerceIn(7f, 28f)
            updateMetrics()
            pushDeviceGrid()
            invalidate()
        }
    var onFontSizeChanged: ((Float) -> Unit)? = null
    var onRequestKeyboard: (() -> Unit)? = null
    /** Horizontal swipe / Ctrl+Tab: +1 next pane, -1 previous. */
    var onSwipePane: ((Int) -> Unit)? = null
    /** Ctrl+Shift+P on a hardware keyboard. */
    var onPalette: (() -> Unit)? = null
    var control = false
        set(value) {
            field = value
            zoom = 1f
            panX = 0f
            invalidate()
        }

    private var frame: Frame? = null
    private val density = resources.displayMetrics.density
    private var typefaces = TermFonts.byId(null).typefaces(context)
    private var lineHeight = 1.08f
    /** Forced cursor shape from Settings, or null to follow the program. */
    private var cursorOverride: CursorShape? = null
    private var blinkAllowed = true
    private val text = Paint(Paint.ANTI_ALIAS_FLAG or Paint.SUBPIXEL_TEXT_FLAG).apply { isLinearText = true }
    private val fill = Paint()
    private val line = Paint(Paint.ANTI_ALIAS_FLAG)
    private var cellW = 8f
    private var cellH = 16f
    private var baseline = 12f
    private val padding = 6f * density

    // Mirror-mode zoom and pan.
    private var zoom = 1f
    private var panX = 0f

    // Scrolling.
    private val scroller = OverScroller(context)
    private var scrollRemainder = 0f
    private var lastFlingY = 0

    // Cursor blink.
    private var blinkOn = true
    private val blink = object : Runnable {
        override fun run() {
            blinkOn = !blinkOn
            invalidate()
            if (shouldBlink()) postDelayed(this, 530) else blinkOn = true
        }
    }

    private fun shouldBlink() =
        blinkAllowed && hasFocus() && hasWindowFocus() && windowVisibility == VISIBLE && frame?.cursor?.blinking == true

    /** Show the cursor solid and start blinking again, as after a keypress. */
    private fun restartBlink() {
        removeCallbacks(blink)
        if (!blinkOn) invalidate()
        blinkOn = true
        if (shouldBlink()) postDelayed(blink, 530)
    }

    /** A frame arrived while we weren't visible; draw it when we are. */
    private var stale = false

    /** Text the keyboard is still composing (CJK, voice, handwriting): drawn at the cursor, sent when committed. */
    private var composing = ""
        set(v) {
            if (field != v) {
                field = v
                invalidate()
            }
        }

    // Selection.
    private var selecting = false
    private var selAnchor: Pair<Int, Int>? = null
    private var actionMode: ActionMode? = null

    init {
        isFocusable = true
        isFocusableInTouchMode = true
        defaultFocusHighlightEnabled = false
        updateMetrics()
    }

    private fun updateMetrics() {
        text.textSize = fontSize * density
        text.typeface = typefaces[0]
        cellW = text.measureText("M")
        val fm = text.fontMetrics
        cellH = (fm.descent - fm.ascent + fm.leading).let { floor(it * lineHeight) }
        baseline = -fm.ascent + (cellH - (fm.descent - fm.ascent)) / 2f
    }

    /** Font, line spacing and cursor from Settings; applies to the open terminal. */
    /**
     * Raw keys (the default) or a keyboard that can compose: voice typing, swipe,
     * CJK. Composing keyboards also bring their own habits, like a period on double space.
     */
    var smartKeyboard = false
        set(v) {
            if (field == v) return
            field = v
            context.getSystemService(InputMethodManager::class.java).restartInput(this)
        }

    fun applyAppearance(fontId: String?, lineHeight: Float, cursor: String?, blink: Boolean) {
        typefaces = TermFonts.byId(fontId).typefaces(context)
        this.lineHeight = lineHeight.coerceIn(1.0f, 1.6f)
        cursorOverride = when (cursor) {
            "block" -> CursorShape.BLOCK
            "bar" -> CursorShape.BAR
            "underline" -> CursorShape.UNDERLINE
            else -> null
        }
        blinkAllowed = blink
        restartBlink()
        updateMetrics()
        pushDeviceGrid()
        requestLayout()
        invalidate()
    }

    /**
     * Vertical shift (px) applied when the grid is taller than the view — e.g. the
     * keyboard just opened and the remote hasn't resized yet, or we're mirroring a
     * taller desktop terminal. Keeps the cursor row (where you type) on screen.
     */
    private fun offsetY(): Float {
        val f = frame ?: return 0f
        val s = scale()
        val visible = height - insetTop - insetBottom
        val contentH = f.rows.toInt() * cellH * s + padding * 2
        // A fitted grid shorter than the view is waiting for the server to grow it
        // (the keyboard just closed): keep its bottom row where it was, so the
        // prompt doesn't jump up and back down.
        if (control && contentH < visible - cellH) return contentH - visible
        if (contentH <= visible) return 0f
        val anchorRow = (f.cursor?.row?.toInt() ?: (f.rows.toInt() - 1)) + 1
        val needBottom = anchorRow * cellH * s + padding * 2
        return (needBottom - visible).coerceIn(0f, contentH - visible)
    }

    /** Scale applied to the remote grid so it fits (mirror mode). */
    private fun scale(): Float {
        val f = frame ?: return 1f
        if (control) return 1f
        val needW = f.cols.toInt() * cellW + padding * 2
        val fit = min(1f, width / needW)
        return fit * zoom
    }

    fun deviceGrid(): Pair<Int, Int> {
        val cols = max(10, ((width - padding * 2) / cellW).toInt())
        val rows = max(4, ((height - insetTop - insetBottom - padding * 2) / cellH).toInt())
        return cols to rows
    }

    private fun pushDeviceGrid() {
        val t = terminal ?: return
        if (width == 0 || height == 0) return
        val (c, r) = deviceGrid()
        t.setDeviceGrid(c.toUShort(), r.toUShort(), cellW.roundToInt().toUInt(), cellH.roundToInt().toUInt())
    }

    override fun onSizeChanged(w: Int, h: Int, oldw: Int, oldh: Int) {
        super.onSizeChanged(w, h, oldw, oldh)
        pushDeviceGrid()
    }

    /** Called on the next vsync after the core says a frame is ready. */
    fun refresh() {
        val t = terminal ?: return
        // In the background, skip the work: output can stream for hours.
        if (windowVisibility != VISIBLE) {
            stale = true
            return
        }
        stale = false
        val f = t.frame()
        val prev = frame
        frame = f
        if (prev == null || prev.background != f.background || prev.foreground != f.foreground) {
            onColors?.invoke(f.background.toInt(), f.foreground.toInt())
        }
        if (prev?.cursor?.blinking != f.cursor?.blinking) restartBlink()
        invalidate()
    }

    /** Called when the terminal's default colors change, so chrome can match. */
    var onColors: ((bg: Int, fg: Int) -> Unit)? = null

    /** Space covered by floating chrome; the grid is sized to what's left. */
    var insetTop = 0f
        set(v) { field = v; pushDeviceGrid(); invalidate() }
    var insetBottom = 0f
        set(v) { field = v; pushDeviceGrid(); invalidate() }

    override fun onWindowVisibilityChanged(visibility: Int) {
        super.onWindowVisibilityChanged(visibility)
        if (visibility == VISIBLE && stale) refresh()
        restartBlink()
    }

    override fun onWindowFocusChanged(hasWindowFocus: Boolean) {
        super.onWindowFocusChanged(hasWindowFocus)
        restartBlink()
    }

    override fun onFocusChanged(gainFocus: Boolean, direction: Int, previouslyFocusedRect: android.graphics.Rect?) {
        super.onFocusChanged(gainFocus, direction, previouslyFocusedRect)
        if (!gainFocus) composing = ""
        restartBlink()
        invalidate()
    }

    override fun onDetachedFromWindow() {
        removeCallbacks(blink)
        actionMode?.finish()
        super.onDetachedFromWindow()
    }

    // ---- drawing ----------------------------------------------------------------

    // The top bar sits over the terminal like glass: live rows start below it,
    // and scrollback that moves up behind it shows through blurred.
    private val contentNode = if (android.os.Build.VERSION.SDK_INT >= 31) android.graphics.RenderNode("terminal") else null
    private val blurNode = if (android.os.Build.VERSION.SDK_INT >= 31) android.graphics.RenderNode("bar") else null
    private val barPaint = Paint()

    override fun onDraw(canvas: Canvas) {
        val bg = frame?.background?.toInt() ?: 0xFF0D1117.toInt()
        val fg = frame?.foreground?.toInt() ?: 0xFFFFFFFF.toInt()
        val bar = insetTop
        if (bar <= 0f) { drawContent(canvas); return }
        val node = contentNode
        val blur = blurNode
        if (node != null && blur != null && canvas.isHardwareAccelerated && android.os.Build.VERSION.SDK_INT >= 31) {
            node.setPosition(0, 0, width, height)
            drawContent(node.beginRecording())
            node.endRecording()
            canvas.drawRenderNode(node)
            blur.setPosition(0, 0, width, kotlin.math.ceil(bar).toInt())
            val r = 22f * density
            blur.setRenderEffect(android.graphics.RenderEffect.createBlurEffect(r, r, android.graphics.Shader.TileMode.CLAMP))
            blur.beginRecording().drawRenderNode(node)
            blur.endRecording()
            canvas.drawRenderNode(blur)
            barPaint.color = withAlpha(bg, 0.55f)
        } else {
            drawContent(canvas)
            barPaint.color = withAlpha(bg, 0.94f)
        }
        canvas.drawRect(0f, 0f, width.toFloat(), bar, barPaint)
        barPaint.color = withAlpha(fg, 0.10f)
        canvas.drawRect(0f, bar - density * 0.7f, width.toFloat(), bar, barPaint)
    }

    private fun withAlpha(c: Int, a: Float): Int = (c and 0x00FFFFFF) or ((a * 255).toInt() shl 24)

    private fun drawContent(canvas: Canvas) {
        val f = frame
        val bg = f?.background?.toInt() ?: 0xFF0D1117.toInt()
        // Compose hosts don't clip children; stay inside our own bounds.
        canvas.clipRect(0, 0, width, height)
        canvas.drawColor(bg)
        if (f == null) return
        val s = scale()
        canvas.save()
        canvas.translate(padding - panX, insetTop + padding - offsetY())
        canvas.scale(s, s)

        val fgDefault = f.foreground.toInt()
        val selColor = blend(fgDefault, bg, 0.30f)
        for ((rowIdx, l) in f.lines.withIndex()) {
            val top = rowIdx * cellH
            // Backgrounds first so glyph overhangs (italics) aren't clipped by neighbors.
            for (run in l.runs) {
                val selected = (run.style.toInt() and STYLE_SELECTED) != 0
                val b = when {
                    selected -> selColor
                    run.bg != null -> run.bg!!.toInt()
                    else -> continue
                }
                fill.color = b
                val x = run.col.toInt() * cellW
                canvas.drawRect(x, top, x + run.cells.toInt() * cellW, top + cellH + 0.5f, fill)
            }
            for (run in l.runs) {
                val style = run.style.toInt()
                val bold = style and STYLE_BOLD != 0
                val italic = style and STYLE_ITALIC != 0
                text.typeface = typefaces[(if (bold) 1 else 0) + (if (italic) 2 else 0)]
                var color = run.fg.toInt()
                if (style and STYLE_FAINT != 0) color = blend(color, bg, 0.5f)
                text.color = color
                val x0 = run.col.toInt() * cellW
                val y = top + baseline
                if (run.text.isNotBlank()) drawRunText(canvas, run.text, run.widths, x0, y)
                val ul = (style shr 8) and 0xF
                if (ul != 0 || style and (STYLE_STRIKE or STYLE_OVERLINE) != 0) {
                    line.color = run.underlineColor?.toInt() ?: color
                    line.strokeWidth = max(1f, density * 0.9f)
                    val x1 = x0 + run.cells.toInt() * cellW
                    if (ul != 0) {
                        val uy = top + cellH - line.strokeWidth * 1.5f
                        when (ul) {
                            2 -> { canvas.drawLine(x0, uy, x1, uy, line); canvas.drawLine(x0, uy - 3 * density, x1, uy - 3 * density, line) }
                            3 -> drawCurly(canvas, x0, x1, uy)
                            4 -> drawDashed(canvas, x0, x1, uy, 1.5f * density)
                            5 -> drawDashed(canvas, x0, x1, uy, 4f * density)
                            else -> canvas.drawLine(x0, uy, x1, uy, line)
                        }
                    }
                    if (style and STYLE_STRIKE != 0) canvas.drawLine(x0, top + cellH / 2, x1, top + cellH / 2, line)
                    if (style and STYLE_OVERLINE != 0) canvas.drawLine(x0, top + 1, x1, top + 1, line)
                }
            }
        }

        f.cursor?.let { c ->
            if (f.scrollOffset + f.scrollLen < f.scrollTotal) return@let // scrolled into history
            if (composing.isNotEmpty()) {
                drawComposing(canvas, c.col.toInt() * cellW, c.row.toInt() * cellH, bg, f.cursorColor?.toInt() ?: fgDefault)
                return@let
            }
            if (blinkAllowed && c.blinking && !blinkOn && hasFocus()) return@let
            val shape = cursorOverride ?: c.shape
            val cx = c.col.toInt() * cellW
            val cy = c.row.toInt() * cellH
            val w = if (c.wide) cellW * 2 else cellW
            fill.color = f.cursorColor?.toInt() ?: fgDefault
            when {
                c.password -> drawLock(canvas, cx, cy)
                !hasFocus() || shape == CursorShape.HOLLOW_BLOCK -> {
                    fill.style = Paint.Style.STROKE; fill.strokeWidth = density
                    canvas.drawRect(cx + 0.5f, cy + 0.5f, cx + w - 0.5f, cy + cellH - 0.5f, fill)
                    fill.style = Paint.Style.FILL
                }
                shape == CursorShape.BAR -> canvas.drawRect(cx, cy, cx + 2 * density, cy + cellH, fill)
                shape == CursorShape.UNDERLINE -> canvas.drawRect(cx, cy + cellH - 2 * density, cx + w, cy + cellH, fill)
                else -> {
                    canvas.drawRect(cx, cy, cx + w, cy + cellH, fill)
                    // Redraw the glyph under the block in the background color.
                    val run = f.lines.getOrNull(c.row.toInt())?.runs?.firstOrNull { c.col >= it.col && c.col < it.col + it.cells }
                    if (run != null) {
                        val idx = graphemeIndexAt(run.widths, (c.col - run.col).toInt())
                        val g = graphemeAt(run.text, idx)
                        if (g.isNotBlank()) {
                            text.color = bg
                            canvas.drawText(g, cx, cy + baseline, text)
                        }
                    }
                }
            }
        }
        canvas.restore()

        // Scroll position indicator while in history.
        if (f.scrollTotal > f.scrollLen && f.scrollOffset + f.scrollLen < f.scrollTotal) {
            val trackH = height - padding * 2
            val thumbH = max(24 * density, trackH * f.scrollLen.toFloat() / f.scrollTotal.toFloat())
            val y = padding + (trackH - thumbH) * f.scrollOffset.toFloat() / (f.scrollTotal - f.scrollLen).toFloat()
            fill.color = blend(fgDefault, bg, 0.55f)
            canvas.drawRoundRect(RectF(width - 5 * density, y, width - 2 * density, y + thumbH), 2 * density, 2 * density, fill)
        }
    }

    private fun drawRunText(canvas: Canvas, s: String, widths: ByteArray, x0: Float, y: Float) {
        // Fast path: Latin text the terminal font covers, one cell per char.
        if (s.length == widths.size && s.all { it.code < 0x250 }) {
            canvas.drawText(s, x0, y, text)
            return
        }
        // Place each cell's text on its own column so fallback fonts can't drift off the grid.
        var col = 0
        var i = 0
        var g = 0
        while (i < s.length) {
            var end = cellEnd(s, i)
            var w = widths.getOrNull(g++)?.toInt() ?: 1
            // Emoji the core splits over several cells (families, skin tones, flags) still draw as one picture.
            while (end < s.length && joinsNext(s, i, end)) {
                w += widths.getOrNull(g++)?.toInt() ?: 1
                end = cellEnd(s, end)
            }
            val x = x0 + col * cellW
            if (!drawBoxChar(canvas, s.codePointAt(i), x, y - baseline, w)) drawCell(canvas, s, i, end, x, y, w)
            col += w
            i = end
        }
    }

    /** Draws one cell's text, shrinking glyphs that are too wide for it (emoji in a narrow cell, symbols from fallback fonts). */
    private fun drawCell(canvas: Canvas, s: String, start: Int, end: Int, x: Float, y: Float, cells: Int) {
        if (end == start + 1 && s[start] == ' ') return
        val room = cellW * cells
        val w = text.measureText(s, start, end)
        if (w <= room * 1.05f) {
            canvas.drawText(s, start, end, if (cells > 1) x + (room - w) / 2 else x, y, text)
            return
        }
        canvas.save()
        val k = room / w
        canvas.scale(k, k, x, y - baseline + cellH / 2)
        canvas.drawText(s, start, end, x, y, text)
        canvas.restore()
    }

    private val boxPath = android.graphics.Path()

    /** Draw box-drawing, block and braille characters as geometry so they join seamlessly. */
    private fun drawBoxChar(canvas: Canvas, cp: Int, x: Float, top: Float, w: Int): Boolean {
        if (cp !in 0x2500..0x259F && cp !in 0x2800..0x28FF && cp !in 0xE0B0..0xE0B3) return false
        val cw = cellW * w
        val cx = x + cw / 2
        val cy = top + cellH / 2
        val r = x + cw
        val b = top + cellH
        fill.color = text.color
        val t = max(1f, (density * 0.9f).roundToInt().toFloat())
        val lines = boxLines(cp)
        if (lines != null) {
            fun th(c: Char) = when (c) { '1' -> t; '2' -> t * 2; else -> 0f }
            val (up, right, down, left) = lines.map(::th)
            val hv = max(up, down) / 2
            val hh = max(left, right) / 2
            if (left > 0) canvas.drawRect(x, cy - left / 2, cx + hv, cy + left / 2, fill)
            if (right > 0) canvas.drawRect(cx - hv, cy - right / 2, r, cy + right / 2, fill)
            if (up > 0) canvas.drawRect(cx - up / 2, top, cx + up / 2, cy + hh, fill)
            if (down > 0) canvas.drawRect(cx - down / 2, cy - hh, cx + down / 2, b, fill)
            return true
        }
        // Double lines: two light strokes with a light-stroke gap.
        val a = t / 2
        val o = t * 1.5f
        fun rect(l: Float, tp: Float, rt: Float, bt: Float) = canvas.drawRect(l, tp, rt, bt, fill)
        when (cp) {
            0x2550 -> { rect(x, cy - o, r, cy - a); rect(x, cy + a, r, cy + o) }
            0x2551 -> { rect(cx - o, top, cx - a, b); rect(cx + a, top, cx + o, b) }
            0x2554 -> { rect(cx - o, cy - o, r, cy - a); rect(cx - o, cy - o, cx - a, b); rect(cx + a, cy + a, r, cy + o); rect(cx + a, cy + a, cx + o, b) }
            0x2557 -> { rect(x, cy - o, cx + o, cy - a); rect(cx + a, cy - o, cx + o, b); rect(x, cy + a, cx - a, cy + o); rect(cx - o, cy + a, cx - a, b) }
            0x255A -> { rect(cx - o, cy + a, r, cy + o); rect(cx - o, top, cx - a, cy + o); rect(cx + a, cy - o, r, cy - a); rect(cx + a, top, cx + o, cy - a) }
            0x255D -> { rect(x, cy + a, cx + o, cy + o); rect(cx + a, top, cx + o, cy + o); rect(x, cy - o, cx - a, cy - a); rect(cx - o, top, cx - a, cy - a) }
            in 0x256D..0x2570 -> {
                // Rounded corners.
                val rad = min(cw, cellH) / 2
                boxPath.reset()
                when (cp) {
                    0x256D -> { boxPath.moveTo(cx, b); boxPath.lineTo(cx, cy + rad); boxPath.arcTo(cx, cy, cx + 2 * rad, cy + 2 * rad, 180f, 90f, false); boxPath.lineTo(r, cy) }
                    0x256E -> { boxPath.moveTo(x, cy); boxPath.lineTo(cx - rad, cy); boxPath.arcTo(cx - 2 * rad, cy, cx, cy + 2 * rad, 270f, 90f, false); boxPath.lineTo(cx, b) }
                    0x256F -> { boxPath.moveTo(cx, top); boxPath.lineTo(cx, cy - rad); boxPath.arcTo(cx - 2 * rad, cy - 2 * rad, cx, cy, 0f, 90f, false); boxPath.lineTo(x, cy) }
                    else -> { boxPath.moveTo(cx, top); boxPath.lineTo(cx, cy - rad); boxPath.arcTo(cx, cy - 2 * rad, cx + 2 * rad, cy, 180f, -90f, false); boxPath.lineTo(r, cy) }
                }
                line.color = fill.color
                line.style = Paint.Style.STROKE
                line.strokeWidth = t
                canvas.drawPath(boxPath, line)
            }
            in 0xE0B0..0xE0B3 -> {
                // Powerline separators, full cell height so prompts join without seams.
                boxPath.reset()
                if (cp < 0xE0B2) { boxPath.moveTo(x, top); boxPath.lineTo(r, cy); boxPath.lineTo(x, b) } else { boxPath.moveTo(r, top); boxPath.lineTo(x, cy); boxPath.lineTo(r, b) }
                line.color = fill.color
                line.strokeWidth = t
                line.style = if (cp % 2 == 0) Paint.Style.FILL else Paint.Style.STROKE
                if (cp % 2 == 0) boxPath.close()
                canvas.drawPath(boxPath, line)
            }
            0x2580 -> rect(x, top, r, cy)
            0x2588 -> rect(x, top, r, b)
            0x2590 -> rect(cx, top, r, b)
            0x2594 -> rect(x, top, r, top + cellH / 8)
            0x2595 -> rect(r - cw / 8, top, r, b)
            0x2591, 0x2592, 0x2593 -> {
                val alpha = fill.alpha
                fill.alpha = when (cp) { 0x2591 -> 64; 0x2592 -> 128; else -> 192 }
                rect(x, top, r, b)
                fill.alpha = alpha
            }
            in 0x2581..0x2587 -> rect(x, b - cellH * (cp - 0x2580) / 8f, r, b)
            in 0x2589..0x258F -> rect(x, top, x + cw * (0x2590 - cp) / 8f, b)
            in 0x2596..0x259F -> {
                // Quadrants: bits are upper left, upper right, lower left, lower right.
                val q = QUADRANTS[cp - 0x2596].digitToInt(16)
                if (q and 1 != 0) rect(x, top, cx, cy)
                if (q and 2 != 0) rect(cx, top, r, cy)
                if (q and 4 != 0) rect(x, cy, cx, b)
                if (q and 8 != 0) rect(cx, cy, r, b)
            }
            in 0x2800..0x28FF -> {
                // Braille: two columns of four dots (used by graphs in btop and friends).
                val bits = cp - 0x2800
                val dot = min(cw / 4, cellH / 8) * 0.8f
                for (i in 0 until 8) {
                    if (bits and (1 shl i) == 0) continue
                    val c = if (i in 3..5 || i == 7) 1 else 0
                    val rw = when (i) { 6, 7 -> 3; in 3..5 -> i - 3; else -> i }
                    canvas.drawCircle(x + cw * (1 + 2 * c) / 4, top + cellH * (1 + 2 * rw) / 8, dot, fill)
                }
            }
            else -> return false
        }
        return true
    }

    private fun drawComposing(canvas: Canvas, x: Float, top: Float, bg: Int, fg: Int) {
        text.typeface = typefaces[0]
        text.color = fg
        val w = text.measureText(composing)
        fill.color = bg
        canvas.drawRect(x, top, x + w, top + cellH, fill)
        canvas.drawText(composing, x, top + baseline, text)
        line.color = fg
        line.strokeWidth = max(1f, density)
        canvas.drawLine(x, top + cellH - density, x + w, top + cellH - density, line)
        fill.color = fg
        canvas.drawRect(x + w, top, x + w + 2 * density, top + cellH, fill)
    }

    private fun drawCurly(canvas: Canvas, x0: Float, x1: Float, y: Float) {
        val amp = 1.5f * density
        val step = 3f * density
        var x = x0
        var up = true
        while (x < x1) {
            val nx = min(x + step, x1)
            canvas.drawLine(x, y + (if (up) amp else -amp), nx, y + (if (up) -amp else amp), line)
            x = nx
            up = !up
        }
    }

    private fun drawDashed(canvas: Canvas, x0: Float, x1: Float, y: Float, dash: Float) {
        var x = x0
        while (x < x1) {
            canvas.drawLine(x, y, min(x + dash, x1), y, line)
            x += dash * 2
        }
    }

    private fun drawLock(canvas: Canvas, x: Float, y: Float) {
        val w = cellW
        fill.style = Paint.Style.FILL
        canvas.drawRoundRect(RectF(x + w * 0.15f, y + cellH * 0.45f, x + w * 0.85f, y + cellH * 0.9f), density, density, fill)
        fill.style = Paint.Style.STROKE
        fill.strokeWidth = density * 1.2f
        canvas.drawArc(RectF(x + w * 0.28f, y + cellH * 0.15f, x + w * 0.72f, y + cellH * 0.65f), 180f, 180f, false, fill)
        fill.style = Paint.Style.FILL
    }

    private fun graphemeIndexAt(widths: ByteArray, cellOffset: Int): Int {
        var c = 0
        for ((i, w) in widths.withIndex()) {
            if (cellOffset < c + w) return i
            c += w
        }
        return widths.size - 1
    }

    private fun graphemeAt(s: String, index: Int): String {
        var i = 0
        var g = 0
        while (i < s.length) {
            val end = cellEnd(s, i)
            if (g == index) return s.substring(i, end)
            i = end
            g++
        }
        return ""
    }

    private fun blend(a: Int, b: Int, t: Float): Int {
        fun ch(c: Int, s: Int) = (c shr s) and 0xFF
        fun mix(s: Int) = (ch(a, s) * (1 - t) + ch(b, s) * t).roundToInt()
        return (0xFF shl 24) or (mix(16) shl 16) or (mix(8) shl 8) or mix(0)
    }

    // ---- touch ------------------------------------------------------------------

    private fun cellAt(x: Float, y: Float): Pair<Int, Int> {
        val s = scale()
        val col = ((x - padding + panX) / s / cellW).toInt()
        val row = ((y - insetTop - padding + offsetY()) / s / cellH).toInt()
        val f = frame
        return col.coerceIn(0, (f?.cols?.toInt() ?: 1) - 1) to row.coerceIn(0, (f?.rows?.toInt() ?: 1) - 1)
    }

    private val gestures = GestureDetector(context, object : GestureDetector.SimpleOnGestureListener() {
        override fun onDown(e: MotionEvent): Boolean {
            scroller.forceFinished(true)
            return true
        }

        override fun onSingleTapUp(e: MotionEvent): Boolean {
            if (actionMode != null) {
                terminal?.clearSelection()
                actionMode?.finish()
                return true
            }
            val t = terminal ?: return true
            if (t.mouseTracking()) {
                val (c, r) = cellAt(e.x, e.y)
                t.tap(c.toUShort(), r.toUShort())
            }
            toggleKeyboard()
            return true
        }

        override fun onDoubleTap(e: MotionEvent): Boolean {
            val (c, r) = cellAt(e.x, e.y)
            if (terminal?.selectWord(c.toUShort(), r.toUShort()) == true) {
                performHapticFeedback(HapticFeedbackConstants.TEXT_HANDLE_MOVE)
                startSelectionMode()
            }
            return true
        }

        // Long-press selects the word under your finger (drag to extend), and
        // anywhere it offers the menu, so Paste is always one gesture away.
        override fun onLongPress(e: MotionEvent) {
            val (c, r) = cellAt(e.x, e.y)
            terminal?.selectWord(c.toUShort(), r.toUShort())
            selecting = true
            selAnchor = c to r
            performHapticFeedback(HapticFeedbackConstants.LONG_PRESS)
            startSelectionMode()
        }

        override fun onScroll(e1: MotionEvent?, e2: MotionEvent, dx: Float, dy: Float): Boolean {
            if (selecting) return true
            val canPan = (!control && scale() < 1f) || zoom > 1f
            if (abs(dx) > abs(dy)) {
                if (canPan) pan(dx)
                return true // horizontal drags are pane swipes, decided on fling
            }
            scrollBy(dy)
            return true
        }

        override fun onFling(e1: MotionEvent?, e2: MotionEvent, vx: Float, vy: Float): Boolean {
            if (selecting) return true
            // A horizontal fling switches panes (unless we're panning a zoomed/mirrored grid).
            val canPan = (!control && scale() < 1f) || zoom > 1f
            if (!canPan && abs(vx) > abs(vy) * 1.3f && abs(vx) > 900f) {
                performHapticFeedback(HapticFeedbackConstants.CONTEXT_CLICK)
                onSwipePane?.invoke(if (vx < 0) 1 else -1)
                return true
            }
            lastFlingY = 0
            scroller.fling(0, 0, 0, vy.toInt(), 0, 0, -100_000, 100_000)
            postInvalidateOnAnimation()
            return true
        }
    })

    private val scaleDetector = ScaleGestureDetector(context, object : ScaleGestureDetector.SimpleOnScaleGestureListener() {
        override fun onScale(d: ScaleGestureDetector): Boolean {
            if (control) {
                fontSize *= d.scaleFactor
            } else {
                zoom = (zoom * d.scaleFactor).coerceIn(1f, 4f)
                clampPan()
                invalidate()
            }
            return true
        }

        override fun onScaleEnd(d: ScaleGestureDetector) {
            if (control) onFontSizeChanged?.invoke(fontSize)
        }
    })

    private fun pan(dx: Float) {
        panX += dx
        clampPan()
        invalidate()
    }

    private fun clampPan() {
        val f = frame ?: return
        val contentW = f.cols.toInt() * cellW * scale() + padding * 2
        panX = panX.coerceIn(0f, max(0f, contentW - width))
    }

    private fun scrollBy(dyPx: Float) {
        val t = terminal ?: return
        scrollRemainder += dyPx / (cellH * scale())
        val rows = scrollRemainder.toInt()
        if (rows == 0) return
        scrollRemainder -= rows
        if (t.wantsWheel() || frame?.alternateScreen == true) {
            val (c, r) = cellAt(width / 2f, height / 2f)
            t.wheel(c.toUShort(), r.toUShort(), rows.toFloat())
        } else {
            t.scroll(rows)
        }
    }

    override fun computeScroll() {
        if (scroller.computeScrollOffset()) {
            val y = scroller.currY
            val d = y - lastFlingY
            lastFlingY = y
            scrollBy(-d.toFloat())
            postInvalidateOnAnimation()
        }
    }

    @SuppressLint("ClickableViewAccessibility")
    override fun onTouchEvent(event: MotionEvent): Boolean {
        scaleDetector.onTouchEvent(event)
        if (selecting && event.actionMasked == MotionEvent.ACTION_MOVE) {
            val a = selAnchor
            if (a != null) {
                val (c, r) = cellAt(event.x, event.y)
                terminal?.selectRange(a.first.toUShort(), a.second.toUShort(), c.toUShort(), r.toUShort())
            }
            return true
        }
        if (event.actionMasked == MotionEvent.ACTION_UP || event.actionMasked == MotionEvent.ACTION_CANCEL) {
            if (selecting) actionMode?.invalidate()
            selecting = false
        }
        if (!scaleDetector.isInProgress) gestures.onTouchEvent(event)
        return true
    }

    private fun startSelectionMode() {
        if (actionMode != null) {
            actionMode?.invalidate()
            actionMode?.invalidateContentRect()
            return
        }
        actionMode = startActionMode(object : ActionMode.Callback2() {
            override fun onCreateActionMode(mode: ActionMode, menu: Menu): Boolean {
                menu.add(0, 1, 0, android.R.string.copy)
                menu.add(0, 2, 1, android.R.string.paste)
                menu.add(0, 3, 2, "Copy All")
                return true
            }

            override fun onPrepareActionMode(mode: ActionMode, menu: Menu): Boolean {
                menu.findItem(1).isVisible = terminal?.selectionText()?.isNotEmpty() == true
                menu.findItem(2).isVisible = context.getSystemService(ClipboardManager::class.java).hasPrimaryClip()
                return true
            }

            override fun onActionItemClicked(mode: ActionMode, item: MenuItem): Boolean {
                when (item.itemId) {
                    1 -> terminal?.selectionText()?.let { copy(it) }
                    2 -> pasteClipboard()
                    3 -> {
                        val all = terminal?.allText().orEmpty()
                        if (all.isNotEmpty()) copy(all)
                    }
                }
                terminal?.clearSelection()
                mode.finish()
                return true
            }

            override fun onDestroyActionMode(mode: ActionMode) {
                actionMode = null
                terminal?.clearSelection()
            }

            override fun onGetContentRect(mode: ActionMode, view: View, outRect: android.graphics.Rect) {
                val a = selAnchor ?: (0 to 0)
                val s = scale()
                val y = (insetTop + padding + a.second * cellH * s - offsetY()).toInt()
                outRect.set(0, y, width, (y + cellH * s).toInt())
            }
        }, ActionMode.TYPE_FLOATING)
    }

    fun copy(s: String) {
        val cm = context.getSystemService(ClipboardManager::class.java)
        cm.setPrimaryClip(ClipData.newPlainText("Terminal", s))
    }

    fun pasteClipboard() {
        val cm = context.getSystemService(ClipboardManager::class.java)
        val s = cm.primaryClip?.getItemAt(0)?.coerceToText(context)?.toString() ?: return
        terminal?.paste(s)
    }

    // ---- keyboard ---------------------------------------------------------------

    override fun onCheckIsTextEditor() = true

    // Screen readers get the visible screen as text.
    override fun onInitializeAccessibilityNodeInfo(info: android.view.accessibility.AccessibilityNodeInfo) {
        super.onInitializeAccessibilityNodeInfo(info)
        info.className = "android.widget.EditText"
        info.isEditable = true
        info.hintText = "Terminal"
        info.text = frame?.lines?.joinToString("\n") { l ->
            buildString {
                for (r in l.runs) {
                    while (length < r.col.toInt()) append(' ')
                    append(r.text)
                }
            }.trimEnd()
        }?.trimEnd()
    }

    override fun onCreateInputConnection(outAttrs: EditorInfo): InputConnection {
        // Visible-password text gets raw characters from most keyboards: no autocorrect,
        // no composing, a number row. The smart mode lets them compose.
        outAttrs.inputType = InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_FLAG_NO_SUGGESTIONS or
            if (smartKeyboard) 0 else InputType.TYPE_TEXT_VARIATION_VISIBLE_PASSWORD
        outAttrs.imeOptions = EditorInfo.IME_FLAG_NO_FULLSCREEN or EditorInfo.IME_FLAG_NO_EXTRACT_UI or EditorInfo.IME_ACTION_NONE
        outAttrs.initialSelStart = 0
        outAttrs.initialSelEnd = 0
        composing = ""
        return object : BaseInputConnection(this, false) {
            override fun commitText(text: CharSequence, newCursorPosition: Int): Boolean {
                composing = ""
                typeText(text.toString())
                return true
            }

            override fun setComposingText(text: CharSequence, newCursorPosition: Int): Boolean {
                composing = text.toString()
                return true
            }

            override fun setComposingRegion(start: Int, end: Int): Boolean = true

            // Keyboards rewrite what they just typed (Gboard's period on double space).
            override fun replaceText(start: Int, end: Int, text: CharSequence, newCursorPosition: Int, textAttribute: android.view.inputmethod.TextAttribute?): Boolean {
                composing = ""
                repeat(end - start) { key(Key.Backspace) }
                typeText(text.toString())
                return true
            }

            override fun finishComposingText(): Boolean {
                val c = composing
                composing = ""
                if (c.isNotEmpty()) typeText(c)
                return true
            }

            // We never report text around the cursor, so keyboards send backspace as a key
            // on an empty line; when they do delete text they think is there, pass it on.
            override fun getTextBeforeCursor(n: Int, flags: Int): CharSequence = ""

            override fun getTextAfterCursor(n: Int, flags: Int): CharSequence = ""

            override fun deleteSurroundingText(beforeLength: Int, afterLength: Int): Boolean {
                repeat(beforeLength) { key(Key.Backspace) }
                repeat(afterLength) { key(Key.Delete) }
                return true
            }

            override fun deleteSurroundingTextInCodePoints(beforeLength: Int, afterLength: Int) = deleteSurroundingText(beforeLength, afterLength)

            override fun performEditorAction(actionCode: Int): Boolean {
                key(Key.Enter)
                return true
            }

            override fun sendKeyEvent(event: KeyEvent): Boolean {
                if (event.action == KeyEvent.ACTION_DOWN) handleKey(event.keyCode, event)
                return true
            }
        }
    }

    private fun typeText(s: String) {
        val t = terminal ?: return
        restartBlink()
        for (part in s.split('\n').withIndex()) {
            if (part.index > 0) key(Key.Enter)
            val str = part.value
            if (str.isEmpty()) continue
            val (ctrl, alt) = modifiers.consume()
            if (ctrl || alt) {
                key(Key.Char(str.substring(0, 1)), ctrl, alt)
                if (str.length > 1) t.sendText(str.substring(1))
            } else {
                t.sendText(str)
            }
        }
    }

    fun key(k: Key, ctrl: Boolean = false, alt: Boolean = false, shift: Boolean = false) {
        val t = terminal ?: return
        restartBlink()
        val (sc, sa) = if (!ctrl && !alt) modifiers.consume() else (false to false)
        t.sendKey(k, ctrl || sc, alt || sa, shift, false)
    }

    override fun onKeyDown(keyCode: Int, event: KeyEvent): Boolean = handleKey(keyCode, event) || super.onKeyDown(keyCode, event)

    /** A dead key (accent) waiting for the letter it goes on. */
    private var deadAccent = 0

    private fun handleKey(keyCode: Int, e: KeyEvent): Boolean {
        val t = terminal ?: return false
        val ctrl = e.isCtrlPressed
        val alt = e.isAltPressed
        val shift = e.isShiftPressed
        val meta = e.isMetaPressed
        restartBlink()
        // Hardware-keyboard shortcuts that belong to the app, not the shell.
        if (ctrl && shift && keyCode == KeyEvent.KEYCODE_V) { pasteClipboard(); return true }
        if (ctrl && shift && keyCode == KeyEvent.KEYCODE_C) { t.selectionText()?.let(::copy); return true }
        if (ctrl && shift && keyCode == KeyEvent.KEYCODE_P) { onPalette?.invoke(); return true }
        if (ctrl && keyCode == KeyEvent.KEYCODE_TAB) { onSwipePane?.invoke(if (shift) -1 else 1); return true }
        val k: Key? = when (keyCode) {
            KeyEvent.KEYCODE_ENTER, KeyEvent.KEYCODE_NUMPAD_ENTER -> Key.Enter
            KeyEvent.KEYCODE_TAB -> Key.Tab
            KeyEvent.KEYCODE_DEL -> Key.Backspace
            KeyEvent.KEYCODE_FORWARD_DEL -> Key.Delete
            KeyEvent.KEYCODE_ESCAPE -> Key.Escape
            KeyEvent.KEYCODE_DPAD_UP -> Key.Up
            KeyEvent.KEYCODE_DPAD_DOWN -> Key.Down
            KeyEvent.KEYCODE_DPAD_LEFT -> Key.Left
            KeyEvent.KEYCODE_DPAD_RIGHT -> Key.Right
            KeyEvent.KEYCODE_MOVE_HOME -> Key.Home
            KeyEvent.KEYCODE_MOVE_END -> Key.End
            KeyEvent.KEYCODE_PAGE_UP -> Key.PageUp
            KeyEvent.KEYCODE_PAGE_DOWN -> Key.PageDown
            KeyEvent.KEYCODE_INSERT -> Key.Insert
            in KeyEvent.KEYCODE_F1..KeyEvent.KEYCODE_F12 -> Key.F((keyCode - KeyEvent.KEYCODE_F1 + 1).toUByte())
            else -> null
        }
        if (k != null) {
            val (sc, sa) = modifiers.consume()
            t.sendKey(k, ctrl || sc, alt || sa, shift, meta)
            return true
        }
        val noMods = e.metaState and (KeyEvent.META_CTRL_MASK or KeyEvent.META_ALT_MASK or KeyEvent.META_META_MASK).inv()
        var uc = e.getUnicodeChar(noMods)
        // AltGr (right Alt) types characters on many layouts; send those as text.
        if (e.metaState and KeyEvent.META_ALT_RIGHT_ON != 0 && !ctrl) {
            val altGr = e.getUnicodeChar(e.metaState and (KeyEvent.META_CTRL_MASK or KeyEvent.META_META_MASK).inv())
            if (altGr != 0 && altGr != uc) {
                t.sendText(String(Character.toChars(altGr)))
                return true
            }
        }
        if (uc and KeyCharacterMap.COMBINING_ACCENT != 0) {
            deadAccent = uc and KeyCharacterMap.COMBINING_ACCENT_MASK
            return true
        }
        if (uc == 0) return false
        if (deadAccent != 0) {
            KeyEvent.getDeadChar(deadAccent, uc).takeIf { it != 0 }?.let { uc = it }
            deadAccent = 0
        }
        val ch = String(Character.toChars(uc))
        val (sc, sa) = modifiers.consume()
        if (ctrl || alt || meta || sc || sa) t.sendKey(Key.Char(ch), ctrl || sc, alt || sa, false, meta) else t.sendText(ch)
        return true
    }

    fun showKeyboard() {
        requestFocus()
        val imm = context.getSystemService(InputMethodManager::class.java)
        imm.showSoftInput(this, InputMethodManager.SHOW_IMPLICIT)
    }

    fun keyboardVisible(): Boolean =
        rootWindowInsets?.let { androidx.core.view.WindowInsetsCompat.toWindowInsetsCompat(it, this).isVisible(androidx.core.view.WindowInsetsCompat.Type.ime()) } ?: false

    fun toggleKeyboard() {
        if (keyboardVisible()) hideKeyboard() else showKeyboard()
    }

    fun hideKeyboard() {
        val imm = context.getSystemService(InputMethodManager::class.java)
        imm.hideSoftInputFromWindow(windowToken, 0)
    }
}
