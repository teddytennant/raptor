package dev.raptor.app

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import androidx.core.app.RemoteInput
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.launch

/** Answers an agent straight from its notification, without opening the app. */
class ReplyReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        val text = RemoteInput.getResultsFromIntent(intent)?.getCharSequence(KEY_REPLY)?.toString() ?: return
        val server = intent.getStringExtra(RaptorApplication.EXTRA_SERVER) ?: return
        val session = intent.getStringExtra(RaptorApplication.EXTRA_SESSION) ?: return
        val block = intent.getStringExtra(RaptorApplication.EXTRA_BLOCK) ?: return
        val id = intent.getIntExtra(EXTRA_ID, 0)
        val pending = goAsync()
        scope.launch {
            val r = runCatching { RaptorApplication.instance.core.quickReply(server, session, block, text + "\r") }
            r.exceptionOrNull()?.let { android.util.Log.w("RaptorReply", "reply failed", it) }
            val ok = r.isSuccess
            // Replace the notification with a quiet confirmation (Android expects the
            // reply UI to be updated, or it keeps spinning).
            val done = NotificationCompat.Builder(context, RaptorApplication.CHANNEL_AGENTS)
                .setSmallIcon(R.drawable.ic_notification)
                .setContentTitle(if (ok) "Sent" else "Couldn't send reply")
                .setContentText(text)
                .setColor(0xFF34D399.toInt())
                .setSilent(true)
                .setTimeoutAfter(if (ok) 4000 else 0)
                .setGroup("raptor")
                .build()
            runCatching { NotificationManagerCompat.from(context).notify(id, done) }
            pending.finish()
        }
    }

    companion object {
        const val KEY_REPLY = "reply"
        const val EXTRA_ID = "id"
        private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
    }
}
