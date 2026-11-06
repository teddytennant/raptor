package dev.raptor.app

import androidx.compose.foundation.layout.size
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.Settings
import androidx.compose.foundation.layout.fillMaxSize
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
import dev.raptor.app.ui.AddServerSheet
import dev.raptor.app.ui.RaptorTheme
import dev.raptor.app.ui.TerminalScreen
import uniffi.raptor_ffi.Server

class MainActivity : ComponentActivity() {
    private val askNotifications = registerForActivityResult(ActivityResultContracts.RequestPermission()) {}

    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge(SystemBarStyle.dark(android.graphics.Color.TRANSPARENT), SystemBarStyle.dark(android.graphics.Color.TRANSPARENT))
        super.onCreate(savedInstanceState)
        handleIntent(intent)
        val app = RaptorApplication.instance
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
            RaptorTheme(dev.raptor.app.ui.Chrome.of(dev.raptor.app.ui.themeById(themeId))) {
                var sheet by remember { mutableStateOf(false) }
                var editing by remember { mutableStateOf<Server?>(null) }
                val pending by app.pendingOpen.collectAsState()
                // raptor://open?session=…&server=…: resolve labels or ids once sessions load.
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
                // The app opens straight into your last session. Back from a terminal
                // goes to the session list; the switcher sheet jumps between sessions.
                var current by remember { mutableStateOf(lastOpen()) }
                var switcher by remember { mutableStateOf(false) }
                var settings by remember { mutableStateOf(false) }
                fun open(f: Focus?) {
                    current = f
                    app.core.setSetting("last_open", f?.let { "${it.serverId}|${it.sessionId}" } ?: "")
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
                    val gone = srv == null || (srv.connectivity is uniffi.raptor_ffi.Connectivity.Online && srv.sessions.none { it.id == c.sessionId })
                    if (gone) open(null)
                }
                val addServer = {
                    editing = null
                    sheet = true
                }
                val editServer = { s: Server -> editing = s; sheet = true }
                val home = @androidx.compose.runtime.Composable { modifier: androidx.compose.ui.Modifier ->
                    dev.raptor.app.ui.Home(current, onOpen = ::open, onAddServer = addServer, onEditServer = editServer, onSettings = { settings = true }, modifier = modifier)
                }

                when {
                    appState.servers.isEmpty() -> dev.raptor.app.ui.Welcome(addServer)
                    else -> dev.raptor.app.ui.BackToHome(current, onBack = { open(null) }, home = { home(androidx.compose.ui.Modifier.fillMaxSize()) }) { c ->
                        TerminalScreen(c.serverId, c.sessionId, c.blockId, onSessions = { switcher = true }, onOpen = ::open)
                    }
                }
                if (switcher) {
                    dev.raptor.app.ui.SessionSheet(
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
                    dev.raptor.app.ui.SettingsScreen(onClose = { settings = false }, onAddServer = addServer, onEditServer = editServer)
                }
                if (sheet) AddServerSheet(editing, onAdded = ::maybeAskNotifications) { sheet = false }
            }
        }
    }

    override fun onResume() {
        super.onResume()
        RaptorApplication.instance.stopWatching()
    }

    override fun onPause() {
        super.onPause()
        if (!isChangingConfigurations) RaptorApplication.instance.maybeWatch()
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        handleIntent(intent)
    }

    /** The session that was open last time, if any. */
    private fun lastOpen(): Focus? {
        val v = RaptorApplication.instance.core.setting("last_open") ?: return null
        val parts = v.split('|')
        return if (parts.size >= 2) Focus(parts[0], parts[1], null) else null
    }

    private fun handleIntent(intent: Intent?) {
        val uri = intent?.data
        if (uri != null && uri.scheme == "raptor" && uri.host == "open") {
            val session = uri.getQueryParameter("session") ?: return
            RaptorApplication.instance.pendingLink.value = session to uri.getQueryParameter("server")
            return
        }
        val server = intent?.getStringExtra(RaptorApplication.EXTRA_SERVER) ?: return
        val session = intent.getStringExtra(RaptorApplication.EXTRA_SESSION) ?: return
        RaptorApplication.instance.pendingOpen.value = Focus(server, session, intent.getStringExtra(RaptorApplication.EXTRA_BLOCK))
    }

    private fun maybeAskNotifications() {
        if (Build.VERSION.SDK_INT >= 33) askNotifications.launch(Manifest.permission.POST_NOTIFICATIONS)
    }
}
