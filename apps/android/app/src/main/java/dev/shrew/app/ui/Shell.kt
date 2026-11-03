package dev.shrew.app.ui

import android.content.Intent
import android.net.Uri
import androidx.compose.ui.text.withStyle
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.KeyboardArrowLeft
import androidx.compose.material.icons.automirrored.filled.KeyboardArrowRight
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material.icons.filled.Terminal
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Slider
import androidx.compose.material3.SliderDefaults
import androidx.compose.material3.Switch
import androidx.compose.material3.SwitchDefaults
import androidx.compose.material3.Text
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import dev.shrew.app.Focus
import dev.shrew.app.ShrewApplication
import kotlinx.coroutines.launch
import uniffi.shrew_ffi.Connectivity
import uniffi.shrew_ffi.Server

/** Boolean settings stored by the core, with defaults. */
object Prefs {
    const val FIT_ON_OPEN = "fit_on_open"
    const val KEY_BAR = "key_bar"
    const val NOTIFY_BLOCKED = "notify_blocked"
    const val NOTIFY_DONE = "notify_done"
    const val NOTIFY_EXIT = "notify_exit"
    const val NOTIFY_PROGRAM = "notify_program"
    const val FONT = "font"
    const val LINE_HEIGHT = "line_height"
    const val CURSOR = "cursor_style"
    const val BLINK = "cursor_blink"

    fun string(key: String): String? = ShrewApplication.instance.core.setting(key)?.takeIf { it.isNotEmpty() }
    fun setString(key: String, v: String) {
        ShrewApplication.instance.core.setSetting(key, v)
        ShrewApplication.instance.appearance.value++
    }

    fun flag(key: String, default: Boolean = true): Boolean =
        ShrewApplication.instance.core.setting(key)?.let { it == "true" } ?: default

    fun set(key: String, on: Boolean) = ShrewApplication.instance.core.setSetting(key, on.toString())
}

/**
 * The session switcher: everything you can jump to, pulled up from the
 * terminal's title. Settings and adding a server live in its header.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SessionSheet(
    current: Focus?,
    onOpen: (Focus) -> Unit,
    onAddServer: () -> Unit,
    onEditServer: (Server) -> Unit,
    onSettings: () -> Unit,
    onDismiss: () -> Unit,
) {
    val sheet = rememberModalBottomSheetState(skipPartiallyExpanded = false)
    val scope = rememberCoroutineScope()
    fun close(after: () -> Unit) {
        scope.launch { sheet.hide() }.invokeOnCompletion {
            onDismiss()
            after()
        }
    }
    ModalBottomSheet(onDismissRequest = onDismiss, sheetState = sheet, containerColor = Ink.bg, dragHandle = {
        Box(Modifier.padding(top = 8.dp, bottom = 4.dp).size(width = 40.dp, height = 4.dp).clip(RoundedCornerShape(2.dp)).background(Ink.tertiary.copy(alpha = 0.5f)))
    }) {
        Column(Modifier.fillMaxWidth().navigationBarsPadding()) {
            Row(Modifier.fillMaxWidth().padding(start = 20.dp, end = 12.dp, top = 4.dp, bottom = 8.dp), verticalAlignment = Alignment.CenterVertically) {
                Text("Sessions", color = Ink.text, fontSize = 22.sp, fontWeight = FontWeight.Bold, modifier = Modifier.weight(1f))
                GlassCircle(size = 40.dp, onClick = { close(onSettings) }) { Icon(Icons.Default.Settings, "Settings", tint = Ink.text, modifier = Modifier.size(20.dp)) }
                Spacer(Modifier.width(8.dp))
                GlassCircle(size = 40.dp, onClick = { close(onAddServer) }) { Icon(Icons.Default.Add, "Add server", tint = Ink.accent, modifier = Modifier.size(22.dp)) }
            }
            SessionList(
                current = current,
                onOpenSession = { s, ses, b -> close { onOpen(Focus(s, ses, b)) } },
                onAddServer = { close(onAddServer) },
                onEditServer = { srv -> close { onEditServer(srv) } },
                modifier = Modifier.fillMaxHeight(0.85f),
            )
        }
    }
}

/** First launch: nothing to open yet. */
@Composable
fun Welcome(onAddServer: () -> Unit) {
    Column(Modifier.fillMaxSize().background(Ink.bg).statusBarsPadding().navigationBarsPadding().padding(horizontal = 28.dp), verticalArrangement = Arrangement.Center) {
        Icon(Icons.Default.Terminal, null, tint = Ink.accent, modifier = Modifier.size(40.dp))
        Spacer(Modifier.height(20.dp))
        Text("Shrew", color = Ink.text, fontSize = 34.sp, fontWeight = FontWeight.Bold)
        Spacer(Modifier.height(10.dp))
        Text(
            "Your terminal sessions, from a Rex server on your Mac or any machine on your tailnet. They keep running when you close the app.",
            color = Ink.secondary,
            fontSize = 16.sp,
            lineHeight = 23.sp,
        )
        Spacer(Modifier.height(32.dp))
        Box(
            Modifier.fillMaxWidth().height(52.dp).clip(RoundedCornerShape(14.dp)).background(Ink.accent).clickable(onClick = onAddServer),
            contentAlignment = Alignment.Center,
        ) { Text("Add a server", color = Color(0xFF0B0D10), fontSize = 17.sp, fontWeight = FontWeight.SemiBold) }
    }
}

