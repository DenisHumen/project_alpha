// SPDX-License-Identifier: MPL-2.0
package app.kinolune.player

import android.content.Context
import android.graphics.Typeface
import android.provider.Settings
import android.view.accessibility.CaptioningManager
import io.flutter.embedding.engine.plugins.FlutterPlugin
import io.flutter.plugin.common.MethodCall
import io.flutter.plugin.common.MethodChannel

/**
 * Android side of `pa_player` (PLAY-007): channel `app.kinolune/captioning`, method `captionStyle` — the
 * system caption settings ([CaptioningManager]) that the player's subtitle overlay follows in "system style"
 * mode. Colors are ARGB ints; a key is absent when the user's style leaves it unset.
 */
class PaPlayerPlugin : FlutterPlugin, MethodChannel.MethodCallHandler {
    private var channel: MethodChannel? = null
    private var context: Context? = null

    override fun onAttachedToEngine(binding: FlutterPlugin.FlutterPluginBinding) {
        context = binding.applicationContext
        channel = MethodChannel(binding.binaryMessenger, CHANNEL).also { it.setMethodCallHandler(this) }
    }

    override fun onDetachedFromEngine(binding: FlutterPlugin.FlutterPluginBinding) {
        channel?.setMethodCallHandler(null)
        channel = null
        context = null
    }

    override fun onMethodCall(call: MethodCall, result: MethodChannel.Result) {
        when (call.method) {
            "captionStyle" -> {
                val ctx = context
                if (ctx == null) {
                    result.success(null)
                    return
                }
                try {
                    result.success(captionStyle(ctx))
                } catch (e: RuntimeException) {
                    result.error("captioning", e.message, null)
                }
            }
            else -> result.notImplemented()
        }
    }

    companion object {
        const val CHANNEL = "app.kinolune/captioning"

        /** Android captioning typefaces (Settings > Accessibility > Caption preferences > Text font). */
        private val TYPEFACES = listOf(
            "sans-serif-condensed",
            "sans-serif-monospace",
            "serif-monospace",
            "sans-serif-smallcaps",
            "serif",
            "monospace",
            "casual",
            "cursive",
            "sans-serif",
        )

        fun captionStyle(ctx: Context): Map<String, Any?> {
            val cm = ctx.getSystemService(Context.CAPTIONING_SERVICE) as CaptioningManager
            val style = cm.userStyle
            val m = HashMap<String, Any?>()
            m["enabled"] = cm.isEnabled
            m["fontScale"] = cm.fontScale.toDouble()
            if (style.hasForegroundColor()) m["foregroundColor"] = style.foregroundColor
            if (style.hasBackgroundColor()) m["backgroundColor"] = style.backgroundColor
            if (style.hasWindowColor()) m["windowColor"] = style.windowColor
            if (style.hasEdgeType()) m["edgeType"] = style.edgeType
            if (style.hasEdgeColor()) m["edgeColor"] = style.edgeColor
            m["typeface"] = typefaceName(ctx, style.typeface)
            m["locale"] = cm.locale?.toLanguageTag()
            return m
        }

        /**
         * Name of the caption typeface. `CaptionStyle` only exposes a [Typeface], made with
         * `Typeface.create(name, NORMAL)`, which returns the cached system instance: compare against those.
         * The setting itself is a fallback (hidden settings may not be readable).
         */
        private fun typefaceName(ctx: Context, typeface: Typeface?): String? {
            if (typeface != null) {
                for (name in TYPEFACES) {
                    val candidate = Typeface.create(name, Typeface.NORMAL)
                    // A family missing on the device comes back as the default one.
                    if (candidate == Typeface.DEFAULT && name != "sans-serif") continue
                    if (candidate == typeface) return name
                }
            }
            return try {
                Settings.Secure.getString(ctx.contentResolver, "accessibility_captioning_typeface")
            } catch (e: SecurityException) {
                null
            }
        }
    }
}
