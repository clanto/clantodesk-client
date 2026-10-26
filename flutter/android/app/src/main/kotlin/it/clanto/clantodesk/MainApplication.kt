package it.clanto.clantodesk

import android.app.Application
import android.util.Log
import ffi.FFI
import it.clanto.clantodesk.clanto.ClantoManagedConfig

class MainApplication : Application() {
    companion object {
        private const val TAG = "MainApplication"
    }

    override fun onCreate() {
        super.onCreate()
        Log.d(TAG, "App start")
        FFI.onAppStart(applicationContext)
        ClantoManagedConfig.register(applicationContext)
    }
}
