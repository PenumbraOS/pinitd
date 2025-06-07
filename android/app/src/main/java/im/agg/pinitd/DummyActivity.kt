package im.agg.pinitd

import android.app.Activity
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.util.Log
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.launch

// adb shell am start -n im.agg.pinitd/.DummyActivity
class DummyActivity : Activity() {
    val scope = CoroutineScope(Dispatchers.Main)
    val handler = Handler(Looper.getMainLooper())

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        Log.w("pinitd-trampoline", "DummyActivity created. Waiting...")
        handler.postDelayed(Runnable {
            scope.launch {
                launchPinitd(scope, this@DummyActivity)
            }
            finishAndRemoveTask()
        }, 1000)
    }
}