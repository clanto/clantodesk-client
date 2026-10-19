// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

package it.clanto.clantodesk.clanto

import android.content.Context
import android.graphics.Color
import android.graphics.PixelFormat
import android.graphics.Typeface
import android.graphics.drawable.GradientDrawable
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.provider.Settings
import android.util.Log
import android.view.Gravity
import android.view.View
import android.view.ViewGroup
import android.view.WindowManager
import android.widget.TextView

object ClantoHostWatermark {
    private const val TAG = "ClantoHostWatermark"
    private val mainHandler = Handler(Looper.getMainLooper())

    private var windowManager: WindowManager? = null
    private var watermarkView: TextView? = null
    private var permissionWarningLogged = false
    private var addFailureLogged = false
    private var hideFailureLogged = false

    @JvmStatic
    fun update(context: Context, text: String?) {
        val appContext = context.applicationContext
        mainHandler.post {
            if (text.isNullOrEmpty()) {
                hide()
            } else {
                showOrUpdate(appContext, text)
            }
        }
    }

    private fun showOrUpdate(context: Context, text: String) {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.M && !Settings.canDrawOverlays(context)) {
            if (!permissionWarningLogged) {
                Log.w(TAG, "Cannot show host watermark: overlay permission is not granted")
                permissionWarningLogged = true
            }
            hide()
            return
        }
        permissionWarningLogged = false

        watermarkView?.let {
            it.text = text
            it.maxLines = lineCount(text)
            it.layoutParams.height = watermarkHeight(context, text)
            windowManager?.updateViewLayout(it, it.layoutParams)
            return
        }

        val manager = context.getSystemService(Context.WINDOW_SERVICE) as WindowManager
        val view = TextView(context).apply {
            this.text = text
            setTextColor(Color.argb(235, 255, 255, 255))
            setTextSize(android.util.TypedValue.COMPLEX_UNIT_SP, 14f)
            setTypeface(Typeface.DEFAULT, Typeface.BOLD)
            gravity = Gravity.CENTER
            setPadding(dp(context, 16), dp(context, 10), dp(context, 16), dp(context, 10))
            maxLines = lineCount(text)
            background = GradientDrawable().apply {
                setColor(Color.argb(170, 116, 0, 0))
            }
            isClickable = false
            isFocusable = false
            isFocusableInTouchMode = false
            importantForAccessibility = View.IMPORTANT_FOR_ACCESSIBILITY_NO_HIDE_DESCENDANTS
        }
        val windowType = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            WindowManager.LayoutParams.TYPE_APPLICATION_OVERLAY
        } else {
            @Suppress("DEPRECATION")
            WindowManager.LayoutParams.TYPE_PHONE
        }
        val params = WindowManager.LayoutParams(
            ViewGroup.LayoutParams.MATCH_PARENT,
            watermarkHeight(context, text),
            windowType,
            WindowManager.LayoutParams.FLAG_NOT_TOUCHABLE or
                WindowManager.LayoutParams.FLAG_NOT_FOCUSABLE,
            PixelFormat.TRANSLUCENT,
        ).apply {
            gravity = Gravity.TOP or Gravity.CENTER_HORIZONTAL
            alpha = 0.75f
        }

        try {
            manager.addView(view, params)
            windowManager = manager
            watermarkView = view
            addFailureLogged = false
            hideFailureLogged = false
        } catch (error: RuntimeException) {
            if (!addFailureLogged) {
                Log.e(TAG, "Failed to show host watermark", error)
                addFailureLogged = true
            }
        }
    }

    private fun hide() {
        val view = watermarkView ?: return
        try {
            windowManager?.removeViewImmediate(view)
            hideFailureLogged = false
        } catch (error: RuntimeException) {
            if (!hideFailureLogged) {
                Log.e(TAG, "Failed to hide host watermark", error)
                hideFailureLogged = true
            }
        } finally {
            watermarkView = null
            windowManager = null
        }
    }

    private fun dp(context: Context, value: Int): Int =
        (value * context.resources.displayMetrics.density).toInt()

    private fun lineCount(text: String): Int =
        text.lineSequence().count().coerceIn(1, 4)

    private fun watermarkHeight(context: Context, text: String): Int =
        dp(context, lineCount(text) * 24 + 16)
}
