package com.penumbraos.pinitd

import android.annotation.SuppressLint
import android.app.Activity
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.util.Log
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch

const val SHARED_TAG = "pinitd-trampoline"

// adb shell am start -n com.penumbraos.pinitd/.ManualLaunchActivity
class ManualLaunchActivity : Activity() {
    val scope = CoroutineScope(Dispatchers.Main)
    val handler = Handler(Looper.getMainLooper())

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        Log.w(SHARED_TAG, "ManualLaunchActivity created. Waiting...")
        handler.postDelayed(Runnable {
            scope.launch {
                launchPinitd(scope, this@ManualLaunchActivity, BootLoopProtection(this@ManualLaunchActivity))
            }
            finishAndRemoveTask()
        }, 1000)
    }
}