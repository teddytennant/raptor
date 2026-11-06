package dev.raptor.app.ui

import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.core.tween
import androidx.compose.animation.slideInHorizontally
import androidx.compose.animation.slideOutHorizontally
import androidx.compose.animation.togetherWith
import android.view.Choreographer
import android.view.HapticFeedbackConstants
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.background
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.ime
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.isImeVisible
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.filled.KeyboardCommandKey
import androidx.compose.material.icons.filled.CloseFullscreen
import androidx.compose.material.icons.filled.OpenInFull
import androidx.compose.material.icons.filled.VerticalSplit
import androidx.compose.material.icons.filled.HorizontalSplit
import androidx.compose.material.icons.filled.ContentPaste
import androidx.compose.material.icons.filled.ContentCopy
import androidx.compose.material.icons.filled.Palette
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.List
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.KeyboardArrowDown
import androidx.compose.material.icons.filled.MoreHoriz
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.MutableState
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.clipToBounds
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.graphics.lerp
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.onClick
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.max
import androidx.compose.ui.unit.sp
import androidx.compose.ui.viewinterop.AndroidView
import dev.raptor.app.Focus
import dev.raptor.app.RaptorApplication
import dev.raptor.app.terminal.Modifiers
import dev.raptor.app.terminal.TerminalView
import kotlinx.coroutines.launch
import uniffi.raptor_ffi.AgentState
import uniffi.raptor_ffi.Block
import uniffi.raptor_ffi.Key
import uniffi.raptor_ffi.Session
import uniffi.raptor_ffi.Terminal
import uniffi.raptor_ffi.TerminalListener
import uniffi.raptor_ffi.TerminalStatus
import uniffi.raptor_ffi.Window

/** A short glyph for what a pane is running, shown on tiles and in the dock. */
fun glyph(b: Block?): String {
    val p = (b?.agent?.app?.takeIf { it.isNotBlank() } ?: b?.process ?: "").lowercase()
    return when {
        b == null -> ">_"
        p.isEmpty() || p in setOf("bash", "zsh", "fish", "sh", "nu", "dash", "ksh", "login") -> ">_"
        p.startsWith("nvim") -> "N"
        p == "vim" || p == "vi" -> "V"
        p.startsWith("claude") || b.agent?.app?.lowercase()?.contains("claude") == true -> "✳"
        p.startsWith("codex") -> "◎"
        p.startsWith("python") -> "Py"
        p == "node" || p == "bun" || p == "deno" -> "JS"
        p in setOf("htop", "top", "btop") -> "◔"
        p == "ssh" || p == "mosh" -> "⇄"
        p == "git" || p == "lazygit" || p == "tig" -> "±"
        p.none { it.isLetter() } -> (b.name.firstOrNull { it.isLetter() } ?: '•').uppercase()
        else -> p.first().uppercase()
    }
}

/** What the open terminal exposes to the screen's chrome. */
class TermHandle(val view: TerminalView, val modifiers: Modifiers, val control: MutableState<Boolean>)

/** The live key bar, published by the open terminal so the screen can place it. */
private val activeKeyBar = mutableStateOf<(@Composable () -> Unit)?>(null)

