package com.penumbraos.pinitd

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.util.Log

class BootCompletedReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context?, intent: Intent?) {
        if (context != null && intent?.action == Intent.ACTION_BOOT_COMPLETED) {
            Log.w(SHARED_TAG, "Boot completed. Starting")

//            val command = Settings(context).command
//            if (command != null) {
//                Log.w(SHARED_TAG, "Executing command: $command")
//            }
            Log.w(SHARED_TAG, "Launching (Disabled)")
//            launch(context)
        }
    }
}