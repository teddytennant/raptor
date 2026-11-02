package dev.shrew.app.ui

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

    ModalBottomSheet(onDismissRequest = onDismiss, sheetState = sheet) {
        Column(
            Modifier.fillMaxWidth().padding(horizontal = 20.dp).navigationBarsPadding().imePadding(),
            verticalArrangement = Arrangement.spacedBy(14.dp),
        ) {
            Text(if (editing == null) "Add a server" else "Edit server", style = MaterialTheme.typography.titleLarge)
            Text(
                "Use the address you'd give rex -S: a tailnet name like studio.tail1234.ts.net, an IP with port (100.64.0.2:7878), or a hostname.",
                style = MaterialTheme.typography.bodyMedium,
                color = LocalStatus.current.muted,
            )
            OutlinedTextField(
                value = address,
                onValueChange = { address = it; error = null },
                label = { Text("Address") },
                placeholder = { Text("my-box.tail1234.ts.net") },
                singleLine = true,
                textStyle = MonoSmall.copy(fontSize = MaterialTheme.typography.bodyLarge.fontSize),
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri, imeAction = ImeAction.Next, autoCorrectEnabled = false),
                modifier = Modifier.fillMaxWidth().focusRequester(focus),
                shape = RoundedCornerShape(12.dp),
            )
            OutlinedTextField(
                value = name,
                onValueChange = { name = it },
                label = { Text("Name (optional)") },
                placeholder = { Text("Studio") },
                singleLine = true,
                modifier = Modifier.fillMaxWidth(),
                shape = RoundedCornerShape(12.dp),
            )
            AnimatedContent(probe, label = "probe") { p ->
                val st = LocalStatus.current
                Row(
                    Modifier.fillMaxWidth().clip(RoundedCornerShape(12.dp)).background(MaterialTheme.colorScheme.surfaceContainerHigh).padding(12.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    when (p) {
                        Probe.Idle -> Text("We'll test the connection as you type.", color = st.muted, style = MaterialTheme.typography.bodySmall)
                        Probe.Testing -> {
                            CircularProgressIndicator(Modifier.size(16.dp), strokeWidth = 2.dp)
                            Spacer(Modifier.width(10.dp))
                            Text("Connecting…", style = MaterialTheme.typography.bodySmall)
                        }
                        is Probe.Ok -> {
                            Icon(Icons.Default.CheckCircle, null, tint = st.online, modifier = Modifier.size(18.dp))
                            Spacer(Modifier.width(10.dp))
                            Text("Connected · Rex ${p.version}", style = MaterialTheme.typography.bodySmall)
                        }
                        is Probe.Failed -> {
                            Icon(Icons.Default.ErrorOutline, null, tint = st.error, modifier = Modifier.size(18.dp))
                            Spacer(Modifier.width(10.dp))
                            Text(p.message, style = MaterialTheme.typography.bodySmall)
                        }
                    }
                }
            }
            error?.let { Text(it, color = MaterialTheme.colorScheme.error, style = MaterialTheme.typography.bodySmall) }
            Row(horizontalArrangement = Arrangement.spacedBy(10.dp), modifier = Modifier.padding(bottom = 16.dp)) {
                OutlinedButton(onClick = onDismiss, modifier = Modifier.weight(1f)) { Text("Cancel") }
                Button(
                    enabled = address.isNotBlank(),
                    modifier = Modifier.weight(1f),
                    onClick = {
                        val r = runCatching {
                            if (editing == null) app.core.addServer(name, address) else app.core.updateServer(editing.id, name.ifBlank { editing.name }, address)
                        }
                        r.onSuccess { scope.launch { sheet.hide(); onDismiss() } }.onFailure { error = it.message }
                    },
                ) { Text(if (probe is Probe.Ok || editing != null) "Save" else "Save anyway") }
            }
        }
    }
}