@OptIn(ExperimentalLayoutApi::class)
@Composable
fun TerminalScreen(serverId: String, sessionId: String, initialBlockId: String?, onSessions: () -> Unit, onOpen: (dev.raptor.app.Focus) -> Unit) {
    val app = RaptorApplication.instance
    val state by app.state.collectAsState()
    val themeId by app.themeId.collectAsState()
    val theme = themeById(themeId)
    var sid by remember { mutableStateOf(sessionId) }
    val server = state.servers.firstOrNull { it.id == serverId }
    val session = server?.sessions?.firstOrNull { it.id == sid }
    var windowId by remember { mutableStateOf<String?>(null) }
    var blockId by remember { mutableStateOf(initialBlockId) }
    val scope = rememberCoroutineScope()

    // Re-run on any change to the session, so a pane that disappears (closed
    // here or elsewhere) is replaced by a live one instead of a dead screen.
    LaunchedEffect(session, blockId) {
        val s = session ?: return@LaunchedEffect
        val win = s.windows.firstOrNull { w -> w.blocks.any { it.id == blockId } }
            ?: s.windows.firstOrNull { it.id == (windowId ?: s.activeWindowId) }
            ?: s.windows.firstOrNull()
        windowId = win?.id
        if (blockId == null || win?.blocks?.none { it.id == blockId } == true) {
            // The window's focus can still name the pane that just closed.
            blockId = win?.focusedBlockId?.takeIf { f -> win.blocks.any { it.id == f } } ?: win?.blocks?.firstOrNull()?.id
        }
    }
    val window = session?.windows?.firstOrNull { it.id == windowId }
    val block = window?.blocks?.firstOrNull { it.id == blockId }

    DisposableEffect(serverId, sid, blockId) {
        app.focus = Focus(serverId, sid, blockId)
        blockId?.let { app.clearNotification(serverId, it) }
        onDispose { app.focus = null }
    }

    // Chrome takes its colors from the terminal it floats on.
    var termColors by remember { mutableStateOf<Pair<Int, Int>?>(null) }
    val tc = termColors
    val chrome = if (theme.ansi == null && tc != null) Chrome.of(Color(tc.first), Color(tc.second), Color(theme.accent)) else Chrome.of(theme)

    RaptorTheme(chrome) {
        // Follows the keyboard frame by frame, so the key bar leaves with it.
        val ime = WindowInsets.ime.getBottom(LocalDensity.current) > 0
        val density = LocalDensity.current
        var topH by remember { mutableIntStateOf(0) }
        var bottomH by remember { mutableIntStateOf(0) }
        val handle = remember { mutableStateOf<TermHandle?>(null) }
        var showPalette by remember { mutableStateOf(false) }
        var slideDir by remember { mutableIntStateOf(1) }
        val panes = session?.windows.orEmpty().flatMap { w -> w.blocks.map { w.id to it.id } }
        // Swipe left/right moves between tabs (windows), landing on each tab's
        // focused pane. With a single tab, it cycles that tab's panes instead.
        fun swipe(dir: Int) {
            val wins = session?.windows.orEmpty()
            if (wins.size > 1) {
                val i = wins.indexOfFirst { it.id == windowId }.coerceAtLeast(0)
                val w = wins[(i + dir + wins.size) % wins.size]
                slideDir = dir
                windowId = w.id
                blockId = w.focusedBlockId?.takeIf { f -> w.blocks.any { it.id == f } } ?: w.blocks.firstOrNull()?.id
                scope.launch { runCatching { app.core.focusWindow(serverId, sid, w.id) } }
                return
            }
            val i = panes.indexOfFirst { it.second == blockId }
            if (panes.size < 2 || i < 0) return
            val next = panes[(i + dir + panes.size) % panes.size]
            slideDir = dir
            windowId = next.first
            blockId = next.second
            scope.launch { runCatching { app.core.focusBlock(serverId, sid, next.second) } }
        }
        // Close a pane: move to its neighbour first (same window if possible), so
        // the screen never shows the closed terminal, then ask the server.
        fun closePane(id: String) {
            if (id == blockId) {
                val sameWindow = panes.filter { it.first == windowId && it.second != id }
                val next = sameWindow.firstOrNull() ?: panes.firstOrNull { it.second != id }
                if (next != null) {
                    windowId = next.first
                    blockId = next.second
                }
            }
            scope.launch { runCatching { app.core.closeBlock(serverId, sid, id) } }
        }
        // The terminal view outlives recompositions; always call the latest swipe.
        val swipeNow = rememberUpdatedState(::swipe)

        Box(Modifier.fillMaxSize().background(Ink.terminal).imePadding()) {
            val b = blockId
            if (b != null) {
                AnimatedContent(
                    b,
                    transitionSpec = {
                        val d = slideDir
                        (slideInHorizontally(tween(240)) { w -> w * d } togetherWith slideOutHorizontally(tween(240)) { w -> -w * d })
                    },
                    label = "pane",
                ) { pane ->
                    LiveTerminal(
                        serverId, sid, pane, theme, handle,
                        insetTop = topH.toFloat(),
                        insetBottom = bottomH.toFloat(),
                        onColors = { bg, fg -> termColors = bg to fg },
                        onSwipe = { swipeNow.value(it) },
                        onPalette = { showPalette = true },
                        modifier = Modifier.fillMaxSize(),
                    )
                }
            } else {
                Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                    if (session == null) CircularProgressIndicator(color = Ink.accent) else Text("Nothing open in this window", color = Ink.secondary)
                }
            }

            // ---- top bar: title row, then window tabs when there's more than one ----
            Column(
                Modifier
                    .fillMaxWidth()
                    // The bar's full height, status bar included: the terminal's
                    // live rows start exactly below it and never hide under it.
                    .onSizeChanged { topH = it.height }
                    .statusBarsPadding()
                    .padding(bottom = 4.dp),
            ) {
                Row(Modifier.fillMaxWidth().padding(horizontal = 6.dp), verticalAlignment = Alignment.CenterVertically) {
                    GlassCircle(onClick = onSessions) { Icon(Icons.AutoMirrored.Filled.List, "Sessions", tint = Ink.text, modifier = Modifier.size(22.dp)) }
                    Spacer(Modifier.width(6.dp))
                    Tile(glyph(block), Modifier.size(32.dp))
                    Spacer(Modifier.width(10.dp))
                    TitleBlock(block, session, Modifier.weight(1f), onClick = onSessions)
                    Spacer(Modifier.width(8.dp))
                    if (window != null && window.blocks.size > 1) {
                        PaneMap(window, blockId, Modifier.size(width = 54.dp, height = 38.dp)) { picked ->
                            blockId = picked
                            scope.launch { runCatching { app.core.focusBlock(serverId, sid, picked) } }
                        }
                        Spacer(Modifier.width(8.dp))
                    }
                    MoreMenu(serverId, sid, blockId, handle.value, onClosePane = ::closePane) { showPalette = true }
                }
                if (session != null && session.windows.size > 1) {
                    Spacer(Modifier.height(8.dp))
                    WindowTabs(serverId, session, windowId) { w ->
                        slideDir = if (session.windows.indexOf(w) > session.windows.indexOfFirst { it.id == windowId }) 1 else -1
                        windowId = w.id
                        blockId = w.focusedBlockId?.takeIf { f -> w.blocks.any { it.id == f } } ?: w.blocks.firstOrNull()?.id
                        scope.launch { runCatching { app.core.focusWindow(serverId, sid, w.id) } }
                    }
                }
            }

            // ---- bottom: the key bar while typing; otherwise the terminal runs to the edge ----
            Box(Modifier.align(Alignment.BottomCenter).fillMaxWidth().onSizeChanged { bottomH = it.height }) {
                if (ime && dev.raptor.app.ui.Prefs.flag(dev.raptor.app.ui.Prefs.KEY_BAR)) activeKeyBar.value?.invoke()
                else Spacer(Modifier.fillMaxWidth().navigationBarsPadding())
            }
        }

        if (showPalette) {
            val h = handle.value
            val local = buildList {
                add(PaletteItem("next-pane", "Next Tab", glyph = "→", keywords = "switch swipe window pane") { swipe(1) })
                add(PaletteItem("prev-pane", "Previous Tab", glyph = "←", keywords = "switch swipe window pane") { swipe(-1) })
                add(PaletteItem("new-window", "New Window", glyph = "+") { scope.launch { runCatching { app.core.newWindow(serverId, sid) } } })
                blockId?.let { bid ->
                    add(PaletteItem("split-right", "Split Right", glyph = "▥") { scope.launch { runCatching { app.core.split(serverId, sid, bid, false) } } })
                    add(PaletteItem("split-down", "Split Down", glyph = "▤") { scope.launch { runCatching { app.core.split(serverId, sid, bid, true) } } })
                    add(PaletteItem("close-pane", "Close Pane", glyph = "×") { closePane(bid) })
                }
                if (h != null) {
                    val control = h.control.value
                    add(PaletteItem("fit", if (control) "Use Desktop Size" else "Fit to This Screen", glyph = "⤢", keywords = "resize size") {
                        val t = h.view.terminal ?: return@PaletteItem
                        if (control) t.releaseControl() else t.takeControl()
                        h.control.value = !control
                        h.view.control = !control
                    })
                    add(PaletteItem("paste", "Paste", glyph = "⎘") { h.view.pasteClipboard() })
                    add(PaletteItem("copy-all", "Copy All Text", glyph = "⧉") { h.view.terminal?.allText()?.let(h.view::copy) })
                }
                add(PaletteItem("new-session", "New Session", category = "Sessions", glyph = "+") {
                    scope.launch { runCatching { app.core.createSession(serverId, null) }.getOrNull()?.let { onOpen(dev.raptor.app.Focus(serverId, it, null)) } }
                })
                for (o in server?.sessions.orEmpty()) if (o.id != sid) {
                    add(PaletteItem("session-${o.id}", "Go to ${o.label}", category = "Sessions", glyph = "▸") { onOpen(dev.raptor.app.Focus(serverId, o.id, null)) })
                }
                for (t in Themes) add(PaletteItem("theme-${t.id}", t.name, category = "Themes", glyph = if (t.id == themeId) "●" else "○", keywords = "theme color") { app.setTheme(t.id) })
            }
            CommandPalette(serverId, sid, blockId, local) { showPalette = false }
        }
    }
}

