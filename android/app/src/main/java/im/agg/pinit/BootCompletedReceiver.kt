package im.agg.pinit

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.util.Log

class BootCompletedReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context?, intent: Intent?) {
        if (context != null && intent?.action == Intent.ACTION_BOOT_COMPLETED) {
            Log.w("Trampoline", "Boot completed. Starting")

            val command = Settings(context).command
            if (command != null) {
                Log.w("Trampoline", "Executing command: $command")
            }
        }
    }
}