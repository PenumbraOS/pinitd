package im.agg.pinitd

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
        ProcessBuilder(listOf("logcat")).start().also { process ->
            this.process = process
        }
    }

    suspend fun waitForSubstring(substring: String, timeout: Duration): Boolean {
        eatInBackground = false
        try {
            withTimeout(timeout) {
                val reader = process.inputStream.bufferedReader()
                while (true) {
                    val line = reader.readLine() ?: break
                    if (line.contains(substring)) {
                        return@withTimeout
                    }
                }
            }
            return true
        } catch (e: Exception) {
            return false
        }
    }

    suspend fun eatInBackground() {
        eatInBackground = true
        scope.launch {
            val reader = process.inputStream.bufferedReader()
            while (eatInBackground) {
                reader.readLine() ?: break
            }
        }
    }
}