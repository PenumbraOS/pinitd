package com.penumbraos.pinitd

import android.util.Log
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.launch
import kotlinx.coroutines.withTimeout
import kotlin.time.Duration

class Logcat {
    val scope: CoroutineScope
    val process: Process
    var eatInBackground: Boolean = false

    constructor(scope: CoroutineScope) {
        this.scope = scope
        // This is probably necessary, but it also breaks the vulnerability start on device (not in emu)
//        ProcessBuilder(listOf("logcat", "-c")).start().waitFor()
        ProcessBuilder(listOf("logcat", "-v", "brief", "*:S", "ActivityManager:V")).start().also { process ->
            this.process = process
        }
    }

    suspend fun waitForSubstring(substring: String, timeout: Duration): Boolean {
        eatInBackground = false
        try {
            withTimeout(timeout) {
                val reader = process.inputStream.bufferedReader()
                Log.w(SHARED_TAG, "Started waiting for substring: \"$substring\"")
                while (true) {
                    val line = reader.readLine() ?: break
                    if (line.contains(substring)) {
                        Log.w(SHARED_TAG, "Matching string")
                        return@withTimeout
                    }
                }
            }
            return true
        } catch (_: Exception) {
            return false
        }
    }

    fun eatInBackground() {
        eatInBackground = true
        scope.launch {
            val reader = process.inputStream.bufferedReader()
            while (eatInBackground) {
                reader.readLine() ?: break
            }
            Log.w(SHARED_TAG, "Logcat eating complete")
        }
    }
}