package com.penumbraos.pinitd

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.util.Log
import com.penumbraos.pinitd.util.EXEMPTIONS_SETTING_URI
import com.penumbraos.pinitd.util.launchWithBootProtection

class BootCompletedReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context?, intent: Intent?) {
        if (context != null) {
            if (intent?.action == Intent.ACTION_BOOT_COMPLETED) {
                launchWithBootProtection(context)
            } else if (intent?.action == Intent.ACTION_SHUTDOWN) {
                Log.w(SHARED_TAG, "Clearing exemptions for shutdown")
                context.contentResolver.delete(EXEMPTIONS_SETTING_URI, null, null)
            }
        }
    }
}