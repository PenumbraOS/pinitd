package com.penumbraos.pinitd

import android.app.IActivityManager
import android.content.ContentValues
import android.os.Binder
import android.os.ServiceManager
import android.util.Log
import android.provider.Settings.Global.NAME
import android.provider.Settings.NameValueTable.VALUE

@Suppress("DEPRECATION")
class SettingWriter {
    companion object {
        @JvmStatic
        fun main(args: Array<String>) {
            var packageName: String? = null
            var payload: String? = null

            if (args.count() >= 1) {
                packageName = args[0]
            } else {
                Log.e(TAG, "Invalid number of arguments")
                return
            }

            if (args.count() >= 2) {
                payload = args[1]
            }

            try {
                val activityManagerBinder = ServiceManager.getService("activity")
                val activityManager = IActivityManager.Stub.asInterface(activityManagerBinder)
                val contentProvider = activityManager.getContentProviderExternal("settings", 0, Binder(), "*cmd*")

                if (payload != null) {
                    val contentValues = ContentValues()
                    contentValues.put(NAME, "hidden_api_blacklist_exemptions")
                    contentValues.put(VALUE, payload)

                    contentProvider.provider.insert(packageName, EXEMPTIONS_SETTING_URI, contentValues)
                    Log.w(TAG, "Successfully issued insert command")
                } else {
                    contentProvider.provider.delete(packageName, EXEMPTIONS_SETTING_URI, null, null)
                    Log.w(TAG, "Successfully issued delete command")
                }
            } catch (e: Exception) {
                Log.e(TAG, "An exception occurred:")
                e.printStackTrace()
            }
        }
    }
}