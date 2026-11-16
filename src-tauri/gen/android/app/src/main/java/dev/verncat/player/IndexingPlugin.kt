package dev.verncat.player

import android.app.Activity
import android.content.Intent
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.os.ResultReceiver
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.Plugin

@InvokeArg
internal class IndexingSessionArgs {
    lateinit var sessionId: String
}

@InvokeArg
internal class IndexingProgressArgs {
    lateinit var sessionId: String
    var current: Int = 0
    var total: Int = 0
}

/** Native Rust owns scan lifetime; this bridge never depends on the WebView. */
@TauriPlugin
class IndexingPlugin(private val activity: Activity) : Plugin(activity) {
    private val main = Handler(Looper.getMainLooper())

    @Command
    fun beginIndexing(invoke: Invoke) {
        try {
            val session = invoke.parseArgs(IndexingSessionArgs::class.java).sessionId
            require(session.isNotBlank()) { "Missing indexing session" }
            main.post {
                val preferences = activity.getSharedPreferences("indexing-notifications", Activity.MODE_PRIVATE)
                if (android.os.Build.VERSION.SDK_INT >= 33 && activity.hasWindowFocus() &&
                    !preferences.getBoolean("asked", false) &&
                    activity.checkSelfPermission(android.Manifest.permission.POST_NOTIFICATIONS) !=
                    android.content.pm.PackageManager.PERMISSION_GRANTED) {
                    preferences.edit().putBoolean("asked", true).apply()
                    activity.requestPermissions(arrayOf(android.Manifest.permission.POST_NOTIFICATIONS), 4102)
                }
                var settled = false
                val timeout = Runnable {
                    if (!settled) {
                        settled = true
                        IndexingService.instance?.end(session)
                        invoke.reject("Timed out starting indexing foreground service")
                    }
                }
                val receiver = object : ResultReceiver(main) {
                    override fun onReceiveResult(resultCode: Int, resultData: Bundle?) {
                        if (settled) {
                            IndexingService.instance?.end(session)
                            return
                        }
                        settled = true
                        main.removeCallbacks(timeout)
                        if (resultCode == 0) invoke.resolve()
                        else invoke.reject(resultData?.getString("error") ?: "Indexing service failed")
                    }
                }
                try {
                    val intent = Intent(activity, IndexingService::class.java)
                        .putExtra(IndexingService.SESSION, session)
                        .putExtra(IndexingService.RECEIVER, receiver)
                    // minSdk is 26. Android's background-start/permission exceptions
                    // reach Rust; do not claim protection before startForeground ran.
                    activity.startForegroundService(intent)
                    main.postDelayed(timeout, 10_000)
                } catch (error: Exception) {
                    settled = true
                    invoke.reject(error.message ?: "Unable to start indexing foreground service")
                }
            }
        } catch (error: Exception) {
            invoke.reject(error.message ?: "Invalid indexing session")
        }
    }

    @Command
    fun updateIndexing(invoke: Invoke) {
        try {
            val args = invoke.parseArgs(IndexingProgressArgs::class.java)
            main.post {
                try {
                    val service = checkNotNull(IndexingService.instance) { "Indexing foreground service stopped" }
                    service.update(args.sessionId, args.current, args.total)
                    invoke.resolve()
                } catch (error: Exception) {
                    invoke.reject(error.message ?: "Unable to update indexing progress")
                }
            }
        } catch (error: Exception) {
            invoke.reject(error.message ?: "Invalid indexing progress")
        }
    }

    @Command
    fun endIndexing(invoke: Invoke) {
        try {
            val session = invoke.parseArgs(IndexingSessionArgs::class.java).sessionId
            main.post {
                try {
                    IndexingService.instance?.end(session)
                    invoke.resolve()
                } catch (error: Exception) {
                    invoke.reject(error.message ?: "Unable to stop indexing foreground service")
                }
            }
        } catch (error: Exception) {
            invoke.reject(error.message ?: "Invalid indexing session")
        }
    }
}