/** Waiting for the saved servers to report their sessions. */
@Composable
fun Loading(onSessions: () -> Unit) {
    Box(Modifier.fillMaxSize().background(Ink.bg), contentAlignment = Alignment.Center) {
        Column(horizontalAlignment = Alignment.CenterHorizontally) {
            androidx.compose.material3.CircularProgressIndicator(Modifier.size(22.dp), strokeWidth = 2.dp, color = Ink.accent)
            Spacer(Modifier.height(16.dp))
            Text("Connecting…", color = Ink.secondary, fontSize = 15.sp)
            Spacer(Modifier.height(20.dp))
            Text("Sessions", color = Ink.accent, fontSize = 15.sp, fontWeight = FontWeight.SemiBold, modifier = Modifier.clip(RoundedCornerShape(8.dp)).clickable(onClick = onSessions).padding(8.dp))
        }
    }
}

// ---- settings --------------------------------------------------------------

@Composable
fun SettingsScreen(onClose: () -> Unit, onAddServer: () -> Unit, onEditServer: (Server) -> Unit) {
    BackHandler(onBack = onClose)
    val app = ShrewApplication.instance
    val state by app.state.collectAsState()
    val themeId by app.themeId.collectAsState()
    val context = LocalContext.current
    var fontSize by remember { mutableFloatStateOf((app.core.setting("font_size")?.toFloatOrNull() ?: 13f).coerceIn(9f, 22f)) }
    val version = remember {
        runCatching { context.packageManager.getPackageInfo(context.packageName, 0).versionName }.getOrNull() ?: ""
    }

    Column(Modifier.fillMaxSize().background(Ink.bg).statusBarsPadding()) {
        Box(Modifier.fillMaxWidth().height(56.dp).padding(horizontal = 10.dp)) {
            GlassCircle(size = 40.dp, onClick = onClose) {
                Icon(Icons.AutoMirrored.Filled.KeyboardArrowLeft, "Back", tint = Ink.text, modifier = Modifier.size(26.dp))
            }
            Text("Settings", color = Ink.text, fontSize = 17.sp, fontWeight = FontWeight.SemiBold, modifier = Modifier.align(Alignment.Center))
        }
        Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).navigationBarsPadding().padding(bottom = 32.dp)) {
            var fontId by remember { mutableStateOf(Prefs.string(Prefs.FONT) ?: "jetbrains-mono") }
            var spacing by remember { mutableFloatStateOf(Prefs.string(Prefs.LINE_HEIGHT)?.toFloatOrNull() ?: 1.08f) }
            var cursor by remember { mutableStateOf(Prefs.string(Prefs.CURSOR) ?: "program") }
            val font = dev.shrew.app.terminal.TermFonts.byId(fontId)
            Section("Appearance") {
                // Live sample of the chosen theme, font, size and spacing.
                val t = themeById(themeId)
                Column(Modifier.fillMaxWidth().padding(16.dp).clip(RoundedCornerShape(10.dp)).background(Color(t.background)).padding(14.dp)) {
                    Text(
                        buildSample(t),
                        fontFamily = font.family,
                        fontSize = fontSize.sp,
                        lineHeight = (fontSize * spacing * 1.2f).sp,
                    )
                }
                Row(Modifier.fillMaxWidth().padding(horizontal = 12.dp), horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                    for (th in Themes) {
                        val on = th.id == themeId
                        Column(
                            Modifier.weight(1f).clip(RoundedCornerShape(10.dp)).clickable { app.setTheme(th.id) }.padding(vertical = 6.dp),
                            horizontalAlignment = Alignment.CenterHorizontally,
                        ) {
                            Box(
                                Modifier.size(34.dp).clip(CircleShape).background(Color(th.background))
                                    .border(if (on) 2.5.dp else 1.dp, if (on) Ink.accent else Ink.glassStroke, CircleShape),
                                contentAlignment = Alignment.Center,
                            ) { Box(Modifier.size(11.dp).clip(CircleShape).background(Color(th.accent))) }
                            Spacer(Modifier.height(6.dp))
                            Text(when (th.id) { "tokyo-night" -> "Tokyo"; "mocha" -> "Mocha"; "solarized-light" -> "Solarized"; "server" -> "Server"; else -> th.name }, color = if (on) Ink.text else Ink.secondary, fontSize = 11.sp, maxLines = 1)
                        }
                    }
                }
                Divider()
                SliderRow("Text size", fontSize, 9f..22f, 12, "${fontSize.toInt()}", { fontSize = Math.round(it).toFloat() }) {
                    app.core.setSetting("font_size", fontSize.toString())
                    app.appearance.value++
                }
                Divider()
                SliderRow("Spacing", spacing, 1.0f..1.6f, 11, "%.2f".format(spacing), { spacing = (Math.round(it * 20) / 20f) }) {
                    Prefs.setString(Prefs.LINE_HEIGHT, spacing.toString())
                }
            }
            Section("Font") {
                for ((i, f) in dev.shrew.app.terminal.TermFonts.all.withIndex()) {
                    if (i > 0) Divider()
                    Row(
                        Modifier.fillMaxWidth().clickable { fontId = f.id; Prefs.setString(Prefs.FONT, f.id) }.padding(horizontal = 16.dp, vertical = 13.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Text(f.name, color = Ink.text, fontFamily = f.family, fontSize = 16.sp, modifier = Modifier.weight(1f))
                        Text("{a}=>0", color = Ink.tertiary, fontFamily = f.family, fontSize = 14.sp)
                        Spacer(Modifier.width(14.dp))
                        Box(Modifier.size(20.dp), contentAlignment = Alignment.Center) {
                            if (f.id == fontId) Icon(Icons.Default.Check, "Selected", tint = Ink.accent, modifier = Modifier.size(20.dp))
                        }
                    }
                }
            }
            Section("Terminal", footer = "Fitting resizes the session to your phone while you use it, so text is readable and the keyboard never covers the prompt. Desktop clients see the phone's size until you leave.") {
                Column(Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 12.dp)) {
                    Text("Cursor", color = Ink.text, fontSize = 16.sp)
                    Spacer(Modifier.height(10.dp))
                    Row(Modifier.fillMaxWidth().clip(RoundedCornerShape(10.dp)).background(Ink.raised).padding(3.dp)) {
                        for ((id, label) in listOf("program" to "Program", "block" to "Block", "bar" to "Bar", "underline" to "Underline")) {
                            val on = cursor == id
                            Box(
                                Modifier.weight(1f).clip(RoundedCornerShape(8.dp)).background(if (on) Ink.surface else Color.Transparent)
                                    .clickable { cursor = id; Prefs.setString(Prefs.CURSOR, id) }.padding(vertical = 8.dp),
                                contentAlignment = Alignment.Center,
                            ) { Text(label, color = if (on) Ink.text else Ink.secondary, fontSize = 13.sp, fontWeight = if (on) FontWeight.SemiBold else FontWeight.Normal) }
                        }
                    }
                }
                Divider()
                Toggle("Blinking cursor", Prefs.BLINK) { ShrewApplication.instance.appearance.value++ }
                Divider()
                Toggle("Fit to this screen when opening", Prefs.FIT_ON_OPEN)
                Divider()
                Toggle("Key bar above the keyboard", Prefs.KEY_BAR)
            }
            Section("Notifications") {
                Toggle("An agent needs you", Prefs.NOTIFY_BLOCKED)
                Divider()
                Toggle("An agent finishes", Prefs.NOTIFY_DONE)
                Divider()
                Toggle("A program exits", Prefs.NOTIFY_EXIT, default = false)
                Divider()
                Toggle("Programs send a notification", Prefs.NOTIFY_PROGRAM)
            }
            Section("Servers") {
                for ((i, srv) in state.servers.withIndex()) {
                    if (i > 0) Divider()
                    Row(
                        Modifier.fillMaxWidth().clickable { onEditServer(srv) }.padding(horizontal = 16.dp, vertical = 12.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Box(Modifier.size(8.dp).clip(CircleShape).background(connectivityColor(srv.connectivity)))
                        Spacer(Modifier.width(12.dp))
                        Column(Modifier.weight(1f)) {
                            Text(srv.name, color = Ink.text, fontSize = 16.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
                            Text(
                                listOfNotNull(srv.address, srv.version?.let { "Rex $it" }.takeIf { srv.connectivity is Connectivity.Online }).joinToString(" · "),
                                color = Ink.secondary,
                                fontSize = 13.sp,
                                fontFamily = Mono,
                                maxLines = 1,
                                overflow = TextOverflow.Ellipsis,
                            )
                        }
                        Icon(Icons.AutoMirrored.Filled.KeyboardArrowRight, null, tint = Ink.tertiary, modifier = Modifier.size(20.dp))
                    }
                }
                if (state.servers.isNotEmpty()) Divider()
                Row(Modifier.fillMaxWidth().clickable(onClick = onAddServer).padding(horizontal = 16.dp, vertical = 14.dp), verticalAlignment = Alignment.CenterVertically) {
                    Icon(Icons.Default.Add, null, tint = Ink.accent, modifier = Modifier.size(20.dp))
                    Spacer(Modifier.width(12.dp))
                    Text("Add a server", color = Ink.accent, fontSize = 16.sp)
                }
            }
            Section("About") {
                Row(Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 14.dp)) {
                    Text("Version", color = Ink.text, fontSize = 16.sp, modifier = Modifier.weight(1f))
                    Text(version, color = Ink.secondary, fontSize = 16.sp)
                }
                Divider()
                Row(
                    Modifier.fillMaxWidth().clickable {
                        context.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse("https://github.com/teddytennant/shrew")))
                    }.padding(horizontal = 16.dp, vertical = 14.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Text("Source code", color = Ink.text, fontSize = 16.sp, modifier = Modifier.weight(1f))
                    Text("MIT", color = Ink.secondary, fontSize = 16.sp)
                    Spacer(Modifier.width(6.dp))
                    Icon(Icons.AutoMirrored.Filled.KeyboardArrowRight, null, tint = Ink.tertiary, modifier = Modifier.size(20.dp))
                }
            }
        }
    }
}

