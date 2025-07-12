package com.penumbraos.pinitd

import android.app.Activity
import android.os.Bundle

class LauncherActivity : Activity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        launchWithBootProtection(this)
    }
}