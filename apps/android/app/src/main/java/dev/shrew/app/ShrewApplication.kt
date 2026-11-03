package dev.shrew.app

import android.Manifest
import android.app.Application
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import android.provider.Settings
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import androidx.core.content.ContextCompat
import androidx.lifecycle.DefaultLifecycleObserver
import androidx.lifecycle.LifecycleOwner
import androidx.lifecycle.ProcessLifecycleOwner
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import uniffi.shrew_ffi.AppListener
import uniffi.shrew_ffi.AppState
import uniffi.shrew_ffi.Notification
import uniffi.shrew_ffi.NotificationKind
import uniffi.shrew_ffi.ShrewApp

/** Where the app is looking right now, so we don't notify about what's on screen. */
data class Focus(val serverId: String, val sessionId: String, val blockId: String?)

class ShrewApplication : Application() {
    lateinit var core: ShrewApp
        private set

    private val _state = MutableStateFlow(AppState(emptyList(), emptyList()))
    val state: StateFlow<AppState> = _state.asStateFlow()

    /** Set by the UI while a session/terminal is visible. */
    @Volatile var focus: Focus? = null
    @Volatile var inForeground = true

    /** The color theme id (see ui.Themes); persisted in the engine's settings. */
    val themeId = MutableStateFlow("shrew")

    fun setTheme(id: String) {
        themeId.value = id
        core.setSetting("theme", id)
    }

    /** Deep link requested by tapping a notification. */
    val pendingOpen = MutableStateFlow<Focus?>(null)
    /** A shrew://open link waiting for its server's sessions to load: (session, server?). */
    val pendingLink = MutableStateFlow<Pair<String, String?>?>(null)

    override fun onCreate() {
        super.onCreate()
        instance = this
        createChannels()
        val label = Settings.Global.getString(contentResolver, Settings.Global.DEVICE_NAME) ?: Build.MODEL
        core = ShrewApp(filesDir.absolutePath, "android", label, object : AppListener {
            override fun stateChanged() {
                _state.value = core.state()
            }

            override fun notify(notification: Notification) = post(notification)
        })
        _state.value = core.state()
        core.setting("theme")?.let { themeId.value = it }
        ProcessLifecycleOwner.get().lifecycle.addObserver(object : DefaultLifecycleObserver {
            override fun onStart(owner: LifecycleOwner) {
                inForeground = true
                core.setForeground(true)
            }

            override fun onStop(owner: LifecycleOwner) {
                inForeground = false
                core.setForeground(false)
            }
        })
    }

    private fun createChannels() {
        val nm = getSystemService(NotificationManager::class.java)
        nm.createNotificationChannel(
            NotificationChannel(CHANNEL_AGENTS, getString(R.string.channel_agents), NotificationManager.IMPORTANCE_HIGH).apply {
                description = getString(R.string.channel_agents_desc)
            },
        )
        nm.createNotificationChannel(
            NotificationChannel(CHANNEL_PROGRAMS, getString(R.string.channel_programs), NotificationManager.IMPORTANCE_DEFAULT).apply {
                description = getString(R.string.channel_programs_desc)
            },
        )
    }

    private fun post(n: Notification) {
        val f = focus
        val looking = inForeground && f != null && f.serverId == n.serverId && f.sessionId == n.sessionId &&
            (f.blockId == null || f.blockId == n.blockId)
        if (looking) return
        if (n.kind == NotificationKind.BELL && inForeground) return
        // What you've chosen to hear about in Settings.
        val wanted = when (n.kind) {
            NotificationKind.AGENT_BLOCKED, NotificationKind.AGENT_ERROR -> dev.shrew.app.ui.Prefs.flag(dev.shrew.app.ui.Prefs.NOTIFY_BLOCKED)
            NotificationKind.AGENT_DONE -> dev.shrew.app.ui.Prefs.flag(dev.shrew.app.ui.Prefs.NOTIFY_DONE)
            NotificationKind.PROCESS_EXITED -> dev.shrew.app.ui.Prefs.flag(dev.shrew.app.ui.Prefs.NOTIFY_EXIT, default = false)
            else -> dev.shrew.app.ui.Prefs.flag(dev.shrew.app.ui.Prefs.NOTIFY_PROGRAM)
        }
        if (!wanted) return
        if (Build.VERSION.SDK_INT >= 33 &&
            ContextCompat.checkSelfPermission(this, Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED
        ) return

        val agent = n.kind == NotificationKind.AGENT_BLOCKED || n.kind == NotificationKind.AGENT_DONE || n.kind == NotificationKind.AGENT_ERROR
        val intent = Intent(this, MainActivity::class.java).apply {
            flags = Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP
            putExtra(EXTRA_SERVER, n.serverId)
            putExtra(EXTRA_SESSION, n.sessionId)
            putExtra(EXTRA_BLOCK, n.blockId)
        }
        val id = (n.serverId + n.blockId).hashCode()
        val pi = PendingIntent.getActivity(this, id, intent, PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE)
        val title = when (n.kind) {
            NotificationKind.AGENT_BLOCKED -> "${n.title} needs you"
            NotificationKind.AGENT_DONE -> "${n.title} finished"
            NotificationKind.AGENT_ERROR -> "${n.title} hit an error"
            else -> n.title
        }
        val notif = NotificationCompat.Builder(this, if (agent) CHANNEL_AGENTS else CHANNEL_PROGRAMS)
            .setSmallIcon(R.drawable.ic_foreground)
            .setContentTitle(title)
            .setContentText(n.body)
            .setStyle(NotificationCompat.BigTextStyle().bigText(n.body))
            .setCategory(if (agent) NotificationCompat.CATEGORY_STATUS else NotificationCompat.CATEGORY_EVENT)
            .setPriority(if (n.kind == NotificationKind.AGENT_BLOCKED) NotificationCompat.PRIORITY_HIGH else NotificationCompat.PRIORITY_DEFAULT)
            .setColor(0xFF34D399.toInt())
            .setContentIntent(pi)
            .setAutoCancel(true)
            .setGroup("shrew")
            .apply {
                if (!agent) return@apply
                // Reply from the notification (like iOS), or open the pane.
                val replyIntent = Intent(this@ShrewApplication, ReplyReceiver::class.java).apply {
                    putExtra(EXTRA_SERVER, n.serverId)
                    putExtra(EXTRA_SESSION, n.sessionId)
                    putExtra(EXTRA_BLOCK, n.blockId)
                    putExtra(ReplyReceiver.EXTRA_ID, id)
                }
                val replyPi = PendingIntent.getBroadcast(this@ShrewApplication, id, replyIntent, PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_MUTABLE)
                val input = androidx.core.app.RemoteInput.Builder(ReplyReceiver.KEY_REPLY).setLabel("Type a reply").build()
                addAction(
                    NotificationCompat.Action.Builder(R.drawable.ic_foreground, "Reply", replyPi)
                        .addRemoteInput(input)
                        .setAllowGeneratedReplies(false)
                        .setSemanticAction(NotificationCompat.Action.SEMANTIC_ACTION_REPLY)
                        .build(),
                )
                addAction(NotificationCompat.Action.Builder(R.drawable.ic_foreground, "Open", pi).build())
            }
            .build()
        NotificationManagerCompat.from(this).notify(id, notif)
    }

    companion object {
        lateinit var instance: ShrewApplication
            private set
        const val CHANNEL_AGENTS = "agents"
        const val CHANNEL_PROGRAMS = "programs"
        const val EXTRA_SERVER = "server"
        const val EXTRA_SESSION = "session"
        const val EXTRA_BLOCK = "block"
    }
}