private fun buildSample(t: ThemeSpec) = androidx.compose.ui.text.buildAnnotatedString {
    val ansi = t.ansi
    fun c(i: Int) = Color(ansi?.getOrNull(i) ?: t.foreground)
    val fg = Color(t.foreground)
    withStyle(androidx.compose.ui.text.SpanStyle(color = c(2))) { append("me@box") }
    withStyle(androidx.compose.ui.text.SpanStyle(color = fg)) { append(":") }
    withStyle(androidx.compose.ui.text.SpanStyle(color = c(4))) { append("~/src") }
    withStyle(androidx.compose.ui.text.SpanStyle(color = fg)) { append("$ cargo test\n") }
    withStyle(androidx.compose.ui.text.SpanStyle(color = fg)) { append("test result: ") }
    withStyle(androidx.compose.ui.text.SpanStyle(color = c(2))) { append("ok") }
    withStyle(androidx.compose.ui.text.SpanStyle(color = fg)) { append(". 11 passed; ") }
    withStyle(androidx.compose.ui.text.SpanStyle(color = c(1))) { append("0 failed") }
}

@Composable
private fun Section(title: String, footer: String? = null, content: @Composable () -> Unit) {
    Text(title, color = Ink.secondary, fontSize = 13.sp, modifier = Modifier.padding(start = 36.dp, top = 22.dp, bottom = 6.dp))
    Column(Modifier.padding(horizontal = 16.dp).fillMaxWidth().clip(RoundedCornerShape(12.dp)).background(Ink.surface)) { content() }
    if (footer != null) Text(footer, color = Ink.tertiary, fontSize = 13.sp, lineHeight = 18.sp, modifier = Modifier.padding(start = 36.dp, end = 36.dp, top = 6.dp))
}

