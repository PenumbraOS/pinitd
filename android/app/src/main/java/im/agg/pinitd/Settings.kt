package im.agg.pinitd

import android.content.Context
import android.content.SharedPreferences
import androidx.core.content.edit

class Settings {
    private val prefs: SharedPreferences

    constructor(context: Context) {
        this.prefs = context.getSharedPreferences(context.packageName, Context.MODE_PRIVATE)
    }

    var command: String?
        get() {
            return this.prefs.getString("command", null)
        }
        set(value) {
            this.prefs.edit() { putString("command", value) }
        }
}