package com.penumbraos.pinitd.util

import android.annotation.SuppressLint
import android.content.Context
import android.content.SharedPreferences
import android.util.Log
import androidx.core.content.edit
import com.penumbraos.pinitd.SHARED_TAG
import java.io.File
import java.io.FileInputStream
import java.util.Date

private const val KEY_FAILURE_COUNT = "failure_count"
private const val KEY_LAST_ATTEMPT_TIME = "last_attempt_time"
private const val KEY_LAST_SUCCESS_TIME = "last_success_time"
private const val KEY_LAUNCH_DISABLED = "launch_disabled"
private const val KEY_MANUAL_OVERRIDE = "manual_override"

private const val MAX_FAILURES = 5
private const val LAUNCH_DISABLED_TIMEOUT_S = 10 * 60

class BootLoopProtection(context: Context) {
    private val prefs: SharedPreferences = context.getSharedPreferences("pinitd_boot_protection", Context.MODE_PRIVATE)

    @SuppressLint("SdCardPath")
    fun shouldAttemptLaunch(): Boolean {
        val failureCount = prefs.getInt(KEY_FAILURE_COUNT, 0)
        val lastAttemptTime = prefs.getLong(KEY_LAST_ATTEMPT_TIME, 0)
        val launchDisabled = prefs.getBoolean(KEY_LAUNCH_DISABLED, false)
        val manualOverride = prefs.getBoolean(KEY_MANUAL_OVERRIDE, false)

        val currentTime = System.currentTimeMillis()

        Log.i(SHARED_TAG, "Boot protection check: failures=$failureCount, launchDisabled=$launchDisabled, override=$manualOverride")

        if (manualOverride) {
            Log.i(SHARED_TAG, "Manual override enabled, allowing launch")
            clearManualOverride()
            return true
        }

        if (launchDisabled) {
            if (currentTime - lastAttemptTime > LAUNCH_DISABLED_TIMEOUT_S * 1000) {
                Log.i(SHARED_TAG, "Launch disabled timeout expired, re-enabling")
                enableLaunch()
                return true
            } else {
                Log.w(SHARED_TAG, "Launch disabled, blocking launch")
                return false
            }
        }

        // Delay as SDCard might not be immediately mounted
        Thread.sleep(5000)

        // First, always signal zygote ready in case pinitd is running
        // This must happen before checking the lock file due to zygote restart timing
        Log.i(SHARED_TAG, "Signaling zygote ready first to handle race condition and delaying")
        createZygoteReadyFile()

        // Give pinitd a moment to process the signal and reacquire the lock if it's running
        Thread.sleep(5000)

        Log.i(SHARED_TAG, "Checking for running pinitd controller")

        try {
            val controllerLockFile = File("/sdcard/penumbra/etc/pinitd/pinitd.lock")
            val inputStream = FileInputStream(controllerLockFile)
            val lock = inputStream.channel.tryLock(0, Long.MAX_VALUE, true)

            if (lock != null) {
                Log.i(SHARED_TAG, "Controller lock file is not locked. pinitd is not running. Launch is required")
                lock.close()
                return true
            } else {
                Log.i(SHARED_TAG, "Controller lock file is locked. pinitd is running and has processed zygote ready. Preventing double launch")
                recordSuccess()
                Log.w(SHARED_TAG, "pinitd full startup successful")
                return false
            }
        } catch (e: Exception) {
            Log.e(SHARED_TAG, "Failed to test controller lock file: ${e.message}")
            e.printStackTrace()
            return true
        }
    }

