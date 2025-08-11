package com.penumbraos.pinitd.util

import android.util.Log
import com.penumbraos.pinitd.SHARED_TAG
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.launch
import kotlinx.coroutines.withTimeout
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlinx.coroutines.suspendCancellableCoroutine
import kotlin.time.Duration
import java.util.concurrent.atomic.AtomicBoolean
import kotlin.coroutines.resume

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
        return try {
            withTimeout(timeout) {
                suspendCancellableCoroutine<Boolean> { continuation ->
                    val found = AtomicBoolean(false)
                    val thread = Thread {
                        try {
                            val reader = process.inputStream.bufferedReader()
                            Log.w(SHARED_TAG, "Started waiting for substring: \"$substring\"")
                            while (!Thread.currentThread().isInterrupted && !found.get()) {
                                val line = reader.readLine()
                                if (line == null) {
                                    if (!found.compareAndSet(false, true)) {
                                        return@Thread
                                    }
                                    continuation.resume(false)
                                    return@Thread
                                } else if (line.contains(substring)) {
                                    Log.w(SHARED_TAG, "Matching string")
                                    if (!found.compareAndSet(false, true)) {
                                        return@Thread
                                    }
                                    continuation.resume(true)
                                    return@Thread
                                }
                            }
                        } catch (_: Exception) {
                            if (!found.compareAndSet(false, true)) {
                                return@Thread
                            }
                            continuation.resume(false)
                        }
                    }
                    
                    continuation.invokeOnCancellation {
                        thread.interrupt()
                    }
                    
                    thread.start()
                }
            }
        } catch (_: Exception) {
            false
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