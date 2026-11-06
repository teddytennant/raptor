package dev.raptor.app.ui

import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.clickable
import androidx.compose.foundation.background
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.Send
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material.icons.filled.Edit
import androidx.compose.material.icons.filled.MoreHoriz
import androidx.compose.material.icons.filled.Refresh
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.tween
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.collectIsPressedAsState
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.runtime.derivedStateOf
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalView
import android.view.HapticFeedbackConstants
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.pulltorefresh.PullToRefreshBox
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import dev.raptor.app.RaptorApplication
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import uniffi.raptor_ffi.AgentState
import uniffi.raptor_ffi.AttentionItem
import uniffi.raptor_ffi.Block
import uniffi.raptor_ffi.Connectivity
import uniffi.raptor_ffi.Server
import uniffi.raptor_ffi.Session

/**
 * Every server and its sessions, as a quiet list: what needs you first, then
 * each server's sessions. Used by the session switcher and the tablet sidebar.
 */
@Composable
fun SessionList(
    current: dev.raptor.app.Focus?,
    onOpenSession: (serverId: String, sessionId: String, blockId: String?) -> Unit,
    onAddServer: () -> Unit,
    onEditServer: (Server) -> Unit,
    modifier: Modifier = Modifier,
    bottomPadding: androidx.compose.ui.unit.Dp = 24.dp,
) {
    val app = RaptorApplication.instance
    val state by app.state.collectAsState()
    val needs = state.attention.filter { it.block.agent?.state == AgentState.BLOCKED || it.block.agent?.state == AgentState.ERROR }
    LazyColumn(modifier.fillMaxWidth(), contentPadding = PaddingValues(bottom = bottomPadding)) {
        if (state.servers.isEmpty()) item(key = "empty") { EmptyState(onAddServer) }
        if (needs.isNotEmpty()) {
            item(key = "needs") { Header("Needs you") }
            items(needs, key = { "n" + it.serverId + it.block.id }) { NeedsRow(it) { onOpenSession(it.serverId, it.sessionId, it.block.id) } }
        }
        for (srv in state.servers) {
            item(key = "s" + srv.id) { ServerSection(srv, current, onEditServer) { sid -> onOpenSession(srv.id, sid, null) } }
        }
    }
}

@Composable
private fun Header(text: String, trailing: @Composable (() -> Unit)? = null) {
    Row(Modifier.fillMaxWidth().padding(start = 20.dp, end = 8.dp, top = 20.dp, bottom = 4.dp), verticalAlignment = Alignment.CenterVertically) {
        Text(text.uppercase(), style = MaterialTheme.typography.labelMedium, color = Ink.tertiary, modifier = Modifier.weight(1f))
        trailing?.invoke()
    }
}

/** What a row says under its title: where it is and what's running. */
internal fun detail(b: Block?): String {
    if (b == null) return ""
    return listOfNotNull(b.shortPwd.takeIf { it.isNotEmpty() }, runningName(b)).distinct().joinToString(" · ")
}

/** What's running, in words a person would use. Shells and junk names (some tools
 *  rename their process to a version number) fall back to the terminal title. */
internal fun runningName(b: Block): String? {
    val shells = setOf("bash", "zsh", "fish", "sh", "nu", "dash", "ksh", "tcsh", "login")
    val p = b.process.trim()
    val junk = p.isEmpty() || p.none { it.isLetter() } || p in shells
    val title = b.name.trim().takeIf { it.isNotEmpty() && it != b.shortPwd && it !in shells && it.any { c -> c.isLetter() } }
    return when {
        !junk -> p
        title != null -> title
        p in shells -> p
        else -> null
    }
}

