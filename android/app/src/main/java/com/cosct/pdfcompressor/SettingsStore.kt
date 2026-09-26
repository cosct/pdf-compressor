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
    /** Persisted SAF tree the queue writes optimized copies into; null until
     *  the user picks one (Phase 4). */
    val outputTree: String?,
) {
    companion object {
        val DEFAULT = SettingsSnapshot("auto", ThemeMode.SYSTEM, "system", null)
    }
}

private val Context.settingsDataStore by preferencesDataStore(name = "settings")

class SettingsRepository(private val context: Context) {

    private object Keys {
        val PRESET_CHOICE = stringPreferencesKey("preset_choice")
        val THEME_MODE = stringPreferencesKey("theme_mode")
        val LANGUAGE = stringPreferencesKey("language")
        val OUTPUT_TREE = stringPreferencesKey("output_tree")
        val RECENTS = stringPreferencesKey("recents")
    }

    val data: Flow<SettingsSnapshot> = context.settingsDataStore.data.map { prefs ->
        SettingsSnapshot(
            presetChoice = prefs[Keys.PRESET_CHOICE] ?: "auto",
            themeMode = prefs[Keys.THEME_MODE]
                ?.let { runCatching { ThemeMode.valueOf(it) }.getOrNull() }
                ?: ThemeMode.SYSTEM,
            language = prefs[Keys.LANGUAGE] ?: "system",
            outputTree = prefs[Keys.OUTPUT_TREE],
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

    suspend fun setOutputTree(tree: String) {
        context.settingsDataStore.edit { it[Keys.OUTPUT_TREE] = tree }
    }

    /** Recently picked documents (Phase 5), newest first, capped at 10.
     *  Entries carry a persistable read grant where the provider allows it;
     *  a stale entry is dropped on first failed open. */
    val recents: Flow<List<RecentFile>> = context.settingsDataStore.data.map { prefs ->
        decodeRecents(prefs[Keys.RECENTS] ?: "")
    }

    suspend fun addRecent(uri: String, name: String) {
        context.settingsDataStore.edit { prefs ->
            val entry = RecentFile(uri, name)
            val merged = (listOf(entry) + decodeRecents(prefs[Keys.RECENTS] ?: "")
                .filter { it.uri != uri }).take(10)
            prefs[Keys.RECENTS] = encodeRecents(merged)
        }
    }

    suspend fun removeRecent(uri: String) {
        context.settingsDataStore.edit { prefs ->
            prefs[Keys.RECENTS] = encodeRecents(
                decodeRecents(prefs[Keys.RECENTS] ?: "").filter { it.uri != uri },
            )
        }
    }

    private fun encodeRecents(list: List<RecentFile>): String =
        list.joinToString("\n") { "${it.uri}|${it.name.replace("|", "/")}" }

    private fun decodeRecents(raw: String): List<RecentFile> =
        raw.lines().mapNotNull { line ->
            val parts = line.split("|", limit = 2)
            if (parts.size == 2 && parts[0].isNotEmpty()) RecentFile(parts[0], parts[1]) else null
        }
}

data class RecentFile(val uri: String, val name: String)
