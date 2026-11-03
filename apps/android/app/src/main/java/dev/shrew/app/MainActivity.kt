package dev.shrew.app

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
import dev.shrew.app.ui.HomeScreen
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
                val nav = rememberNavController()
                var sheet by remember { mutableStateOf(false) }
                var editing by remember { mutableStateOf<Server?>(null) }
                val pending by app.pendingOpen.collectAsState()
                LaunchedEffect(pending) {
                    val p = pending ?: return@LaunchedEffect
                    app.pendingOpen.value = null
                    nav.navigate("t/${p.serverId}/${p.sessionId}?block=${p.blockId.orEmpty()}") { launchSingleTop = true }
                }
                NavHost(
                    nav,
                    startDestination = "home",
                    enterTransition = { slideInHorizontally { it / 3 } + fadeIn() },
                    exitTransition = { fadeOut() },
                    popEnterTransition = { fadeIn() },
                    popExitTransition = { slideOutHorizontally { it / 3 } + fadeOut() },
                ) {
                    composable("home") {
                        HomeScreen(
                            onOpenSession = { s, ses, b -> nav.navigate("t/$s/$ses?block=${b.orEmpty()}") },
                            onAddServer = {
                                editing = null
                                sheet = true
                                maybeAskNotifications()
                            },
                            onEditServer = {
                                editing = it
                                sheet = true
                            },
                        )
                    }
                    composable(
                        "t/{server}/{session}?block={block}",
                        arguments = listOf(navArgument("block") { type = NavType.StringType; defaultValue = "" }),
                    ) { e ->
                        val a = e.arguments!!
                        TerminalScreen(
                            serverId = a.getString("server")!!,
                            sessionId = a.getString("session")!!,
                            initialBlockId = a.getString("block")?.ifEmpty { null },
                            onBack = { nav.popBackStack() },
                        )
                    }
                }
                if (sheet) AddServerSheet(editing) { sheet = false }
            }
        }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        handleIntent(intent)
    }

    private fun handleIntent(intent: Intent?) {
        val server = intent?.getStringExtra(ShrewApplication.EXTRA_SERVER) ?: return
        val session = intent.getStringExtra(ShrewApplication.EXTRA_SESSION) ?: return
        ShrewApplication.instance.pendingOpen.value = Focus(server, session, intent.getStringExtra(ShrewApplication.EXTRA_BLOCK))
    }

    private fun maybeAskNotifications() {
        if (Build.VERSION.SDK_INT >= 33) askNotifications.launch(Manifest.permission.POST_NOTIFICATIONS)
    }
}