@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun SessionRow(srv: Server, s: Session, selected: Boolean, onOpen: () -> Unit) {
    val app = RaptorApplication.instance
    val st = LocalStatus.current
    val scope = rememberCoroutineScope()
    var menu by remember { mutableStateOf(false) }
    var renaming by remember { mutableStateOf(false) }
    val active = s.windows.firstOrNull { it.id == s.activeWindowId } ?: s.windows.firstOrNull()
    val focused = active?.blocks?.firstOrNull { it.id == active.focusedBlockId } ?: active?.blocks?.firstOrNull()
    val online = srv.connectivity is Connectivity.Online
    val agent = s.agentState?.takeIf { it != AgentState.IDLE }
    val interaction = remember { MutableInteractionSource() }
    val pressed by interaction.collectIsPressedAsState()
    val view = LocalView.current

    Box {
        Row(
            Modifier
                .fillMaxWidth()
                .background(if (pressed || selected) Ink.raised else Color.Transparent)
                .combinedClickable(
                    interactionSource = interaction,
                    indication = null,
                    enabled = online,
                    onClick = onOpen,
                    onLongClick = {
                        view.performHapticFeedback(HapticFeedbackConstants.LONG_PRESS)
                        menu = true
                    },
                )
                .padding(horizontal = 16.dp, vertical = 11.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Column(Modifier.weight(1f)) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text(s.label, color = if (online) Ink.text else Ink.tertiary, style = MaterialTheme.typography.bodyLarge, fontWeight = FontWeight.Medium, maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f, fill = false))
                    if (agent != null) {
                        Spacer(Modifier.width(8.dp))
                        Box(Modifier.size(7.dp).clip(CircleShape).background(st.agent(agent)))
                    }
                }
                val d = detail(focused)
                if (d.isNotEmpty()) {
                    Spacer(Modifier.height(2.dp))
                    Text(d, style = MonoSmall, color = Ink.secondary, maxLines = 1, overflow = TextOverflow.Ellipsis)
                }
            }
            val panes = s.blockCount.toInt()
            if (s.windows.size > 1 || panes > 1) {
                Spacer(Modifier.width(12.dp))
                Text(
                    if (s.windows.size > 1) "${s.windows.size}" else "$panes",
                    style = MonoSmall,
                    color = Ink.secondary,
                    modifier = Modifier.clip(RoundedCornerShape(6.dp)).background(Ink.raised).padding(horizontal = 7.dp, vertical = 2.dp),
                )
            }
            if (selected) {
                Spacer(Modifier.width(12.dp))
                Icon(Icons.Default.Check, "Current", tint = Ink.accent, modifier = Modifier.size(18.dp))
            }
        }
        RaptorMenu(menu, { menu = false }) {
            RaptorMenuItem("Rename…", Icons.Default.Edit) { menu = false; renaming = true }
            RaptorMenuItem("New Window", Icons.Default.Add) {
                menu = false
                scope.launch { runCatching { app.core.newWindow(srv.id, s.id) } }
            }
            HorizontalDivider(color = Ink.hairline)
            RaptorMenuItem("End Session", Icons.Default.Close, color = LocalStatus.current.error) {
                menu = false
                scope.launch { runCatching { app.core.destroySession(srv.id, s.id) } }
            }
        }
    }
    if (renaming) RenameDialog(s.label, onDismiss = { renaming = false }) { name ->
        renaming = false
        scope.launch { runCatching { app.core.renameSession(srv.id, s.id, name) } }
    }
}

@Composable
private fun RenameDialog(current: String, onDismiss: () -> Unit, onDone: (String) -> Unit) {
    var name by remember { mutableStateOf(current) }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Rename session") },
        text = { OutlinedTextField(name, { name = it }, singleLine = true) },
        confirmButton = { TextButton(onClick = { onDone(name.trim()) }, enabled = name.isNotBlank()) { Text("Rename") } },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Cancel") } },
        containerColor = Ink.raised,
    )
}

/** What to call a server: its machine name when we know it, else what the user typed. */
internal fun serverTitle(srv: Server): String {
    val looksDefault = srv.name.contains('@') || srv.name.firstOrNull()?.isDigit() == true
    return if (looksDefault) srv.hostName?.takeIf { it.isNotBlank() } ?: srv.name else srv.name
}

