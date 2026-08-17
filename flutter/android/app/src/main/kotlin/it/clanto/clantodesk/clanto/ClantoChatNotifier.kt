// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

package it.clanto.clantodesk.clanto

import android.annotation.SuppressLint
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.os.Build
import androidx.core.app.NotificationCompat
import androidx.core.content.ContextCompat
import io.flutter.plugin.common.MethodChannel
import it.clanto.clantodesk.MainActivity
import it.clanto.clantodesk.R
import it.clanto.clantodesk.translate

object ClantoChatNotifier {
    private const val CHANNEL_ID = "clantodesk_controller_chat"
    private const val NOTIFICATION_ID = 2

    @SuppressLint("UnspecifiedImmutableFlag")
    fun publish(context: Context, text: String?, result: MethodChannel.Result) {
        if (text == null) {
            result.error("invalid_arguments", "Testo del messaggio mancante", null)
            return
        }
        if (!MainActivity.isInForeground) {
            val appContext = context.applicationContext
            val notificationManager = appContext.getSystemService(Context.NOTIFICATION_SERVICE)
                as NotificationManager
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O &&
                notificationManager.getNotificationChannel(CHANNEL_ID) == null
            ) {
                notificationManager.createNotificationChannel(
                    NotificationChannel(
                        CHANNEL_ID,
                        "ClantoDesk Chat",
                        NotificationManager.IMPORTANCE_HIGH
                    )
                )
            }
            val intent = Intent(appContext, MainActivity::class.java).apply {
                flags = Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_RESET_TASK_IF_NEEDED
                action = Intent.ACTION_MAIN
                addCategory(Intent.CATEGORY_LAUNCHER)
            }
            val pendingIntentFlags = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.M) {
                PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
            } else {
                PendingIntent.FLAG_UPDATE_CURRENT
            }
            val pendingIntent = PendingIntent.getActivity(
                appContext,
                NOTIFICATION_ID,
                intent,
                pendingIntentFlags
            )
            val notification = NotificationCompat.Builder(appContext, CHANNEL_ID)
                .setSmallIcon(R.mipmap.ic_stat_logo)
                .setAutoCancel(true)
                .setPriority(NotificationCompat.PRIORITY_HIGH)
                .setCategory(NotificationCompat.CATEGORY_MESSAGE)
                .setDefaults(Notification.DEFAULT_ALL)
                .setContentTitle(translate("New message"))
                .setContentText(text)
                .setStyle(NotificationCompat.BigTextStyle().bigText(text))
                .setContentIntent(pendingIntent)
                .setColor(ContextCompat.getColor(appContext, R.color.primary))
                .build()
            notificationManager.notify(NOTIFICATION_ID, notification)
        }
        result.success(null)
    }
}
