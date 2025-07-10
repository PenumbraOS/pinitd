package com.penumbraos.pinitd

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.util.Log
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch

class BootCompletedReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context?, intent: Intent?) {
        if (context != null && intent?.action == Intent.ACTION_BOOT_COMPLETED) {
            Log.w(SHARED_TAG, "Boot completed. Checking boot protection")

            // Attempt to make absolutely sure the exploit is cleared. This will ideally prevent
            // boot looping since this runs so early
            context.contentResolver.delete(EXEMPTIONS_SETTING_URI, null, null)

            val protection = BootLoopProtection(context)
            
            if (protection.shouldAttemptLaunch()) {
                Log.w(SHARED_TAG, "Boot protection allows launch, proceeding")
                protection.recordAttempt()

                val scope = CoroutineScope(Dispatchers.IO)
                scope.launch {
                    launchPinitd(scope, context, protection)
                }
            } else {
                Log.w(SHARED_TAG, "Boot protection blocked launch")
                Log.i(SHARED_TAG, protection.getStatus())
            }
        }
    }
}