/**
 * One server: a quiet heading (long-press for its options), then its sessions
 * in a single rounded group ending with "New session".
 */
@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun ServerSection(srv: Server, current: dev.raptor.app.Focus?, onEdit: (Server) -> Unit, onOpen: (String) -> Unit) {
    val app = RaptorApplication.instance
    val scope = rememberCoroutineScope()
    val view = LocalView.current
    var menu by remember { mutableStateOf(false) }
    val st = LocalStatus.current
    val online = srv.connectivity is Connectivity.Online
    Column(Modifier.padding(top = 18.dp)) {
        Box {
            Row(
                Modifier.fillMaxWidth().combinedClickable(onClick = {}, onLongClick = {
                    view.performHapticFeedback(HapticFeedbackConstants.LONG_PRESS)
                    menu = true
                }).padding(start = 32.dp, end = 32.dp, bottom = 8.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(serverTitle(srv).uppercase(), color = Ink.secondary, fontSize = 12.sp, fontWeight = FontWeight.SemiBold, letterSpacing = 0.6.sp, maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f))
                when (val c = srv.connectivity) {
                    is Connectivity.Online -> srv.latencyMs?.let { Text("${it} ms", color = Ink.tertiary, fontSize = 12.sp, fontFamily = Mono) }
                    is Connectivity.Connecting -> Text("connecting", color = st.connecting, fontSize = 12.sp)
                    is Connectivity.Offline -> Text("offline", color = st.error, fontSize = 12.sp)
                    is Connectivity.Idle -> Unit
                }
            }
            RaptorMenu(menu, { menu = false }) {
                RaptorMenuItem("Edit…", Icons.Default.Edit) { menu = false; onEdit(srv) }
                RaptorMenuItem("Reconnect", Icons.Default.Refresh) { menu = false; app.core.reconnect(srv.id) }
                HorizontalDivider(color = Ink.hairline)
                RaptorMenuItem("Remove", Icons.Default.Delete, color = st.error) { menu = false; app.core.removeServer(srv.id) }
            }
        }
        Column(Modifier.padding(horizontal = 16.dp).fillMaxWidth().clip(RoundedCornerShape(14.dp)).background(Ink.surface)) {
            val offline = srv.connectivity as? Connectivity.Offline
            if (offline != null) {
                Row(Modifier.fillMaxWidth().padding(start = 16.dp, end = 6.dp, top = 4.dp, bottom = 4.dp), verticalAlignment = Alignment.CenterVertically) {
                    Text(offline.error, color = Ink.secondary, fontSize = 14.sp, maxLines = 2, modifier = Modifier.weight(1f))
                    TextButton(onClick = { app.core.reconnect(srv.id) }) { Text("Retry", color = Ink.accent, fontWeight = FontWeight.SemiBold) }
                }
            } else {
                for (s in srv.sessions) {
                    SessionRow(srv, s, selected = current?.serverId == srv.id && current.sessionId == s.id) { onOpen(s.id) }
                    HorizontalDivider(Modifier.padding(start = 16.dp), color = Ink.hairline)
                }
                if (online) {
                    Row(
                        Modifier.fillMaxWidth().clickable { scope.launch { runCatching { app.core.createSession(srv.id, null) }.onSuccess(onOpen) } }.padding(horizontal = 16.dp, vertical = 13.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Icon(Icons.Default.Add, null, tint = Ink.accent, modifier = Modifier.size(18.dp))
                        Spacer(Modifier.width(10.dp))
                        Text("New session", color = Ink.accent, fontSize = 15.sp)
                    }
                } else {
                    Text("Connecting…", color = Ink.tertiary, fontSize = 14.sp, modifier = Modifier.padding(16.dp))
                }
            }
        }
    }
}

/** An agent waiting on you: what it said, the last few lines, and a reply box. */
@Composable
private fun NeedsRow(item: AttentionItem, onOpen: () -> Unit) {
    val app = RaptorApplication.instance
    val st = LocalStatus.current
    val a = item.block.agent ?: return
    val color = st.agent(a.state)
    var reply by remember { mutableStateOf("") }
    var preview by remember { mutableStateOf<String?>(null) }
    val scope = rememberCoroutineScope()
    LaunchedEffect(item.block.id, a.updatedAt) {
        preview = runCatching { app.core.previewText(item.serverId, item.sessionId, item.block.id) }.getOrNull()
            ?.lines()?.map { it.trimEnd() }?.filter { it.isNotBlank() }?.takeLast(4)?.joinToString("\n")
    }
    fun send() {
        val t = reply
        if (t.isEmpty()) return
        reply = ""
        scope.launch { runCatching { app.core.quickReply(item.serverId, item.sessionId, item.block.id, t + "\r") } }
    }
    Column(
        Modifier
            .fillMaxWidth()
            .padding(horizontal = 12.dp, vertical = 4.dp)
            .clip(RoundedCornerShape(12.dp))
            .background(Ink.surface)
            .combinedClickable(onClick = onOpen)
            .padding(14.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Box(Modifier.size(7.dp).clip(CircleShape).background(color))
            Spacer(Modifier.width(8.dp))
            Text(a.app.ifEmpty { item.block.name }, color = Ink.text, fontWeight = FontWeight.Medium, fontSize = 15.sp)
            Spacer(Modifier.width(8.dp))
            Text(agentLabel(a), color = color, fontSize = 13.sp)
            Spacer(Modifier.weight(1f))
            Text(item.sessionLabel, style = MonoSmall, color = Ink.tertiary, maxLines = 1)
        }
        val msg = a.message.ifEmpty { a.title }
        if (msg.isNotEmpty()) Text(msg, color = Ink.text, fontSize = 14.sp, maxLines = 3, overflow = TextOverflow.Ellipsis)
        preview?.takeIf { it.isNotEmpty() }?.let {
            Text(it, style = MonoSmall, color = Ink.secondary, maxLines = 4, modifier = Modifier.fillMaxWidth().clip(RoundedCornerShape(8.dp)).background(Ink.terminal).padding(10.dp))
        }
        Row(
            Modifier.fillMaxWidth().clip(RoundedCornerShape(8.dp)).background(Ink.raised).padding(start = 12.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Box(Modifier.weight(1f)) {
                if (reply.isEmpty()) Text("Reply", color = Ink.tertiary, fontSize = 14.sp)
                BasicTextField(
                    reply,
                    { reply = it },
                    singleLine = true,
                    textStyle = TextStyle(color = Ink.text, fontSize = 14.sp, fontFamily = Mono),
                    cursorBrush = SolidColor(Ink.accent),
                    keyboardOptions = KeyboardOptions(imeAction = ImeAction.Send, autoCorrectEnabled = false),
                    keyboardActions = KeyboardActions(onSend = { send() }),
                    modifier = Modifier.fillMaxWidth(),
                )
            }
            IconButton(onClick = { send() }, enabled = reply.isNotEmpty()) {
                SendGlyph(if (reply.isNotEmpty()) Ink.accent else Ink.tertiary, 24.dp)
            }
        }
    }
}

@Composable
private fun EmptyState(onAdd: () -> Unit) {
    Column(Modifier.fillMaxWidth().padding(start = 24.dp, end = 24.dp, top = 48.dp)) {
        Text("No servers yet", color = Ink.text, fontSize = 20.sp, fontWeight = FontWeight.SemiBold)
        Spacer(Modifier.height(8.dp))
        Text(
            "Raptor connects to a Rex server — on your Mac, or any machine on your tailnet — and keeps your sessions one tap away.",
            color = Ink.secondary,
            fontSize = 15.sp,
            lineHeight = 21.sp,
        )
        Spacer(Modifier.height(20.dp))
        TextButton(onClick = onAdd, contentPadding = PaddingValues(0.dp)) { Text("Add a server", fontSize = 17.sp, fontWeight = FontWeight.SemiBold, color = Ink.accent) }
    }
}