/** The translucent surface shared by all floating chrome. */
@Composable
fun Modifier.glass(shape: Shape): Modifier = this.clip(shape).background(Ink.glass).border(1.dp, Ink.glassStroke, shape)

@Composable
fun GlassCircle(size: Dp = 40.dp, onClick: () -> Unit, content: @Composable () -> Unit) {
    // Drawn at [size], touched at 48dp.
    Box(Modifier.size(max(size, 48.dp)).clip(CircleShape).clickable(onClick = onClick), contentAlignment = Alignment.Center) {
        Box(Modifier.size(size).glass(CircleShape), contentAlignment = Alignment.Center) { content() }
    }
}

/** A rounded tile with a glyph for the running program. */
@Composable
fun Tile(text: String, modifier: Modifier) {
    val shape = RoundedCornerShape(9.dp)
    Box(modifier.clip(shape).background(lerp(Ink.raised, Ink.accent, 0.16f)).border(1.dp, Ink.glassStroke, shape), contentAlignment = Alignment.Center) {
        Text(text, color = Ink.accent, fontFamily = Mono, fontWeight = FontWeight.Bold, fontSize = if (text.length > 1) 12.sp else 15.sp)
    }
}

@Composable
private fun TitleBlock(block: Block?, session: Session?, modifier: Modifier, onClick: () -> Unit) {
    Column(modifier.clip(RoundedCornerShape(8.dp)).clickable(onClick = onClick).padding(vertical = 2.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(block?.let { runningName(it) } ?: "terminal", color = Ink.text, fontWeight = FontWeight.SemiBold, fontSize = 15.sp, maxLines = 1)
            val pwd = block?.shortPwd.orEmpty()
            if (pwd.isNotEmpty()) {
                Spacer(Modifier.width(6.dp))
                Text(pwd, color = Ink.secondary, fontSize = 13.sp, maxLines = 1, overflow = TextOverflow.Ellipsis, fontFamily = Mono)
            }
        }
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(session?.label ?: "", color = Ink.tertiary, fontSize = 12.sp, maxLines = 1)
            val agent = block?.agent?.takeIf { it.state != AgentState.IDLE }
            if (agent != null) Text(" · " + agentLabel(agent), color = LocalStatus.current.agent(agent.state), fontSize = 12.sp, maxLines = 1)
            Icon(Icons.Default.KeyboardArrowDown, null, tint = Ink.tertiary, modifier = Modifier.size(14.dp))
        }
    }
}

