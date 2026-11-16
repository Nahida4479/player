package dev.verncat.player

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.Bundle
import android.os.IBinder
import android.os.PowerManager
import android.os.ResultReceiver

/** All session and wake-lock operations run on Android's main thread. */
class IndexingService : Service() {
    companion object {
        internal const val SESSION = "sessionId"
        internal const val RECEIVER = "receiver"
        private const val CHANNEL = "library_indexing"
        private const val NOTIFICATION = 1002
        internal var instance: IndexingService? = null
            private set
    }

    private data class Progress(val current: Int = 0, val total: Int = 0)
    private val sessions = linkedMapOf<String, Progress>()
    private var wakeLock: PowerManager.WakeLock? = null

    override fun onCreate() {
        super.onCreate()
        instance = this
        getSystemService(NotificationManager::class.java).createNotificationChannel(
            NotificationChannel(CHANNEL, "Library indexing", NotificationManager.IMPORTANCE_LOW)
        )
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        @Suppress("DEPRECATION")
        val receiver = intent?.getParcelableExtra<ResultReceiver>(RECEIVER)
        val session = intent?.getStringExtra(SESSION)
        if (session.isNullOrBlank()) {
            receiver?.send(1, Bundle().apply { putString("error", "Missing indexing session") })
            if (sessions.isEmpty()) stopSelf(startId)
            return START_NOT_STICKY
        }
        try {
            sessions.putIfAbsent(session, Progress())
            val notification = notification()
            if (Build.VERSION.SDK_INT >= 29) {
                startForeground(NOTIFICATION, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC)
            } else {
                startForeground(NOTIFICATION, notification)
            }
            if (wakeLock == null) {
                wakeLock = getSystemService(PowerManager::class.java)
                    .newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "$packageName:library-indexing")
                    .apply { setReferenceCounted(false); acquire() }
            }
            receiver?.send(0, Bundle())
        } catch (error: Exception) {
            sessions.remove(session)
            if (sessions.isEmpty()) finish()
            receiver?.send(1, Bundle().apply { putString("error", error.message ?: "Unable to start indexing service") })
        }
        return START_NOT_STICKY
    }

    internal fun update(session: String, current: Int, total: Int) {
        check(sessions.containsKey(session)) { "Indexing foreground session is no longer active" }
        sessions[session] = Progress(current.coerceAtLeast(0), total.coerceAtLeast(0))
        getSystemService(NotificationManager::class.java).notify(NOTIFICATION, notification())
    }

    internal fun end(session: String) {
        sessions.remove(session)
        if (sessions.isEmpty()) finish()
        else getSystemService(NotificationManager::class.java).notify(NOTIFICATION, notification())
    }

    private fun notification(): Notification {
        val progress = sessions.values.lastOrNull() ?: Progress()
        val open = PendingIntent.getActivity(this, 0,
            Intent(this, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP),
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE)
        return Notification.Builder(this, CHANNEL)
            .setSmallIcon(android.R.drawable.stat_notify_sync)
            .setContentTitle("Indexing music library")
            .setContentText(if (progress.total <= 0) "Discovering audio files…"
                else "${progress.current} / ${progress.total} files")
            .setSubText(if (sessions.size > 1) "${sessions.size} active scans" else null)
            .setContentIntent(open)
            .setOnlyAlertOnce(true)
            .setOngoing(true)
            .setProgress(progress.total, progress.current.coerceAtMost(progress.total), progress.total <= 0)
            .apply {
                if (Build.VERSION.SDK_INT >= 31) {
                    setForegroundServiceBehavior(Notification.FOREGROUND_SERVICE_IMMEDIATE)
                }
            }
            .build()
    }

    private fun release() {
        sessions.clear()
        wakeLock?.let { if (it.isHeld) it.release() }
        wakeLock = null
        stopForeground(STOP_FOREGROUND_REMOVE)
    }

    private fun finish() {
        release()
        stopSelf()
    }

    // Android 15's dataSync budget is shared by the app. Do not restart or retain a
    // pretend foreground notification after the OS revokes this execution window.
    override fun onTimeout(startId: Int, fgsType: Int) {
        finish()
    }

    override fun onDestroy() {
        release()
        if (instance === this) instance = null
        super.onDestroy()
    }

    override fun onBind(intent: Intent?): IBinder? = null
}
