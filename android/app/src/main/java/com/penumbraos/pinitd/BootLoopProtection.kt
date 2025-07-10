package com.penumbraos.pinitd

import android.content.Context
import android.content.SharedPreferences
import android.util.Log
import kotlin.math.min
import kotlin.math.pow
import androidx.core.content.edit

private const val KEY_FAILURE_COUNT = "failure_count"
private const val KEY_LAST_ATTEMPT_TIME = "last_attempt_time"
private const val KEY_LAST_SUCCESS_TIME = "last_success_time"
private const val KEY_LAUNCH_DISABLED = "launch_disabled"
private const val KEY_MANUAL_OVERRIDE = "manual_override"

private const val MAX_FAILURES = 5
private const val LAUNCH_DISABLED_TIMEOUT_S = 10 * 60

class BootLoopProtection(private val context: Context) {
    private val prefs: SharedPreferences = context.getSharedPreferences("pinitd_boot_protection", Context.MODE_PRIVATE)

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
        
        return true
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
            appendLine("  Last Attempt: ${if (lastAttemptTime > 0) java.util.Date(lastAttemptTime) else "Never"}")
            appendLine("  Last Success: ${if (lastSuccessTime > 0) java.util.Date(lastSuccessTime) else "Never"}")
        }
    }
    
    fun reset() {
        prefs.edit { clear() }
        Log.i(SHARED_TAG, "Boot protection reset")
    }
}