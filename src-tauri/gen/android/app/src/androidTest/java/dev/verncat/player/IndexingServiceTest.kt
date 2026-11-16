package dev.verncat.player

import android.app.NotificationManager
import android.content.Intent
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.os.ResultReceiver
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assert.*
import org.junit.Test
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

/** Uses the real foreground service, notification manager and partial wake lock. */
class IndexingServiceTest {
    private fun webView(view: android.view.View): android.webkit.WebView? {
        if (view is android.webkit.WebView) return view
        if (view is android.view.ViewGroup) for (index in 0 until view.childCount) {
            webView(view.getChildAt(index))?.let { return it }
        }
        return null
    }

    @Test fun foregroundIndexingLifecycleAndNativeBackgroundScan() {
        context.startActivity(Intent(context, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
        instrumentation.waitForIdleSync()
        nativeReindexBridgeRunsInBackgroundAndFinishes()
        overlappingSessionsSurviveBackgroundAndReleaseOnlyAfterLastEnd()
        simulatedDataSyncTimeoutRemovesProtectionAndRejectsFurtherProgress()
    }

    private fun nativeReindexBridgeRunsInBackgroundAndFinishes() {
        val fixture = java.io.File("/sdcard/Player/indexing-instrumentation-${System.nanoTime()}")
        assertTrue(fixture.mkdirs())
        val fileCount = 48
        try {
            // Valid PCM WAVs exercise native hashing/metadata/waveform work, not mocks.
            for (index in 0 until fileCount) {
                java.io.DataOutputStream(java.io.File(fixture, "fixture-$index.wav").outputStream().buffered()).use { out ->
                    fun le(value: Int) = out.writeInt(Integer.reverseBytes(value))
                    fun short(value: Int) = out.writeShort(java.lang.Short.reverseBytes(value.toShort()).toInt())
                    val bytes = 44100 * 2 * 90
                    out.writeBytes("RIFF"); le(bytes + 36); out.writeBytes("WAVEfmt ")
                    le(16); short(1); short(1); le(44100); le(88200); short(2); short(16)
                    out.writeBytes("data"); le(bytes)
                    val buffer = ByteArray(8192)
                    buffer[0] = index.toByte()
                    buffer[1] = (index ushr 8).toByte()
                    buffer[2] = (index ushr 16).toByte()
                    var remaining = bytes
                    while (remaining > 0) {
                        val count = minOf(remaining, buffer.size)
                        out.write(buffer, 0, count); remaining -= count
                    }
                }
            }
            var invoked = false
            repeat(100) {
                if (!invoked) {
                    main {
                        MainActivity.instance?.get()?.let { activity ->
                            webView(activity.window.decorView)?.evaluateJavascript(
                                "if(window.__TAURI_INTERNALS__){window.__TAURI_INTERNALS__.invoke('reindex');true}else{false}") {
                                invoked = it == "true"
                            }
                        }
                    }
                    Thread.sleep(100)
                }
            }
            assertTrue("Current app native reindex command unavailable", invoked)
            var observed = false
            repeat(200) {
                if (!observed) {
                    observed = notificationPresent()
                    if (!observed) Thread.sleep(50)
                }
            }
            assertTrue("Rust scan never started indexing foreground notification", observed)
            assertTrue(wakeHeld())
            instrumentation.uiAutomation.executeShellCommand("input keyevent KEYCODE_HOME").close()
            instrumentation.uiAutomation.executeShellCommand("input keyevent KEYCODE_SLEEP").close()
            val backgroundStart = context.getSystemService(NotificationManager::class.java)
                .activeNotifications.first { it.notification.channelId == "library_indexing" }
                .notification.extras.getCharSequence("android.text").toString()
            var backgroundProgress = false
            var lastProgress = backgroundStart
            var finished = false
            repeat(36000) {
                if (!finished) {
                    finished = !notificationPresent()
                    val text = context.getSystemService(NotificationManager::class.java).activeNotifications
                        .firstOrNull { it.notification.channelId == "library_indexing" }
                        ?.notification?.extras?.getCharSequence("android.text")?.toString()
                    if (text != null && text != lastProgress) {
                        backgroundProgress = true
                        lastProgress = text
                    }
                    if (!finished) Thread.sleep(50)
                }
            }
            assertTrue("Native background scan did not finish", finished)
            assertTrue("No native file progress observed with screen off", backgroundProgress)
            assertFalse(wakeHeld())
        } finally {
            instrumentation.uiAutomation.executeShellCommand("input keyevent KEYCODE_WAKEUP").close()
            fixture.deleteRecursively()
        }
    }
    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private val context = instrumentation.targetContext

    private fun main(action: () -> Unit) = instrumentation.runOnMainSync(action)

    private fun begin(session: String) {
        val acknowledged = CountDownLatch(1)
        var status = -1
        main {
            context.startForegroundService(Intent(context, IndexingService::class.java)
                .putExtra(IndexingService.SESSION, session)
                .putExtra(IndexingService.RECEIVER, object : ResultReceiver(Handler(Looper.getMainLooper())) {
                    override fun onReceiveResult(resultCode: Int, resultData: Bundle?) {
                        status = resultCode
                        acknowledged.countDown()
                    }
                }))
        }
        assertTrue("Foreground start was not acknowledged", acknowledged.await(10, TimeUnit.SECONDS))
        assertEquals(0, status)
    }

    private fun wakeHeld(): Boolean {
        val result = instrumentation.uiAutomation.executeShellCommand("dumpsys power")
        val dump = android.os.ParcelFileDescriptor.AutoCloseInputStream(result).bufferedReader().use { it.readText() }
        return dump.lineSequence().any { it.contains("PARTIAL_WAKE_LOCK") && it.contains("dev.verncat.player:library-indexing") }
    }

    private fun notificationPresent(): Boolean {
        repeat(20) {
            if (context.getSystemService(NotificationManager::class.java).activeNotifications
                .any { it.notification.channelId == "library_indexing" }) return true
            Thread.sleep(25)
        }
        return false
    }

    private fun overlappingSessionsSurviveBackgroundAndReleaseOnlyAfterLastEnd() {
        instrumentation.uiAutomation.executeShellCommand("input keyevent KEYCODE_WAKEUP").close()
        instrumentation.uiAutomation.executeShellCommand("am start -n dev.verncat.player/.MainActivity").close()
        try {
            begin("instrumentation-first")
            begin("instrumentation-second")
            assertTrue(notificationPresent())
            assertTrue(wakeHeld())
            instrumentation.uiAutomation.executeShellCommand("input keyevent KEYCODE_HOME").close()
            main { IndexingService.instance!!.update("instrumentation-second", 2, 5) }
            assertTrue(notificationPresent())
            assertTrue(wakeHeld())
            main { IndexingService.instance!!.end("instrumentation-first") }
            assertTrue(notificationPresent())
            assertTrue(wakeHeld())
            main { IndexingService.instance!!.end("instrumentation-second") }
            assertFalse(notificationPresent())
            assertFalse(wakeHeld())
        } finally {
            main {
                IndexingService.instance?.end("instrumentation-first")
                IndexingService.instance?.end("instrumentation-second")
            }
        }
    }

    private fun simulatedDataSyncTimeoutRemovesProtectionAndRejectsFurtherProgress() {
        instrumentation.uiAutomation.executeShellCommand("am start -n dev.verncat.player/.MainActivity").close()
        begin("instrumentation-timeout")
        try {
            assertTrue(wakeHeld())
            main {
                val service = IndexingService.instance!!
                // Direct callback simulation; this does not claim OS budget expiry.
                service.onTimeout(1, android.content.pm.ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC)
                assertThrows(IllegalStateException::class.java) {
                    service.update("instrumentation-timeout", 1, 2)
                }
            }
            assertFalse(notificationPresent())
            assertFalse(wakeHeld())
        } finally {
            main { IndexingService.instance?.end("instrumentation-timeout") }
        }
    }
}