/** The window's pane layout in miniature. Tap a pane to switch to it. */
@Composable
private fun PaneMap(window: Window, selected: String?, modifier: Modifier, onPick: (String) -> Unit) {
    val st = LocalStatus.current
    val on = Ink.accent
    val off = Ink.key
    Box(modifier.glass(RoundedCornerShape(10.dp)).padding(4.dp)) {
        Canvas(
            Modifier.fillMaxSize().pointerInput(window) {
                detectTapGestures { p ->
                    val fx = p.x / size.width
                    val fy = p.y / size.height
                    window.blocks.firstOrNull { fx >= it.x && fx < it.x + it.w && fy >= it.y && fy < it.y + it.h }?.let { onPick(it.id) }
                }
            },
        ) {
            val gap = 2.dp.toPx()
            for (b in window.blocks) {
                val tint = b.agent?.takeIf { it.state != AgentState.IDLE }?.let { st.agent(it.state) }
                drawRoundRect(
                    color = if (b.id == selected) on.copy(alpha = 0.55f) else (tint?.copy(alpha = 0.55f) ?: off),
                    topLeft = Offset((b.x * size.width).toFloat() + gap / 2, (b.y * size.height).toFloat() + gap / 2),
                    size = Size((b.w * size.width).toFloat() - gap, (b.h * size.height).toFloat() - gap),
                    cornerRadius = CornerRadius(3.dp.toPx()),
                )
            }
        }
    }
}

