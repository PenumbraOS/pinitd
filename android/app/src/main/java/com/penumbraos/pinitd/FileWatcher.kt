package com.penumbraos.pinitd

import android.annotation.SuppressLint
import android.os.FileObserver
import android.util.Log
import kotlinx.coroutines.suspendCancellableCoroutine
import kotlinx.coroutines.withTimeoutOrNull
import java.io.File
import kotlin.coroutines.resume

class FileWatcher {
    @SuppressLint("SdCardPath")
    private val statusDir = File("/sdcard/penumbra/etc/pinitd/")
    private val successFile = File(statusDir, "boot_success")
    
    fun clearStatus() {
        try {
            // Ensure directory exists
            statusDir.mkdirs()
            
            // Clear any existing status files
            successFile.delete()
            Log.i(SHARED_TAG, "Cleared boot status files")
        } catch (e: Exception) {
            Log.w(SHARED_TAG, "Error clearing status files: ${e.message}")
        }
    }

    suspend fun waitForSuccess(timeoutMs: Long): Boolean {
        return try {
            Log.i(SHARED_TAG, "Waiting for pinitd success file (timeout: ${timeoutMs}ms)")
            
            val result = withTimeoutOrNull(timeoutMs) {
                suspendCancellableCoroutine<Boolean> { continuation ->
                    val observer = object : FileObserver(statusDir, FileObserver.CREATE) {
                        override fun onEvent(event: Int, path: String?) {
                            if (path == "boot_success") {
                                Log.i(SHARED_TAG, "Success file created")
                                successFile.delete()
                                continuation.resume(true)
                            }
                        }
                    }
                    
                    observer.startWatching()

                    if (successFile.exists()) {
                        // File is already here
                        Log.i(SHARED_TAG, "Success file already exists by time file watcher is set up")
                        successFile.delete()
                        continuation.resume(true)
                    }
                    
                    continuation.invokeOnCancellation {
                        observer.stopWatching()
                    }
                }
            }
            
            if (result == null) {
                Log.w(SHARED_TAG, "Timeout waiting for pinitd success file")
                false
            } else {
                result
            }
        } catch (e: Exception) {
            Log.e(SHARED_TAG, "Error waiting for status file: ${e.message}")
            false
        }
    }
}