    fun recordAttempt() {
        val failureCount = prefs.getInt(KEY_FAILURE_COUNT, 0) + 1
        val currentTime = System.currentTimeMillis()
        prefs.edit {
            putLong(KEY_LAST_ATTEMPT_TIME, currentTime)

            putInt(KEY_FAILURE_COUNT, failureCount)
            putLong(KEY_LAST_ATTEMPT_TIME, currentTime)

            if (failureCount >= MAX_FAILURES) {
                Log.e(SHARED_TAG, "Too many failures ($failureCount), disabling launch")
                putBoolean(KEY_LAUNCH_DISABLED, true)
            }
        }

        Log.i(SHARED_TAG, "Recorded boot attempt at $currentTime")
    }

    fun recordSuccess() {
        val currentTime = System.currentTimeMillis()
        prefs.edit {
            putInt(KEY_FAILURE_COUNT, 0)
            putLong(KEY_LAST_SUCCESS_TIME, currentTime)
            putBoolean(KEY_LAUNCH_DISABLED, false)
        }

        Log.i(SHARED_TAG, "Recorded successful launch, reset failure count")
    }

    fun enableManualOverride() {
        prefs.edit {
            putBoolean(KEY_MANUAL_OVERRIDE, true)
        }

        Log.i(SHARED_TAG, "Manual override enabled")
    }

    private fun clearManualOverride() {
        prefs.edit {
            putBoolean(KEY_MANUAL_OVERRIDE, false)
        }
    }

    private fun enableLaunch() {
        prefs.edit {
            putBoolean(KEY_LAUNCH_DISABLED, false)
            putInt(KEY_FAILURE_COUNT, 0)
        }
    }

    private fun createZygoteReadyFile() {
        try {
            val zygoteReadyFile = File("/sdcard/penumbra/etc/pinitd/zygote_ready")
            val parentDir = zygoteReadyFile.parentFile

            Log.w(SHARED_TAG, "Creating zygote ready file at ${zygoteReadyFile.absolutePath}")

            // Ensure parent directory exists
            if (parentDir != null && !parentDir.exists()) {
                if (parentDir.mkdirs()) {
                    Log.w(SHARED_TAG, "Created parent directory: ${parentDir.absolutePath}")
                } else {
                    Log.e(SHARED_TAG, "Failed to create parent directory: ${parentDir.absolutePath}")
                    return
                }
            }

            // Delete any existing file first
            if (zygoteReadyFile.exists()) {
                zygoteReadyFile.delete()
            }

            // Create new zygote ready file
            if (zygoteReadyFile.createNewFile()) {
                Log.w(SHARED_TAG, "Created zygote ready signal file at ${zygoteReadyFile.absolutePath}")
            } else {
                Log.e(SHARED_TAG, "Failed to create zygote ready file")
            }
        } catch (e: Exception) {
            Log.e(SHARED_TAG, "Exception creating zygote ready file: ${e.message}")
            e.printStackTrace()
        }
    }

    fun getStatus(): String {
        val failureCount = prefs.getInt(KEY_FAILURE_COUNT, 0)
        val lastAttemptTime = prefs.getLong(KEY_LAST_ATTEMPT_TIME, 0)
        val lastSuccessTime = prefs.getLong(KEY_LAST_SUCCESS_TIME, 0)
        val launchDisabled = prefs.getBoolean(KEY_LAUNCH_DISABLED, false)
        val manualOverride = prefs.getBoolean(KEY_MANUAL_OVERRIDE, false)

        return buildString {
            appendLine("Boot Protection Status:")
            appendLine("  Failure Count: $failureCount/$MAX_FAILURES")
            appendLine("  Launch Status: ${if (launchDisabled) "DISABLED" else "ENABLED"}")
            appendLine("  Manual Override: ${if (manualOverride) "ENABLED" else "DISABLED"}")
            appendLine("  Last Attempt: ${if (lastAttemptTime > 0) Date(lastAttemptTime) else "Never"}")
            appendLine("  Last Success: ${if (lastSuccessTime > 0) Date(lastSuccessTime) else "Never"}")
        }
    }

    fun reset() {
        prefs.edit { clear() }
        Log.i(SHARED_TAG, "Boot protection reset")
    }
}