package dev.shrew.app.ui

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
import dev.shrew.app.ShrewApplication
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

/** One row in the command palette. */
data class PaletteItem(
    val id: String,
    val title: String,
    val subtitle: String = "",
    val category: String = "Shrew",
    val keywords: String = "",
    val glyph: String = "›",
    val run: () -> Unit,
)

/**
 * Everything you can do here, searchable: Shrew's own commands plus the server's
 * actions (its Lua config's `rex.action`s and block actions like Clear Terminal).
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun CommandPalette(serverId: String, sessionId: String, blockId: String?, local: List<PaletteItem>, onDismiss: () -> Unit) {
    val app = ShrewApplication.instance
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
    fun pick(i: PaletteItem) {
        scope.launch { sheet.hide(); onDismiss() }
        i.run()
    }

    ModalBottomSheet(onDismissRequest = onDismiss, sheetState = sheet, containerColor = Ink.surface) {
        Column(Modifier.navigationBarsPadding().imePadding()) {
            Row(
                Modifier.padding(horizontal = 16.dp).fillMaxWidth().clip(RoundedCornerShape(12.dp)).background(Ink.raised).padding(horizontal = 12.dp, vertical = 10.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Icon(Icons.Default.Search, null, tint = Ink.tertiary, modifier = Modifier.size(18.dp))
                Spacer(Modifier.width(8.dp))
                Box(Modifier.weight(1f)) {
                    if (query.isEmpty()) Text("Run a command", color = Ink.tertiary, fontSize = 15.sp)
                    BasicTextField(
                        query,
                        { query = it },
                        singleLine = true,
                        textStyle = TextStyle(color = Ink.text, fontSize = 15.sp),
                        cursorBrush = SolidColor(Ink.accent),
                        keyboardOptions = KeyboardOptions(imeAction = ImeAction.Go, autoCorrectEnabled = false),
                        keyboardActions = KeyboardActions(onGo = { shown.firstOrNull()?.let(::pick) }),
                        modifier = Modifier.fillMaxWidth().focusRequester(focus),
                    )
                }
                if (server == null) CircularProgressIndicator(Modifier.size(14.dp), strokeWidth = 1.5.dp, color = Ink.accent)
            }
            LazyColumn(Modifier.fillMaxWidth().heightIn(max = 480.dp).padding(top = 8.dp, bottom = 8.dp)) {
                var last: String? = null
                for (i in shown) {
                    if (i.category != last) {
                        val cat = i.category
                        item(key = "h-$cat-${i.id}") {
                            Text(cat.uppercase(), color = Ink.tertiary, fontSize = 11.sp, fontWeight = FontWeight.Medium, modifier = Modifier.padding(start = 20.dp, top = 14.dp, bottom = 4.dp))
                        }
                        last = cat
                    }
                    item(key = i.id) {
                        Row(
                            Modifier.fillMaxWidth().clickable { pick(i) }.padding(horizontal = 20.dp, vertical = 10.dp),
                            verticalAlignment = Alignment.CenterVertically,
                            horizontalArrangement = Arrangement.spacedBy(12.dp),
                        ) {
                            Text(i.glyph, color = Ink.accent, fontFamily = Mono, fontSize = 14.sp, modifier = Modifier.width(18.dp))
                            Column(Modifier.weight(1f)) {
                                Text(i.title, color = Ink.text, fontSize = 15.sp)
                                if (i.subtitle.isNotEmpty()) Text(i.subtitle, color = Ink.secondary, fontSize = 12.sp, maxLines = 1)
                            }
                        }
                    }
                }
            }
        }
    }
}
