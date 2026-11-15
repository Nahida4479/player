package dev.verncat.player

import android.content.Intent
import android.view.View
import android.view.ViewGroup
import android.webkit.WebView
import androidx.test.platform.app.InstrumentationRegistry
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

class FontScaleTest {
    @Test fun systemFontScaleUpdatesExistingWebViewWithoutCompounding() {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        fun shell(command: String): String = instrumentation.uiAutomation.executeShellCommand(command)
            .use { android.os.ParcelFileDescriptor.AutoCloseInputStream(it).bufferedReader().readText().trim() }
        fun find(view: View): WebView? {
            if (view is WebView) return view
            if (view is ViewGroup) for (i in 0 until view.childCount) find(view.getChildAt(i))?.let { return it }
            return null
        }
        val originalScale = shell("settings get system font_scale")
        var activity: MainActivity? = null
        var webView: WebView? = null
        fun evaluate(script: String): JSONObject {
            val result = CountDownLatch(1)
            var value = ""
            instrumentation.runOnMainSync {
                requireNotNull(webView).evaluateJavascript(script) { value = it; result.countDown() }
            }
            assertTrue("JavaScript result", result.await(10, TimeUnit.SECONDS))
            return JSONObject(value)
        }
        try {
            shell("settings put system font_scale 1.0")
            shell("appops set ${instrumentation.targetContext.packageName} MANAGE_EXTERNAL_STORAGE allow")
            activity = instrumentation.startActivitySync(Intent(instrumentation.targetContext, MainActivity::class.java)
                .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)) as MainActivity
            val creationDeadline = System.nanoTime() + TimeUnit.SECONDS.toNanos(20)
            do {
                instrumentation.runOnMainSync { webView = find(requireNotNull(activity).window.decorView) }
                if (webView != null) break
                Thread.sleep(100)
            } while (System.nanoTime() < creationDeadline)
            assertNotNull("Tauri creates its WebView", webView)
            instrumentation.runOnMainSync {
                requireNotNull(webView).loadDataWithBaseURL("https://localhost/", """
                    <meta name="viewport" content="width=device-width,initial-scale=1">
                    <style>body{margin:0}button{font-size:16px;line-height:1.5;padding:10px;max-width:100%;white-space:normal}span{font-size:16px}</style>
                    <button><span id="label">Downloading 100%</span></button>
                """.trimIndent(), "text/html", "UTF-8", null)
            }
            var baseline = 0.0
            for (scale in listOf(1.0, 1.3, 1.5, 1.3, 1.0)) {
                shell("settings put system font_scale $scale")
                val deadline = System.nanoTime() + TimeUnit.SECONDS.toNanos(15)
                var zoom = 0
                do {
                    instrumentation.runOnMainSync { zoom = requireNotNull(webView).settings.textZoom }
                    if (zoom == (scale * 100).toInt()) break
                    Thread.sleep(100)
                } while (System.nanoTime() < deadline)
                assertEquals("Live text zoom at $scale", (scale * 100).toInt(), zoom)
                var metrics: JSONObject
                do {
                    metrics = evaluate("""(()=>{const e=document.getElementById('label');if(!e)return {};const r=document.createRange();r.selectNodeContents(e);return {font:parseFloat(getComputedStyle(e).fontSize),width:r.getBoundingClientRect().width,height:r.getBoundingClientRect().height,viewport:innerWidth,overflow:document.documentElement.scrollWidth>innerWidth}})()""")
                    if (metrics.has("font") && kotlin.math.abs(metrics.getDouble("font") - 16 * scale) < 0.1) break
                    Thread.sleep(100)
                } while (System.nanoTime() < deadline)
                assertFalse("No horizontal overflow: $metrics", metrics.getBoolean("overflow"))
                assertEquals("Computed font follows system scale: $metrics", 16 * scale, metrics.getDouble("font"), 0.1)
                if (baseline == 0.0) baseline = metrics.getDouble("width")
                assertEquals("Rendered text width at $scale: $metrics", baseline * scale, metrics.getDouble("width"), 2.0)
                android.util.Log.i("FontScaleTest", "scale=$scale zoom=$zoom metrics=$metrics")
                assertSame("Font changes retain the activity", activity, MainActivity.instance?.get())
            }
        } finally {
            shell(if (originalScale == "null") "settings delete system font_scale" else "settings put system font_scale $originalScale")
            instrumentation.runOnMainSync { activity?.finish() }
        }
    }
}
