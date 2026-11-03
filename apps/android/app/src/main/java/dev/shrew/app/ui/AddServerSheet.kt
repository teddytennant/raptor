package dev.shrew.app.ui

import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.Box
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.TextButton
import androidx.compose.material.icons.filled.Sensors
import androidx.compose.material.icons.filled.Warning
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.sp
import androidx.compose.animation.AnimatedContent
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.CheckCircle
import androidx.compose.material.icons.filled.ErrorOutline
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
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
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import dev.shrew.app.ShrewApplication
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import uniffi.shrew_ffi.Server

private sealed interface Probe {
    data object Idle : Probe
    data object Testing : Probe
    data class Ok(val version: String) : Probe
    data class Failed(val message: String) : Probe
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun AddServerSheet(editing: Server?, onDismiss: () -> Unit) {
    val app = ShrewApplication.instance
    val sheet = rememberModalBottomSheetState(skipPartiallyExpanded = true)
    var name by remember { mutableStateOf(editing?.name.orEmpty()) }
    var address by remember { mutableStateOf(editing?.address.orEmpty()) }
    var probe by remember { mutableStateOf<Probe>(Probe.Idle) }
    var error by remember { mutableStateOf<String?>(null) }
    val scope = rememberCoroutineScope()
    val focus = remember { FocusRequester() }
    LaunchedEffect(Unit) { delay(250); runCatching { focus.requestFocus() } }

    // Test the address as the user types (debounced), so mistakes show up early.
    LaunchedEffect(address) {
        if (address.isBlank()) { probe = Probe.Idle; return@LaunchedEffect }
        delay(600)
        probe = Probe.Testing
        probe = runCatching { Probe.Ok(app.core.probe(address.trim())) }.getOrElse { Probe.Failed(it.message ?: "Can't connect") }
    }

    fun save() {
        if (address.isBlank()) return
        val r = runCatching {
            if (editing == null) app.core.addServer(name, address.trim()) else app.core.updateServer(editing.id, name.ifBlank { editing.name }, address.trim())
        }
        r.onSuccess { scope.launch { sheet.hide() }.invokeOnCompletion { onDismiss() } }.onFailure { error = it.message }
    }
    val st = LocalStatus.current

    // Laid out like the iOS form: an inline bar (Cancel · title · Save), then
    // grouped fields in one rounded card with a header and a footer.
    ModalBottomSheet(onDismissRequest = onDismiss, sheetState = sheet, containerColor = Ink.bg, dragHandle = null) {
        Column(Modifier.fillMaxWidth().navigationBarsPadding().imePadding()) {
            Box(Modifier.fillMaxWidth().height(56.dp).padding(horizontal = 8.dp)) {
                TextButton(onClick = { scope.launch { sheet.hide() }.invokeOnCompletion { onDismiss() } }, modifier = Modifier.align(Alignment.CenterStart)) {
                    Text("Cancel", color = Ink.accent, fontSize = 17.sp)
                }
                Text(
                    if (editing == null) "Add Server" else "Edit Server",
                    color = Ink.text,
                    fontSize = 17.sp,
                    fontWeight = FontWeight.SemiBold,
                    modifier = Modifier.align(Alignment.Center),
                )
                TextButton(onClick = ::save, enabled = address.isNotBlank(), modifier = Modifier.align(Alignment.CenterEnd)) {
                    Text("Save", color = if (address.isNotBlank()) Ink.accent else Ink.tertiary, fontSize = 17.sp, fontWeight = FontWeight.SemiBold)
                }
            }
            Text("Address", color = Ink.secondary, fontSize = 13.sp, modifier = Modifier.padding(start = 36.dp, top = 12.dp, bottom = 6.dp))
            Column(Modifier.padding(horizontal = 16.dp).fillMaxWidth().clip(RoundedCornerShape(12.dp)).background(Ink.surface)) {
                FormField(
                    address,
                    { address = it; error = null },
                    placeholder = "my-box.tail1234.ts.net",
                    mono = true,
                    keyboard = KeyboardOptions(keyboardType = KeyboardType.Uri, imeAction = ImeAction.Next, autoCorrectEnabled = false),
                    modifier = Modifier.focusRequester(focus),
                )
                HorizontalDivider(Modifier.padding(start = 20.dp), color = Ink.hairline)
                FormField(
                    name,
                    { name = it },
                    placeholder = "Name (optional)",
                    keyboard = KeyboardOptions(imeAction = ImeAction.Done),
                    actions = KeyboardActions(onDone = { save() }),
                )
            }
            Text(
                "Anything you'd pass to rex -S: a tailnet name, an IP with port like 100.64.0.2:7878, or a hostname.",
                color = Ink.secondary,
                fontSize = 13.sp,
                lineHeight = 18.sp,
                modifier = Modifier.padding(start = 36.dp, end = 36.dp, top = 8.dp),
            )
            Row(
                Modifier.padding(start = 16.dp, end = 16.dp, top = 24.dp).fillMaxWidth().clip(RoundedCornerShape(12.dp)).background(Ink.surface).padding(horizontal = 20.dp, vertical = 14.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                when (val p = probe) {
                    Probe.Idle -> {
                        Icon(Icons.Default.Sensors, null, tint = Ink.secondary, modifier = Modifier.size(20.dp))
                        Spacer(Modifier.width(12.dp))
                        Text("We'll test the connection as you type.", color = Ink.secondary, fontSize = 16.sp)
                    }
                    Probe.Testing -> {
                        CircularProgressIndicator(Modifier.size(18.dp), strokeWidth = 2.dp, color = Ink.secondary)
                        Spacer(Modifier.width(12.dp))
                        Text("Connecting…", color = Ink.secondary, fontSize = 16.sp)
                    }
                    is Probe.Ok -> {
                        Icon(Icons.Default.CheckCircle, null, tint = Ink.accent, modifier = Modifier.size(20.dp))
                        Spacer(Modifier.width(12.dp))
                        Text("Connected · Rex ${p.version}", color = Ink.accent, fontSize = 16.sp)
                    }
                    is Probe.Failed -> {
                        Icon(Icons.Default.Warning, null, tint = st.error, modifier = Modifier.size(20.dp))
                        Spacer(Modifier.width(12.dp))
                        Text(p.message, color = st.error, fontSize = 16.sp)
                    }
                }
            }
            error?.let {
                Text(it, color = st.error, fontSize = 15.sp, modifier = Modifier.padding(start = 16.dp, end = 16.dp, top = 16.dp).fillMaxWidth().clip(RoundedCornerShape(12.dp)).background(Ink.surface).padding(horizontal = 20.dp, vertical = 14.dp))
            }
            Spacer(Modifier.height(28.dp))
        }
    }
}

@Composable
private fun FormField(
    value: String,
    onChange: (String) -> Unit,
    placeholder: String,
    mono: Boolean = false,
    keyboard: KeyboardOptions = KeyboardOptions.Default,
    actions: KeyboardActions = KeyboardActions.Default,
    modifier: Modifier = Modifier,
) {
    Box(Modifier.fillMaxWidth().heightIn(min = 52.dp).padding(horizontal = 20.dp), contentAlignment = Alignment.CenterStart) {
        val style = TextStyle(color = Ink.text, fontSize = 17.sp, fontFamily = if (mono) Mono else null)
        if (value.isEmpty()) Text(placeholder, style = style.copy(color = Ink.tertiary))
        BasicTextField(value, onChange, singleLine = true, textStyle = style, cursorBrush = SolidColor(Ink.accent), keyboardOptions = keyboard, keyboardActions = actions, modifier = modifier.fillMaxWidth())
    }
}
