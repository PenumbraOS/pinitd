package com.penumbraos.pinitddummy32

import android.app.Activity
import android.os.Bundle
import android.util.Log

class DummyActivity: Activity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        Log.w("pinitd-dummy32", "Dummy activity created")

        Thread.sleep(5000)
        finishAndRemoveTask()
    }
}