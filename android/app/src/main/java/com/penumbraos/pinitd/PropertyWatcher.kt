package com.penumbraos.pinitd

import android.os.Build
import android.os.SystemProperties
import android.util.Log
import kotlinx.coroutines.suspendCancellableCoroutine
import kotlinx.coroutines.withTimeoutOrNull
import kotlin.coroutines.resume

class PropertyWatcher {
    fun clearStatus() {
        // Clear any existing status
        SystemProperties.set("pinitd.boot.status", "")
    }

    suspend fun waitForSuccess(timeoutMs: Long): Boolean {
        return try {
            Log.i(SHARED_TAG, "Waiting for pinitd success property (timeout: ${timeoutMs}ms)")

            val result = withTimeoutOrNull(timeoutMs) {
                suspendCancellableCoroutine<Boolean> { continuation ->
                    val callback = Runnable {
                        val status = SystemProperties.get("pinitd.boot.status", "")
                        when (status) {
                            "success" -> {
                                Log.i(SHARED_TAG, "Received pinitd success signal")
                                continuation.resume(true)
                            }
                            "failure" -> {
                                Log.w(SHARED_TAG, "Received pinitd failure signal")
                                continuation.resume(false)
                            }
                        }
                    }

                    SystemProperties.addChangeCallback(callback)

                    continuation.invokeOnCancellation {
                        try {
                            SystemProperties.removeChangeCallback(callback)
                        } catch (e: Exception) {
                            Log.w(SHARED_TAG, "Error removing property callback: ${e.message}")
                        }
                    }
                }
            }
            
            if (result == null) {
                Log.w(SHARED_TAG, "Timeout waiting for pinitd success signal")
                false
            } else {
                result
            }
        } catch (e: Exception) {
            Log.e(SHARED_TAG, "Error waiting for property: ${e.message}")
            false
        }
    }
}