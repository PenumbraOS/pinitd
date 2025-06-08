package im.agg.pinitd

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
        ProcessBuilder(listOf("logcat", "-v", "brief", "*:S", "ActivityManager:V")).start().also { process ->
            this.process = process
        }
    }

    suspend fun waitForSubstring(substring: String, timeout: Duration): Boolean {
        eatInBackground = false
        try {
            withTimeout(timeout) {
                val reader = process.inputStream.bufferedReader()
                Log.w("pinitd-trampoline", "Started waiting for substring ${reader.readLine()}")
                while (true) {
                    val line = reader.readLine() ?: break
                    if (line.contains(substring)) {
                        Log.w("pinitd-trampoline", "Matching string")
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
            Log.w("pinitd-trampoline", "Logcat eating complete")
        }
    }
}