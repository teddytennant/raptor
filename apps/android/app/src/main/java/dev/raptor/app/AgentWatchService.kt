package dev.raptor.app

import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Build
import androidx.core.app.NotificationCompat
import androidx.core.app.ServiceCompat
import androidx.core.content.ContextCompat
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.launch
import uniffi.raptor_ffi.AgentState
import uniffi.raptor_ffi.AppState

/**
 * Keeps the connection open after you leave the app, but only while an agent is
 * working or waiting for you, so its "needs you" and "finished" notifications
 * still arrive. Once every agent is idle the service stops and the app sleeps.
 */
class AgentWatchService : Service() {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main)

    override fun onBind(intent: Intent?) = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        val app = RaptorApplication.instance
        show(activeAgents(app.state.value))
        scope.launch {
            app.state.map(::activeAgents).distinctUntilChanged().collect { n ->
                if (n == 0) stop() else show(n)
            }
        }
        return START_NOT_STICKY
    }

    private fun show(n: Int) {
        val open = PendingIntent.getActivity(this, 0, Intent(this, MainActivity::class.java), PendingIntent.FLAG_IMMUTABLE)
        val notification = NotificationCompat.Builder(this, RaptorApplication.CHANNEL_WATCH)
            .setSmallIcon(R.drawable.ic_notification)
            .setContentTitle(if (n == 1) "Watching 1 agent" else "Watching $n agents")
            .setContentText("You'll get a notification when one needs you")
            .setContentIntent(open)
            .setOngoing(true)
            .setSilent(true)
            .build()
        val type = if (Build.VERSION.SDK_INT >= 34) ServiceInfo.FOREGROUND_SERVICE_TYPE_REMOTE_MESSAGING else 0
        ServiceCompat.startForeground(this, ID, notification, type)
    }

    private fun stop() {
        RaptorApplication.instance.watching(false)
        ServiceCompat.stopForeground(this, ServiceCompat.STOP_FOREGROUND_REMOVE)
        stopSelf()
    }

    override fun onDestroy() {
        scope.cancel()
        super.onDestroy()
    }

    companion object {
        private const val ID = 1

        /** Agents that are working or waiting for you, on every server. */
        fun activeAgents(s: AppState): Int = s.servers.sumOf { srv ->
            srv.sessions.sumOf { ses ->
                ses.windows.sumOf { w -> w.blocks.count { it.agent?.state == AgentState.WORKING || it.agent?.state == AgentState.BLOCKED } }
            }
        }

        fun start(context: Context) = ContextCompat.startForegroundService(context, Intent(context, AgentWatchService::class.java))
    }
}
