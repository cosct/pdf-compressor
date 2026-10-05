package com.cosct.pdfcompressor

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.RadioButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp

/** Settings dialog: theme mode + language. The preset choice persists from
 *  the picker itself (Phase 3 item 5), so it needs no entry here. */
@Composable
fun SettingsDialog(
    snapshot: SettingsSnapshot,
    onTheme: (ThemeMode) -> Unit,
    onLanguage: (String) -> Unit,
    onDismiss: () -> Unit,
) {
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(stringResource(R.string.settings_title)) },
        text = {
            Column {
                Text(
                    stringResource(R.string.settings_theme),
                    style = MaterialTheme.typography.titleSmall,
                    modifier = Modifier.padding(vertical = 8.dp),
                )
                ThemeMode.entries.forEach { mode ->
                    val label = when (mode) {
                        ThemeMode.SYSTEM -> stringResource(R.string.theme_system)
                        ThemeMode.LIGHT -> stringResource(R.string.theme_light)
                        ThemeMode.DARK -> stringResource(R.string.theme_dark)
                    }
                    RadioRow(
                        label = label,
                        selected = snapshot.themeMode == mode,
                        onClick = { onTheme(mode) },
                    )
                }

                Text(
                    stringResource(R.string.settings_language),
                    style = MaterialTheme.typography.titleSmall,
                    modifier = Modifier.padding(vertical = 8.dp),
                )
                // Language names stay in their own tongue, as is convention.
                listOf(
                    "system" to stringResource(R.string.lang_system),
                    "en" to "English",
                    "zh-CN" to "中文",
                ).forEach { (tag, label) ->
                    RadioRow(
                        label = label,
                        selected = snapshot.language == tag,
                        onClick = { onLanguage(tag) },
                    )
                }
            }
        },
        confirmButton = {
            TextButton(onClick = onDismiss) { Text(stringResource(android.R.string.ok)) }
        },
    )
}

@Composable
private fun RadioRow(label: String, selected: Boolean, onClick: () -> Unit) {
    Row(
        verticalAlignment = Alignment.CenterVertically,
        modifier = Modifier
            .fillMaxWidth()
            .clickable(onClick = onClick),
    ) {
        RadioButton(selected = selected, onClick = onClick)
        Text(label)
    }
}
