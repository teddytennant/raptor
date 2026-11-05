package dev.raptor.app.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Search
import androidx.compose.material.icons.filled.Close
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.HorizontalDivider
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Text
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import dev.raptor.app.RaptorApplication
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

/** One row in the command palette. */
data class PaletteItem(
    val id: String,
    val title: String,
    val subtitle: String = "",
    val category: String = "Raptor",
    val keywords: String = "",
    val glyph: String = "›",
    val run: () -> Unit,
)

/**
 * Everything you can do here, searchable: Raptor's own commands plus the server's
 * actions (its Lua config's `rex.action`s and block actions like Clear Terminal).
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun CommandPalette(serverId: String, sessionId: String, blockId: String?, local: List<PaletteItem>, onDismiss: () -> Unit) {
    val app = RaptorApplication.instance
    val sheet = rememberModalBottomSheetState(skipPartiallyExpanded = true)
    val scope = rememberCoroutineScope()
    var query by remember { mutableStateOf("") }
    var server by remember { mutableStateOf<List<PaletteItem>?>(null) }
    val focus = remember { FocusRequester() }

    LaunchedEffect(serverId) {
        server = runCatching { app.core.paletteActions(serverId) }.getOrDefault(emptyList()).map { a ->
            PaletteItem(
                id = a.name,
                title = a.title,
                subtitle = a.description,
                category = a.category.ifEmpty { "Actions" },
                keywords = a.keywords + " " + a.name,
                glyph = if (a.name.contains('.')) "◆" else "λ",
            ) { scope.launch { runCatching { app.core.runAction(serverId, sessionId, blockId, a.name) } } }
        }
    }
    LaunchedEffect(Unit) { delay(200); runCatching { focus.requestFocus() } }

    val all = local + server.orEmpty()
    val words = query.trim().lowercase().split(' ').filter { it.isNotEmpty() }
    val shown = all.filter { i ->
        val hay = "${i.title} ${i.subtitle} ${i.category} ${i.keywords}".lowercase()
        words.all { hay.contains(it) }
    }
    // Close first, then act: running the command mid-animation can interrupt the
    // hide, which would leave the sheet's scrim over the terminal.
    fun close(after: () -> Unit = {}) {
        scope.launch { sheet.hide() }.invokeOnCompletion {
            onDismiss()
            // Let the sheet's window (and its dim layer) finish going away before
            // the command changes the screen under it, or the dim can stick.
            android.os.Handler(android.os.Looper.getMainLooper()).postDelayed(after, 250)
        }
    }
    fun pick(i: PaletteItem) = close(i.run)

    ModalBottomSheet(onDismissRequest = onDismiss, sheetState = sheet, containerColor = Ink.bg, dragHandle = {
        Box(Modifier.padding(top = 8.dp, bottom = 10.dp).size(width = 40.dp, height = 4.dp).clip(RoundedCornerShape(2.dp)).background(Ink.tertiary.copy(alpha = 0.5f)))
    }) {
        Column(Modifier.navigationBarsPadding().imePadding()) {
            Row(Modifier.padding(horizontal = 16.dp).fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                Row(
                    Modifier.weight(1f).height(48.dp).glass(RoundedCornerShape(50)).padding(horizontal = 14.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Icon(Icons.Default.Search, null, tint = Ink.secondary, modifier = Modifier.size(20.dp))
                    Spacer(Modifier.width(10.dp))
                    Box(Modifier.weight(1f)) {
                        if (query.isEmpty()) Text("Run a command", color = Ink.tertiary, fontSize = 17.sp)
                        BasicTextField(
                            query,
                            { query = it },
                            singleLine = true,
                            textStyle = TextStyle(color = Ink.text, fontSize = 17.sp),
                            cursorBrush = SolidColor(Ink.accent),
                            keyboardOptions = KeyboardOptions(imeAction = ImeAction.Go, autoCorrectEnabled = false),
                            keyboardActions = KeyboardActions(onGo = { shown.firstOrNull()?.let(::pick) }),
                            modifier = Modifier.fillMaxWidth().focusRequester(focus),
                        )
                    }
                    if (server == null) CircularProgressIndicator(Modifier.size(14.dp), strokeWidth = 1.5.dp, color = Ink.accent)
                    else if (query.isNotEmpty()) Icon(Icons.Default.Close, "Clear", tint = Ink.secondary, modifier = Modifier.size(18.dp).clip(CircleShape).clickable { query = "" })
                }
                Spacer(Modifier.width(10.dp))
                GlassCircle(size = 48.dp, onClick = { close() }) {
                    Icon(Icons.Default.Close, "Close", tint = Ink.text, modifier = Modifier.size(22.dp))
                }
            }
            val groups = shown.groupBy { it.category }
            LazyColumn(Modifier.fillMaxWidth().heightIn(max = 520.dp).padding(top = 4.dp), contentPadding = PaddingValues(bottom = 16.dp)) {
                for ((cat, rows) in groups) {
                    item(key = "h-$cat") {
                        Text(cat, color = Ink.secondary, fontSize = 17.sp, fontWeight = FontWeight.SemiBold, modifier = Modifier.padding(start = 32.dp, top = 18.dp, bottom = 8.dp))
                    }
                    item(key = "c-$cat") {
                        Column(Modifier.padding(horizontal = 16.dp).fillMaxWidth().clip(RoundedCornerShape(22.dp)).background(Ink.surface)) {
                            rows.forEachIndexed { idx, i ->
                                Row(
                                    Modifier.fillMaxWidth().clickable { pick(i) }.padding(horizontal = 18.dp, vertical = 12.dp),
                                    verticalAlignment = Alignment.CenterVertically,
                                ) {
                                    Text(i.glyph, color = Ink.accent, fontFamily = Mono, fontSize = 16.sp, modifier = Modifier.width(26.dp))
                                    Spacer(Modifier.width(12.dp))
                                    Column(Modifier.weight(1f)) {
                                        Text(i.title, color = Ink.text, fontSize = 17.sp)
                                        if (i.subtitle.isNotEmpty()) Text(i.subtitle, color = Ink.secondary, fontSize = 14.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
                                    }
                                }
                                if (idx < rows.lastIndex) HorizontalDivider(Modifier.padding(start = 56.dp, end = 18.dp), color = Ink.hairline)
                            }
                        }
                    }
                }
                if (server == null) item(key = "loading") {
                    Text("Loading server actions…", color = Ink.tertiary, fontSize = 14.sp, modifier = Modifier.padding(start = 32.dp, top = 16.dp))
                }
            }
        }
    }
}
