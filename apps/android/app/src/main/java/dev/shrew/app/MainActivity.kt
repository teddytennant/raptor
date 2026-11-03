package dev.shrew.app

import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.ui.unit.sp
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.Settings
import dev.shrew.app.ui.SessionList
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.width
import androidx.compose.ui.unit.dp
import android.Manifest
import android.content.Intent
import android.os.Build
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.SystemBarStyle
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.slideInHorizontally
import androidx.compose.animation.slideOutHorizontally
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.navigation.NavType
import androidx.navigation.compose.NavHost
import androidx.navigation.compose.composable
import androidx.navigation.compose.rememberNavController
import androidx.navigation.navArgument
import dev.shrew.app.ui.AddServerSheet
import dev.shrew.app.ui.ShrewTheme
import dev.shrew.app.ui.TerminalScreen
import uniffi.shrew_ffi.Server

class MainActivity : ComponentActivity() {
    private val askNotifications = registerForActivityResult(ActivityResultContracts.RequestPermission()) {}

    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge(SystemBarStyle.dark(android.graphics.Color.TRANSPARENT), SystemBarStyle.dark(android.graphics.Color.TRANSPARENT))
        super.onCreate(savedInstanceState)
        handleIntent(intent)
        val app = ShrewApplication.instance
        // With a hardware keyboard Android leaves touch mode and paints a default
        // focus highlight over any focused View that has no focus style of its
        // own. The Compose root fills the window, so that reads as a grey wash
        // over the whole app. Compose draws its own focus states; turn this off.
        window.decorView.viewTreeObserver.addOnGlobalLayoutListener {
            fun off(v: android.view.View) {
                v.defaultFocusHighlightEnabled = false
                if (v is android.view.ViewGroup) for (i in 0 until v.childCount) off(v.getChildAt(i))
            }
            off(window.decorView)
        }
        setContent {
            val themeId by app.themeId.collectAsState()
            ShrewTheme(dev.shrew.app.ui.Chrome.of(dev.shrew.app.ui.themeById(themeId))) {
                var sheet by remember { mutableStateOf(false) }
                var editing by remember { mutableStateOf<Server?>(null) }
                val pending by app.pendingOpen.collectAsState()
                // shrew://open?session=…&server=…: resolve labels or ids once sessions load.
                val link by app.pendingLink.collectAsState()
                val appState by app.state.collectAsState()
                LaunchedEffect(link, appState) {
                    val (want, srvWant) = link ?: return@LaunchedEffect
                    for (srv in appState.servers) {
                        if (srvWant != null && srv.id != srvWant && srv.name != srvWant) continue
                        val s = srv.sessions.firstOrNull { it.id == want || it.label == want } ?: continue
                        app.pendingLink.value = null
                        app.pendingOpen.value = Focus(srv.id, s.id, null)
                        return@LaunchedEffect
                    }
                }
                // The app opens straight into your last session. Everything else
                // (other sessions, servers, settings) is one tap away in the switcher.
                val wide = androidx.compose.ui.platform.LocalConfiguration.current.screenWidthDp >= 720
                var current by remember { mutableStateOf(lastOpen()) }
                var switcher by remember { mutableStateOf(false) }
                var settings by remember { mutableStateOf(false) }
                fun open(f: Focus) {
                    current = f
                    app.core.setSetting("last_open", "${f.serverId}|${f.sessionId}")
                }
                LaunchedEffect(pending) {
                    val p = pending ?: return@LaunchedEffect
                    app.pendingOpen.value = null
                    open(p)
                }
                // If the saved session is gone, forget it. Never open a session the
                // user didn't pick: opening one fits it to the phone, which resizes
                // it for everyone else watching it.
                LaunchedEffect(appState, current) {
                    val c = current ?: return@LaunchedEffect
                    val srv = appState.servers.firstOrNull { it.id == c.serverId }
                    val gone = srv == null || (srv.connectivity is uniffi.shrew_ffi.Connectivity.Online && srv.sessions.none { it.id == c.sessionId })
                    if (gone && appState.servers.isNotEmpty()) {
                        current = null
                        app.core.setSetting("last_open", "")
                        switcher = true
                    }
                }
                // Nothing open yet: offer the switcher once.
                LaunchedEffect(Unit) { if (current == null && appState.servers.isNotEmpty()) switcher = true }
                val addServer = {
                    editing = null
                    sheet = true
                    maybeAskNotifications()
                }
                val editServer = { s: Server -> editing = s; sheet = true }

                val terminal = @androidx.compose.runtime.Composable {
                    val c = current
                    when {
                        appState.servers.isEmpty() -> dev.shrew.app.ui.Welcome(addServer)
                        c == null -> dev.shrew.app.ui.PickASession(onSessions = { switcher = true })
                        else -> androidx.compose.runtime.key(c) {
                            TerminalScreen(c.serverId, c.sessionId, c.blockId, onSessions = { switcher = true }, onOpen = ::open)
                        }
                    }
                }
                if (wide && appState.servers.isNotEmpty()) {
                    // Tablets and unfolded foldables: sessions beside the terminal.
                    androidx.compose.foundation.layout.Row(androidx.compose.ui.Modifier.fillMaxSize().background(dev.shrew.app.ui.Ink.bg)) {
                        androidx.compose.foundation.layout.Column(androidx.compose.ui.Modifier.width(340.dp).fillMaxHeight().statusBarsPadding()) {
                            androidx.compose.foundation.layout.Row(
                                androidx.compose.ui.Modifier.fillMaxWidth().padding(start = 20.dp, end = 12.dp, top = 8.dp, bottom = 8.dp),
                                verticalAlignment = androidx.compose.ui.Alignment.CenterVertically,
                            ) {
                                androidx.compose.material3.Text("Sessions", color = dev.shrew.app.ui.Ink.text, fontSize = 22.sp, fontWeight = androidx.compose.ui.text.font.FontWeight.Bold, modifier = androidx.compose.ui.Modifier.weight(1f))
                                dev.shrew.app.ui.GlassCircle(size = 40.dp, onClick = { settings = true }) {
                                    androidx.compose.material3.Icon(androidx.compose.material.icons.Icons.Default.Settings, "Settings", tint = dev.shrew.app.ui.Ink.text, modifier = androidx.compose.ui.Modifier.size(20.dp))
                                }
                                androidx.compose.foundation.layout.Spacer(androidx.compose.ui.Modifier.width(8.dp))
                                dev.shrew.app.ui.GlassCircle(size = 40.dp, onClick = addServer) {
                                    androidx.compose.material3.Icon(androidx.compose.material.icons.Icons.Default.Add, "Add server", tint = dev.shrew.app.ui.Ink.accent, modifier = androidx.compose.ui.Modifier.size(22.dp))
                                }
                            }
                            SessionList(current, onOpenSession = { s, ses, b -> open(Focus(s, ses, b)) }, onAddServer = addServer, onEditServer = editServer)
                        }
                        androidx.compose.material3.VerticalDivider(color = dev.shrew.app.ui.Ink.hairline)
                        androidx.compose.foundation.layout.Box(androidx.compose.ui.Modifier.weight(1f).fillMaxHeight()) { terminal() }
                    }
                } else {
                    terminal()
                }
                if (switcher) {
                    dev.shrew.app.ui.SessionSheet(
                        current = current,
                        onOpen = ::open,
                        onAddServer = addServer,
                        onEditServer = editServer,
                        onSettings = { settings = true },
                        onDismiss = { switcher = false },
                    )
                }
                androidx.compose.animation.AnimatedVisibility(
                    settings,
                    enter = androidx.compose.animation.slideInHorizontally { it } + fadeIn(),
                    exit = androidx.compose.animation.slideOutHorizontally { it } + fadeOut(),
                ) {
                    dev.shrew.app.ui.SettingsScreen(onClose = { settings = false }, onAddServer = addServer, onEditServer = editServer)
                }
                if (sheet) AddServerSheet(editing) { sheet = false }
            }
        }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        handleIntent(intent)
    }

    /** The session that was open last time, if any. */
    private fun lastOpen(): Focus? {
        val v = ShrewApplication.instance.core.setting("last_open") ?: return null
        val parts = v.split('|')
        return if (parts.size >= 2) Focus(parts[0], parts[1], null) else null
    }

    private fun handleIntent(intent: Intent?) {
        val uri = intent?.data
        if (uri != null && uri.scheme == "shrew" && uri.host == "open") {
            val session = uri.getQueryParameter("session") ?: return
            ShrewApplication.instance.pendingLink.value = session to uri.getQueryParameter("server")
            return
        }
        val server = intent?.getStringExtra(ShrewApplication.EXTRA_SERVER) ?: return
        val session = intent.getStringExtra(ShrewApplication.EXTRA_SESSION) ?: return
        ShrewApplication.instance.pendingOpen.value = Focus(server, session, intent.getStringExtra(ShrewApplication.EXTRA_BLOCK))
    }

    private fun maybeAskNotifications() {
        if (Build.VERSION.SDK_INT >= 33) askNotifications.launch(Manifest.permission.POST_NOTIFICATIONS)
    }
}
