package com.cosct.pdfcompressor

import android.content.Context
import androidx.datastore.preferences.core.edit
import androidx.datastore.preferences.core.stringPreferencesKey
import androidx.datastore.preferences.preferencesDataStore
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.map

enum class ThemeMode { SYSTEM, LIGHT, DARK }

/** Persisted user choices (plan Phase 3 item 5): preset choice, theme,
 *  language. Passwords never land here — session memory only, same
 *  discipline as the desktop. */
data class SettingsSnapshot(
    /** "auto" follows the engine's per-document recommendation; otherwise a
     *  preset label from `preset_defaults()` (conservative/balanced/maximum). */
    val presetChoice: String,
    val themeMode: ThemeMode,
    /** "system" or a BCP-47 tag ("en", "zh-CN"). */
    val language: String,
) {
    companion object {
        val DEFAULT = SettingsSnapshot("auto", ThemeMode.SYSTEM, "system")
    }
}

private val Context.settingsDataStore by preferencesDataStore(name = "settings")

class SettingsRepository(private val context: Context) {

    private object Keys {
        val PRESET_CHOICE = stringPreferencesKey("preset_choice")
        val THEME_MODE = stringPreferencesKey("theme_mode")
        val LANGUAGE = stringPreferencesKey("language")
    }

    val data: Flow<SettingsSnapshot> = context.settingsDataStore.data.map { prefs ->
        SettingsSnapshot(
            presetChoice = prefs[Keys.PRESET_CHOICE] ?: "auto",
            themeMode = prefs[Keys.THEME_MODE]
                ?.let { runCatching { ThemeMode.valueOf(it) }.getOrNull() }
                ?: ThemeMode.SYSTEM,
            language = prefs[Keys.LANGUAGE] ?: "system",
        )
    }

    suspend fun setPresetChoice(choice: String) {
        context.settingsDataStore.edit { it[Keys.PRESET_CHOICE] = choice }
    }

    suspend fun setThemeMode(mode: ThemeMode) {
        context.settingsDataStore.edit { it[Keys.THEME_MODE] = mode.name }
    }

    suspend fun setLanguage(language: String) {
        context.settingsDataStore.edit { it[Keys.LANGUAGE] = language }
    }
}
