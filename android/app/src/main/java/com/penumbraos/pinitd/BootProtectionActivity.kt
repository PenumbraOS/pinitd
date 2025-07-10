package com.penumbraos.pinitd

import android.app.Activity
import android.os.Bundle
import android.util.Log

/**
 * Activity for ADB control of pinitd boot protection.
 */
class BootProtectionActivity : Activity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        
        val protection = BootLoopProtection(this)
        val action = intent.getStringExtra("action")
        
        when (action) {
            "status" -> {
                Log.i(SHARED_TAG, protection.getStatus())
            }
            "override" -> {
                protection.enableManualOverride()
                Log.i(SHARED_TAG, "Manual override enabled")
            }
            "reset" -> {
                protection.reset()
                Log.i(SHARED_TAG, "Boot protection reset")
            }
            else -> {
                Log.w(SHARED_TAG, "Unknown action: $action")
                Log.i(SHARED_TAG, "Available actions: status, override, reset")
            }
        }
        
        finish()
    }
}