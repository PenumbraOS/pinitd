package im.agg.pinitd

import android.app.Activity
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.util.Log

// adb shell am start -n im.agg.pinitd/.DummyActivity
class DummyActivity : Activity() {
    val handler = Handler(Looper.getMainLooper())

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        Log.w("pinitd-trampoline", "DummyActivity created. Waiting...")
        handler.postDelayed(Runnable {
            launch(this)
            finishAndRemoveTask()
        }, 1000)
    }
}