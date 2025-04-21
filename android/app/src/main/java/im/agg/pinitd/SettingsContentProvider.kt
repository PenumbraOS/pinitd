package im.agg.pinitd

import android.content.ContentProvider
import android.content.ContentValues
import android.content.Context
import android.content.UriMatcher
import android.database.Cursor
import android.database.MatrixCursor
import android.net.Uri
import android.util.Log

// adb shell content update --uri content://im.agg.pinitd/settings/your_setting_key --bind value:s:your_new_value
class SettingsContentProvider : ContentProvider() {
    companion object {
        const val AUTHORITY = "im.agg.pinitd"
        const val PATH_SETTINGS = "settings"

        private const val TAG = "SettingsProvider"

        // Define column names for the cursor
        const val COLUMN_KEY = "key"
        const val COLUMN_VALUE = "value"

        // URI Matcher codes
        private const val SETTINGS = 100 // Match for the settings path
        private const val SETTING_ITEM = 101 // Match for a specific setting key

        private val uriMatcher = UriMatcher(UriMatcher.NO_MATCH).apply {
            addURI(AUTHORITY, PATH_SETTINGS, SETTINGS) // content://authority/settings
            addURI(AUTHORITY, "$PATH_SETTINGS/*", SETTING_ITEM) // content://authority/settings/key
        }
    }

    override fun getType(uri: Uri): String? {
        return when (uriMatcher.match(uri)) {
            SETTINGS -> "vnd.android.cursor.dir/$AUTHORITY.$PATH_SETTINGS"
            SETTING_ITEM -> "vnd.android.cursor.item/$AUTHORITY.$PATH_SETTINGS"
            else -> null
        }
    }

    override fun onCreate(): Boolean {
        return true
    }

    override fun update(
        uri: Uri,
        values: ContentValues?,
        selection: String?,
        selectionArgs: Array<out String?>?
    ): Int {
        val context = context ?: return 0
        val prefs = context.getSharedPreferences(context.packageName, Context.MODE_PRIVATE)
        val editor = prefs.edit()
        var rowsAffected = 0

        when (uriMatcher.match(uri)) {
            SETTING_ITEM -> {
                val settingKey = uri.lastPathSegment
                if (settingKey == "command" && values != null) {
                    val valueToSet = values.getAsString("value") // Matches --bind value:s:xyz

                    if (valueToSet != null) {
                        Log.w(TAG, "Updating setting via Provider: Key='$settingKey', Value='$valueToSet'")
                        editor.putString(settingKey, valueToSet)
                        editor.apply()
                        rowsAffected = 1
                        // Notify potential observers about the change
                        context.contentResolver.notifyChange(uri, null)
                    } else {
                        Log.w(TAG, "Value not provided or not found in ContentValues for key '$settingKey'")
                    }
                } else {
                    Log.w(TAG, "Setting key missing in URI or ContentValues are null")
                }
            }
            SETTINGS -> {
                Log.e(TAG, "Updating the entire collection via URI $uri not supported")
            }
            else -> {
                Log.e(TAG, "Unknown URI for update: $uri")
                throw IllegalArgumentException("Unknown URI: $uri")
            }
        }
        return rowsAffected
    }

    override fun insert(uri: Uri, values: ContentValues?): Uri? {
        Log.e(TAG, "Insert for URI $uri not supported")
        return null
    }

    override fun delete(uri: Uri, selection: String?, selectionArgs: Array<out String?>?): Int {
        Log.e(TAG, "Delete for URI $uri not supported")
        return 0
    }

    override fun query(
        uri: Uri,
        projection: Array<out String?>?,
        selection: String?,
        selectionArgs: Array<out String?>?,
        sortOrder: String?
    ): Cursor? {
        val context = context ?: return null // Need context to access SharedPreferences
        val prefs = context.getSharedPreferences(context.packageName, Context.MODE_PRIVATE)

        // Determine the columns to include in the result. Default to (key, value) if null.
        val finalProjection = projection ?: arrayOf(COLUMN_KEY, COLUMN_VALUE)
        val cursor = MatrixCursor(finalProjection)

        when (uriMatcher.match(uri)) {
            SETTING_ITEM -> {
                val settingKey = uri.lastPathSegment
                if (settingKey == null) {
                    Log.e(TAG, "Query URI is missing the setting key: $uri")
                    return null
                }

                // Check if the key exists in SharedPreferences
                if (prefs.contains(settingKey)) {
                    val value = prefs.getString(settingKey, null)

                    cursor.addRow(arrayOf(settingKey, value))
                    Log.w(TAG, "Query successful for key '$settingKey', value '$value'")
                } else {
                    // Key not found in SharedPreferences
                    Log.w(TAG, "Query failed, key not found: '$settingKey'")
                    // Return empty cursor, as query was valid but yielded no results
                }
            }
            SETTINGS -> {
                Log.e(TAG, "Querying all settings (URI: $uri) is not implemented. Returning empty cursor.")
            }
            else -> {
                Log.e(TAG, "Unknown URI for query: $uri")
                return null
            }
        }

        // Set notification URI so observers can re-query if data changes
        cursor.setNotificationUri(context.contentResolver, uri)

        return cursor

    }
}