/** The session's windows as slim tabs in the top bar; always reachable, even while typing. */
@Composable
private fun WindowTabs(serverId: String, session: Session, current: String?, onPick: (Window) -> Unit) {
    val app = RaptorApplication.instance
    val scope = rememberCoroutineScope()
    val st = LocalStatus.current
    val view = LocalView.current
    Row(
        Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()).padding(horizontal = 10.dp),
        horizontalArrangement = Arrangement.spacedBy(6.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        for (w in session.windows) {
            val focused = w.blocks.firstOrNull { it.id == w.focusedBlockId } ?: w.blocks.firstOrNull()
            val sel = w.id == current
            val urgent = w.blocks.any { it.agent?.state == AgentState.BLOCKED }
            val label = w.label.takeIf { it.isNotBlank() && !it.startsWith("Window ") } ?: focused?.let { runningName(it) } ?: "shell"
            Row(
                Modifier.height(30.dp).clip(RoundedCornerShape(9.dp))
                    .background(if (sel) Ink.raised else Color.Transparent)
                    .border(1.dp, if (sel) Ink.glassStroke else Color.Transparent, RoundedCornerShape(9.dp))
                    .clickable {
                        if (!sel) view.performHapticFeedback(android.view.HapticFeedbackConstants.CLOCK_TICK)
                        onPick(w)
                    }
                    .padding(horizontal = 11.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                if (urgent) {
                    Box(Modifier.size(6.dp).clip(CircleShape).background(st.blocked))
                    Spacer(Modifier.width(6.dp))
                }
                Text(label, color = if (sel) Ink.text else Ink.secondary, fontSize = 13.sp, fontWeight = if (sel) FontWeight.SemiBold else FontWeight.Normal, maxLines = 1)
                if (w.blocks.size > 1) {
                    Spacer(Modifier.width(6.dp))
                    Text("${w.blocks.size}", color = Ink.tertiary, fontSize = 11.sp, fontFamily = Mono)
                }
            }
        }
        Box(
            Modifier.size(30.dp).clip(RoundedCornerShape(9.dp)).clickable { scope.launch { runCatching { app.core.newWindow(serverId, session.id) } } },
            contentAlignment = Alignment.Center,
        ) { Icon(Icons.Default.Add, "New window", tint = Ink.secondary, modifier = Modifier.size(18.dp)) }
    }
}

@Composable
private fun LiveTerminal(
    serverId: String,
    sessionId: String,
    blockId: String,
    theme: ThemeSpec,
    handle: MutableState<TermHandle?>,
    insetTop: Float,
    insetBottom: Float,
    onColors: (Int, Int) -> Unit,
    onSwipe: (Int) -> Unit,
    onPalette: () -> Unit,
    modifier: Modifier,
) {
    val app = RaptorApplication.instance
    val context = LocalView.current.context
    val modifiers = remember { Modifiers() }
    var status by remember { mutableStateOf<TerminalStatus>(TerminalStatus.Connecting) }
    val controlState = remember { mutableStateOf(false) }
    var control by controlState
    val fontSize = remember { app.core.setting("font_size")?.toFloatOrNull() ?: 13f }

    val view = remember {
        TerminalView(context, modifiers).also { v ->
            v.fontSize = fontSize
            v.onFontSizeChanged = { app.core.setSetting("font_size", it.toString()) }
        }
    }
    view.onRequestKeyboard = { view.showKeyboard() }
    view.onColors = onColors
    view.onSwipePane = onSwipe
    view.onPalette = onPalette
    if (view.insetTop != insetTop) view.insetTop = insetTop
    if (view.insetBottom != insetBottom) view.insetBottom = insetBottom

    DisposableEffect(blockId) {
        val choreographer = Choreographer.getInstance()
        var scheduled = false
        val frameCb = Choreographer.FrameCallback {
            scheduled = false
            view.refresh()
        }
        val listener = object : TerminalListener {
            override fun frameReady() {
                view.post {
                    if (!scheduled) {
                        scheduled = true
                        choreographer.postFrameCallback(frameCb)
                    }
                }
            }

            override fun statusChanged(status_: TerminalStatus) {
                view.post { status = status_ }
            }

            override fun clipboard(text: String) {
                view.post { view.copy(text) }
            }

            override fun sizeChanged(cols: UShort, rows: UShort) {
                view.post { view.invalidate() }
            }
        }
        val term: Terminal? = runCatching { app.core.openTerminal(serverId, sessionId, blockId, listener) }.getOrNull()
        view.terminal = term
        // The phone takes the size while you use it (like tmux following the active
        // client): readable text, and the keyboard never hides the prompt.
        if (Prefs.flag(Prefs.FIT_ON_OPEN)) term?.takeControl()
        control = term != null && Prefs.flag(Prefs.FIT_ON_OPEN)
        view.control = control
        // Don't pop the keyboard on open: look first, tap the terminal to type.
        onDispose {
            choreographer.removeFrameCallback(frameCb)
            view.terminal = null
            term?.shutdown()
            term?.destroy()
        }
    }

    // Apply the chosen theme to the terminal itself.
    // Font, size, spacing and cursor from Settings, live.
    val appearance by app.appearance.collectAsState()
    LaunchedEffect(appearance) {
        view.applyAppearance(
            Prefs.string(Prefs.FONT),
            Prefs.string(Prefs.LINE_HEIGHT)?.toFloatOrNull() ?: 1.08f,
            Prefs.string(Prefs.CURSOR),
            Prefs.flag(Prefs.BLINK),
        )
        view.smartKeyboard = Prefs.flag(Prefs.SMART_KEYBOARD, default = false)
        app.core.setting("font_size")?.toFloatOrNull()?.let { if (it != view.fontSize) view.fontSize = it }
    }

    LaunchedEffect(theme.id, view.terminal) {
        val t = view.terminal ?: return@LaunchedEffect
        val ansi = theme.ansi
        if (ansi == null) t.useServerColors()
        else t.setTheme(theme.foreground.toUInt(), theme.background.toUInt(), theme.cursor.toUInt(), ansi.map { it.toUInt() })
    }

    val bar: @Composable () -> Unit = remember { @Composable { KeyBar(view, modifiers) } }
    DisposableEffect(Unit) {
        val h = TermHandle(view, modifiers, controlState)
        handle.value = h
        activeKeyBar.value = bar
        onDispose {
            if (handle.value === h) handle.value = null
            if (activeKeyBar.value === bar) activeKeyBar.value = null
        }
    }

    Box(modifier) {
        AndroidView({ view }, Modifier.fillMaxSize().clipToBounds())
        StatusOverlay(status, Modifier.align(Alignment.Center))
    }
}

@Composable
private fun MoreMenu(serverId: String, sessionId: String, blockId: String?, h: TermHandle?, onClosePane: (String) -> Unit, onPalette: () -> Unit) {
    val app = RaptorApplication.instance
    val scope = rememberCoroutineScope()
    var menu by remember { mutableStateOf(false) }
    var themes by remember { mutableStateOf(false) }
    Box {
        GlassCircle(onClick = { menu = true }) { Icon(Icons.Default.MoreHoriz, "More", tint = Ink.text, modifier = Modifier.size(20.dp)) }
        DropdownMenu(expanded = menu, onDismissRequest = { menu = false }, containerColor = Ink.raised, shape = RoundedCornerShape(14.dp), border = androidx.compose.foundation.BorderStroke(1.dp, Ink.glassStroke), shadowElevation = 12.dp) {
            RaptorMenuItem("Command Palette", Icons.Default.KeyboardCommandKey) { menu = false; onPalette() }
            RaptorMenuItem("New Window", Icons.Default.Add) { menu = false; scope.launch { runCatching { app.core.newWindow(serverId, sessionId) } } }
            HorizontalDivider(color = Ink.hairline)
            if (h != null) {
                val control = h.control.value
                RaptorMenuItem(
                    if (control) "Use Desktop Size" else "Fit to This Screen",
                    if (control) Icons.Default.CloseFullscreen else Icons.Default.OpenInFull,
                ) {
                    menu = false
                    val t = h.view.terminal ?: return@RaptorMenuItem
                    if (control) t.releaseControl() else t.takeControl()
                    h.control.value = !control
                    h.view.control = !control
                }
            }
            if (blockId != null) {
                RaptorMenuItem("Split Right", Icons.Default.VerticalSplit) { menu = false; scope.launch { runCatching { app.core.split(serverId, sessionId, blockId, false) } } }
                RaptorMenuItem("Split Down", Icons.Default.HorizontalSplit) { menu = false; scope.launch { runCatching { app.core.split(serverId, sessionId, blockId, true) } } }
            }
            HorizontalDivider(color = Ink.hairline)
            if (h != null) {
                RaptorMenuItem("Paste", Icons.Default.ContentPaste) { menu = false; h.view.pasteClipboard() }
                RaptorMenuItem("Copy All Text", Icons.Default.ContentCopy) { menu = false; h.view.terminal?.allText()?.let(h.view::copy) }
            }
            RaptorMenuItem("Theme", Icons.Default.Palette) { menu = false; themes = true }
            if (blockId != null) {
                HorizontalDivider(color = Ink.hairline)
                RaptorMenuItem("Close Pane", Icons.Default.Close, color = LocalStatus.current.error) {
                    menu = false
                    onClosePane(blockId)
                }
            }
        }
        DropdownMenu(expanded = themes, onDismissRequest = { themes = false }, containerColor = Ink.raised, shape = RoundedCornerShape(14.dp), border = androidx.compose.foundation.BorderStroke(1.dp, Ink.glassStroke), shadowElevation = 12.dp) {
            val current by app.themeId.collectAsState()
            for (t in Themes) {
                DropdownMenuItem(
                    leadingIcon = { Box(Modifier.size(16.dp).clip(CircleShape).background(Color(t.background)).border(1.5.dp, Color(t.accent), CircleShape)) },
                    text = { Text(t.name, color = if (t.id == current) Ink.accent else Ink.text, fontSize = 14.sp) },
                    onClick = { themes = false; app.setTheme(t.id) },
                )
            }
        }
    }
}

@Composable
private fun MenuRow(text: String, color: Color = Ink.text, onClick: () -> Unit) = RaptorMenuItem(text, color = color, onClick = onClick)

@Composable
private fun StatusOverlay(status: TerminalStatus, modifier: Modifier) {
    val text = when (status) {
        is TerminalStatus.Connecting -> "Connecting…"
        is TerminalStatus.Reconnecting -> "Reconnecting…"
        is TerminalStatus.Exited -> if (status.code == 0) "Process exited" else "Process exited (${status.code})"
        is TerminalStatus.Ended -> status.reason
        is TerminalStatus.Live -> null
    }
    AnimatedVisibility(text != null, modifier, enter = fadeIn(), exit = fadeOut()) {
        Row(
            Modifier.glass(RoundedCornerShape(50)).padding(horizontal = 14.dp, vertical = 8.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            if (status is TerminalStatus.Connecting || status is TerminalStatus.Reconnecting) {
                CircularProgressIndicator(Modifier.size(12.dp), strokeWidth = 1.5.dp, color = Ink.accent)
            }
            Text(text ?: "", color = Ink.text, fontSize = 13.sp)
        }
    }
}

private data class BarKey(val label: String, val key: Key? = null, val text: String? = null, val repeat: Boolean = false, val spoken: String = label)

private val mainKeys = listOf(
    BarKey("esc", Key.Escape, spoken = "Escape"),
    BarKey("tab", Key.Tab, spoken = "Tab"),
    BarKey("ctrl", spoken = "Control"),
    BarKey("alt", spoken = "Alt"),
    BarKey("←", Key.Left, repeat = true, spoken = "Left arrow"),
    BarKey("↓", Key.Down, repeat = true, spoken = "Down arrow"),
    BarKey("↑", Key.Up, repeat = true, spoken = "Up arrow"),
    BarKey("→", Key.Right, repeat = true, spoken = "Right arrow"),
)

private val moreKeys = listOf(
    BarKey("|", text = "|", spoken = "Pipe"),
    BarKey("~", text = "~", spoken = "Tilde"),
    BarKey("/", text = "/", spoken = "Slash"),
    BarKey("-", text = "-", spoken = "Dash"),
    BarKey("`", text = "`", spoken = "Backtick"),
    BarKey("home", Key.Home, spoken = "Home"),
    BarKey("end", Key.End, spoken = "End"),
    BarKey("pgup", Key.PageUp, repeat = true, spoken = "Page up"),
    BarKey("pgdn", Key.PageDown, repeat = true, spoken = "Page down"),
)

/** Terminal keys on a glass strip right above the soft keyboard. Two pages, no scrolling. */
@Composable
private fun KeyBar(view: TerminalView, mods: Modifiers) {
    val hv = LocalView.current
    var more by remember { mutableStateOf(false) }
    // Modifiers is a plain object; bump a tick so ctrl/alt highlights update.
    var tick by remember { mutableIntStateOf(0) }
    DisposableEffect(mods) {
        mods.onChange = { tick++ }
        onDispose { mods.onChange = null }
    }
    @Suppress("UNUSED_EXPRESSION") tick
    fun press(k: BarKey) {
        hv.performHapticFeedback(HapticFeedbackConstants.KEYBOARD_TAP)
        when {
            k.label == "ctrl" -> {
                when { mods.ctrlLocked -> { mods.ctrlLocked = false; mods.ctrl = false }; mods.ctrl -> mods.ctrlLocked = true; else -> mods.ctrl = true }
                mods.onChange?.invoke()
            }
            k.label == "alt" -> {
                when { mods.altLocked -> { mods.altLocked = false; mods.alt = false }; mods.alt -> mods.altLocked = true; else -> mods.alt = true }
                mods.onChange?.invoke()
            }
            k.key != null -> view.key(k.key)
            k.text != null -> {
                val (c, a) = mods.consume()
                if (c || a) view.terminal?.sendKey(Key.Char(k.text), c, a, false, false) else view.terminal?.sendText(k.text)
            }
        }
    }
    Row(
        Modifier.fillMaxWidth().padding(horizontal = 6.dp, vertical = 6.dp).glass(RoundedCornerShape(14.dp)).height(48.dp).padding(3.dp),
        horizontalArrangement = Arrangement.spacedBy(2.dp),
    ) {
        for (k in if (more) moreKeys else mainKeys) {
            val on = (k.label == "ctrl" && mods.ctrl) || (k.label == "alt" && mods.alt)
            val locked = (k.label == "ctrl" && mods.ctrlLocked) || (k.label == "alt" && mods.altLocked)
            BarButton(
                label = k.label,
                spoken = k.spoken,
                selected = if (k.key == null && k.text == null) on || locked else null,
                background = when { locked -> Ink.accent; on -> Ink.accent.copy(alpha = 0.3f); else -> Color.Transparent },
                foreground = if (locked) Ink.bg else Ink.text,
                repeat = k.repeat,
                modifier = Modifier.weight(1f),
            ) { press(k) }
        }
        BarButton(label = if (more) "abc" else "•••", spoken = if (more) "Main keys" else "More keys", background = Color.Transparent, foreground = Ink.secondary, modifier = Modifier.weight(1f)) {
            hv.performHapticFeedback(HapticFeedbackConstants.KEYBOARD_TAP)
            more = !more
        }
    }
}

@Composable
private fun BarButton(label: String, spoken: String, background: Color, foreground: Color, modifier: Modifier, repeat: Boolean = false, selected: Boolean? = null, onPress: () -> Unit) {
    var down by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()
    val downColor = Ink.keyDown
    Box(
        modifier
            .fillMaxSize()
            .clip(RoundedCornerShape(9.dp))
            .background(if (down) downColor else background)
            .clearAndSetSemantics {
                contentDescription = spoken
                role = Role.Button
                selected?.let { this.selected = it }
                onClick { onPress(); true }
            }
            .pointerInput(repeat) {
                detectTapGestures(onPress = {
                    down = true
                    onPress()
                    // Hold to repeat (arrows, paging), like a hardware key.
                    val job = if (repeat) scope.launch {
                        kotlinx.coroutines.delay(380)
                        while (true) { onPress(); kotlinx.coroutines.delay(45) }
                    } else null
                    tryAwaitRelease()
                    job?.cancel()
                    down = false
                })
            },
        contentAlignment = Alignment.Center,
    ) {
        Text(label, color = foreground, fontSize = if (label.length > 2) 12.sp else 15.sp, fontFamily = Mono)
    }
}