@Composable
private fun SliderRow(label: String, value: Float, range: ClosedFloatingPointRange<Float>, steps: Int, shown: String, onChange: (Float) -> Unit, onDone: () -> Unit) {
    Row(Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 6.dp), verticalAlignment = Alignment.CenterVertically) {
        Text(label, color = Ink.text, fontSize = 16.sp, modifier = Modifier.width(88.dp))
        Slider(
            value = value,
            onValueChange = onChange,
            onValueChangeFinished = onDone,
            valueRange = range,
            steps = steps,
            colors = SliderDefaults.colors(thumbColor = Ink.accent, activeTrackColor = Ink.accent, inactiveTrackColor = Ink.raised, activeTickColor = Color.Transparent, inactiveTickColor = Color.Transparent),
            modifier = Modifier.weight(1f),
        )
        Text(shown, color = Ink.secondary, fontFamily = Mono, fontSize = 14.sp, modifier = Modifier.width(44.dp).padding(start = 8.dp))
    }
}

@Composable
private fun Divider() = HorizontalDivider(Modifier.padding(start = 16.dp), color = Ink.hairline)

@Composable
private fun Toggle(label: String, key: String, default: Boolean = true, onChange: () -> Unit = {}) {
    var on by remember { mutableStateOf(Prefs.flag(key, default)) }
    Row(
        Modifier.fillMaxWidth().clickable { on = !on; Prefs.set(key, on); onChange() }.padding(horizontal = 16.dp, vertical = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(label, color = Ink.text, fontSize = 16.sp, modifier = Modifier.weight(1f))
        Switch(
            checked = on,
            onCheckedChange = { on = it; Prefs.set(key, it); onChange() },
            colors = SwitchDefaults.colors(checkedTrackColor = Ink.accent, checkedThumbColor = Color(0xFF0B0D10), uncheckedTrackColor = Ink.raised, uncheckedBorderColor = Ink.glassStroke, uncheckedThumbColor = Ink.secondary),
        )
    }
}

@Suppress("unused")
private val keepCheck = Icons.Default.Check
