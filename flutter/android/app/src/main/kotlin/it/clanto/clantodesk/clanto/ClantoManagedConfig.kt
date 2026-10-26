// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

package it.clanto.clantodesk.clanto

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.content.RestrictionsManager
import android.os.Build
import android.util.Log
import org.json.JSONObject

/**
 * Configurazione gestita da MDM (managed configurations, res/xml/app_restrictions.xml):
 * all'avvio e a ogni modifica le restrizioni passano a Rust
 * (src/clanto/managed_config.rs), con lo stesso effetto della GPO di Windows.
 */
object ClantoManagedConfig {
    private const val TAG = "ClantoManagedConfig"

    @JvmStatic
    external fun apply(json: String)

    fun register(context: Context) {
        val app = context.applicationContext
        push(app)
        val receiver = object : BroadcastReceiver() {
            override fun onReceive(c: Context, intent: Intent) = push(app)
        }
        val filter = IntentFilter(Intent.ACTION_APPLICATION_RESTRICTIONS_CHANGED)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            app.registerReceiver(receiver, filter, Context.RECEIVER_NOT_EXPORTED)
        } else {
            app.registerReceiver(receiver, filter)
        }
    }

    private fun push(context: Context) {
        try {
            val manager = context.getSystemService(Context.RESTRICTIONS_SERVICE) as RestrictionsManager
            val restrictions = manager.applicationRestrictions
            val json = JSONObject()
            for (key in restrictions.keySet()) {
                when (val value = restrictions.get(key)) {
                    is Boolean, is Int, is String -> json.put(key, value)
                }
            }
            apply(json.toString())
        } catch (e: Exception) {
            Log.w(TAG, "managed configuration not applied: $e")
        }
    }
}
