package dev.shrew.app.ui

import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.scale
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import uniffi.shrew_ffi.AgentState
import uniffi.shrew_ffi.AgentStatus
import uniffi.shrew_ffi.Connectivity
import uniffi.shrew_ffi.Window

/** A dot that breathes while something is in progress. */
@Composable
fun StatusDot(color: Color, pulsing: Boolean, size: Dp = 8.dp) {
    val t = rememberInfiniteTransition(label = "dot")
    val s by t.animateFloat(1f, if (pulsing) 1.6f else 1f, infiniteRepeatable(tween(900, easing = LinearEasing), RepeatMode.Reverse), label = "s")
    val a by t.animateFloat(0.45f, if (pulsing) 0f else 0.45f, infiniteRepeatable(tween(900, easing = LinearEasing), RepeatMode.Reverse), label = "a")
    Box(Modifier.size(size * 2), contentAlignment = Alignment.Center) {
        if (pulsing) Box(Modifier.size(size).scale(s).clip(CircleShape).background(color.copy(alpha = a)))
        Box(Modifier.size(size).clip(CircleShape).background(color))
    }
}

@Composable
fun connectivityColor(c: Connectivity): Color {
    val st = LocalStatus.current
    return when (c) {
        is Connectivity.Online -> st.online
        is Connectivity.Connecting -> st.connecting
        is Connectivity.Offline -> st.error.copy(alpha = 0.85f)
        is Connectivity.Idle -> st.offline
    }
}

fun connectivityLabel(c: Connectivity, latency: UInt?): String = when (c) {
    is Connectivity.Online -> if (latency != null) "Online · ${latency}ms" else "Online"
    is Connectivity.Connecting -> "Connecting…"
    is Connectivity.Offline -> c.error
    is Connectivity.Idle -> "Paused"
}

fun agentLabel(a: AgentStatus): String = when (a.state) {
    AgentState.BLOCKED -> when (a.waitingFor) {
        "permission" -> "Needs permission"
        "question" -> "Has a question"
        "auth" -> "Needs credentials"
        else -> "Waiting for you"
    }
    AgentState.WORKING -> a.progress?.let { "Working · $it%" } ?: "Working"
    AgentState.DONE -> "Done"
    AgentState.ERROR -> "Error"
    AgentState.IDLE -> "Idle"
}

/** A compact pill: colored dot + label, e.g. "● Needs permission". */
@Composable
fun AgentPill(a: AgentStatus, modifier: Modifier = Modifier) {
    val color = LocalStatus.current.agent(a.state)
    Row(
        modifier
            .clip(RoundedCornerShape(50))
            .background(color.copy(alpha = 0.14f))
            .padding(horizontal = 8.dp, vertical = 3.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(5.dp),
    ) {
        if (a.state == AgentState.WORKING) {
            CircularProgressIndicator(Modifier.size(9.dp), color = color, strokeWidth = 1.5.dp)
        } else {
            Box(Modifier.size(6.dp).clip(CircleShape).background(color))
        }
        Text(agentLabel(a), color = color, fontSize = 11.sp, fontWeight = FontWeight.SemiBold, maxLines = 1)
    }
}

/**
 * A miniature of a window's pane layout, with each pane tinted by its agent state.
 * Lets you recognize a session at a glance.
 */
@Composable
fun LayoutThumbnail(window: Window?, modifier: Modifier = Modifier, highlight: String? = null) {
    val st = LocalStatus.current
    val base = MaterialTheme.colorScheme.surfaceContainerHighest
    val stroke = MaterialTheme.colorScheme.outline
    val accent = MaterialTheme.colorScheme.primary
    Canvas(modifier) {
        val gap = 2.dp.toPx()
        val r = CornerRadius(3.dp.toPx())
        val blocks = window?.blocks.orEmpty()
        if (blocks.isEmpty()) {
            drawRoundRect(base, cornerRadius = r)
            return@Canvas
        }
        for (b in blocks) {
            val x = (b.x * size.width).toFloat() + gap / 2
            val y = (b.y * size.height).toFloat() + gap / 2
            val w = (b.w * size.width).toFloat() - gap
            val h = (b.h * size.height).toFloat() - gap
            val tint = b.agent?.let { st.agent(it.state) }
            drawRoundRect(tint?.copy(alpha = 0.28f) ?: base, Offset(x, y), Size(w, h), r)
            if (b.id == (highlight ?: window?.focusedBlockId)) {
                drawRoundRect(accent.copy(alpha = 0.9f), Offset(x, y), Size(w, h), r, style = Stroke(1.5.dp.toPx()))
            } else {
                drawRoundRect(stroke.copy(alpha = 0.6f), Offset(x, y), Size(w, h), r, style = Stroke(0.75.dp.toPx()))
            }
            // A few "text lines" so it reads as a terminal.
            val lineH = 2.dp.toPx()
            var ly = y + 4.dp.toPx()
            var i = 0
            while (ly + lineH < y + h - 3.dp.toPx() && i < 6) {
                val lw = (w - 8.dp.toPx()) * listOf(0.7f, 0.45f, 0.85f, 0.3f, 0.6f, 0.5f)[i]
                drawRoundRect((tint ?: stroke).copy(alpha = 0.55f), Offset(x + 4.dp.toPx(), ly), Size(lw.coerceAtLeast(0f), lineH), CornerRadius(1.dp.toPx()))
                ly += lineH * 2.2f
                i++
            }
        }
    }
}
