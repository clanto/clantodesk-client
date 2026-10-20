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
import android.text.TextUtils
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
            it.layoutParams.height = watermarkHeight(context)
            windowManager?.updateViewLayout(it, it.layoutParams)
            return
        }

        val manager = context.getSystemService(Context.WINDOW_SERVICE) as WindowManager
        val view = TextView(context).apply {
            this.text = text
            setTextColor(Color.WHITE)
            setTextSize(android.util.TypedValue.COMPLEX_UNIT_SP, 14f)
            setTypeface(Typeface.DEFAULT, Typeface.BOLD)
            gravity = Gravity.CENTER
            setPadding(dp(context, 16), dp(context, 4), dp(context, 16), dp(context, 4))
            maxLines = 1
            ellipsize = TextUtils.TruncateAt.END
            background = GradientDrawable().apply {
                setColor(Color.rgb(116, 0, 0))
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
            watermarkHeight(context),
            windowType,
            WindowManager.LayoutParams.FLAG_NOT_TOUCHABLE or
                WindowManager.LayoutParams.FLAG_NOT_FOCUSABLE,
            PixelFormat.TRANSLUCENT,
        ).apply {
            gravity = Gravity.TOP or Gravity.CENTER_HORIZONTAL
            alpha = 0.55f
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

    private fun watermarkHeight(context: Context): Int = dp(context, 24 + 8)
}
