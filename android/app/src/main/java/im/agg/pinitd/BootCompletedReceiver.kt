package im.agg.pinitd

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.util.Log

class BootCompletedReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context?, intent: Intent?) {
        if (context != null && intent?.action == Intent.ACTION_BOOT_COMPLETED) {
            Log.w("pinitd-trampoline", "Boot completed. Starting")

//            val command = Settings(context).command
//            if (command != null) {
//                Log.w("pinid-trampoline", "Executing command: $command")
//            }
            Log.w("pinitd-trampoline", "Launching")
            launch(context)
        }
    }
}