package im.agg.pinitd

import android.os.Bundle
import android.util.Log
import androidx.appcompat.app.AppCompatActivity

// adb shell am start -n im.agg.pinitd/.DummyActivity
class DummyActivity : AppCompatActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        Log.w("pinitd-trampoline", "DummyActivity created")
        launch(this)

        finishAndRemoveTask()
    }
}