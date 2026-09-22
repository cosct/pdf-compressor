package com.cosct.pdfcompressor

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable

/**
 * Follow-system light/dark Material3 scheme. Brand colors align with the
 * desktop build once the palette is extracted from the Tauri theme; the
 * skeleton ships the baseline purple-free defaults.
 */
@Composable
fun AppTheme(content: @Composable () -> Unit) {
    val scheme = if (isSystemInDarkTheme()) darkColorScheme() else lightColorScheme()
    MaterialTheme(colorScheme = scheme, content = content)
}
