package com.cosct.pdfcompressor

import android.content.ClipData
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Bundle
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.compose.setContent
import androidx.activity.result.contract.ActivityResultContracts
import androidx.appcompat.app.AppCompatActivity
import androidx.appcompat.app.AppCompatDelegate
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.core.content.FileProvider
import androidx.core.os.LocaleListCompat
import androidx.lifecycle.viewmodel.compose.viewModel
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import uniffi.pdfcompressor.FfiAnalysis
import uniffi.pdfcompressor.FfiCompressResult
import java.io.File
import java.io.IOException

/**
 * MVP single-file flow (Phase 3): SAF pick → analyze → preset picker (Auto
 * plus the engine's three rows, with the simplified custom quality/edge
 * panel) → compress with live progress + cancel → SAF save / FileProvider
 * share of the optimized copy. Flow state lives in [CompressViewModel] so
 * rotation and theme/locale recreations do not lose a run. Password retry
 * keeps the password in memory only — never persisted (desktop discipline).
 * Theme and language persist via DataStore; the queue/background batch grows
 * from here (Phase 4).
 */
class MainActivity : AppCompatActivity() {

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val settingsRepository = SettingsRepository(applicationContext)
        setContent {
            // Null until DataStore's first real emission. The side effects
            // below must never fire for this placeholder: applying DEFAULT
            // (system theme/locale) before the persisted value arrives
            // bounces night mode / locales and recreates the activity in a
            // loop on cold start. Rendering with DEFAULT meanwhile is fine.
            val settings by settingsRepository.data.collectAsState(initial = null)
            val snapshot = settings ?: SettingsSnapshot.DEFAULT
            // Persisted theme → appcompat night mode (keeps the window in
            // step with the Compose scheme) and → the Material scheme.
            LaunchedEffect(settings?.themeMode) {
                val themeMode = settings?.themeMode ?: return@LaunchedEffect
                AppCompatDelegate.setDefaultNightMode(
                    when (themeMode) {
                        ThemeMode.SYSTEM -> AppCompatDelegate.MODE_NIGHT_FOLLOW_SYSTEM
                        ThemeMode.LIGHT -> AppCompatDelegate.MODE_NIGHT_NO
                        ThemeMode.DARK -> AppCompatDelegate.MODE_NIGHT_YES
                    },
                )
            }
            // Persisted language → per-app locales (no-op when unchanged, so
            // cold starts with appcompat's autoStoreLocales do not loop).
            LaunchedEffect(settings?.language) {
                val language = settings?.language ?: return@LaunchedEffect
                AppCompatDelegate.setApplicationLocales(
                    if (language == "system") {
                        LocaleListCompat.getEmptyLocaleList()
                    } else {
                        LocaleListCompat.forLanguageTags(language)
                    },
                )
            }
            AppTheme(themeMode = snapshot.themeMode) {
                Surface(modifier = Modifier.fillMaxSize()) {
                    CompressScreen(settingsRepository, snapshot)
                }
            }
        }
    }
}

@Composable
private fun CompressScreen(
    settingsRepository: SettingsRepository,
    settings: SettingsSnapshot,
    viewModel: CompressViewModel = viewModel(),
) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var showSettings by rememberSaveable { mutableStateOf(false) }
    val stage = viewModel.stage

    val pickPdf = rememberLauncherForActivityResult(
        ActivityResultContracts.OpenDocument(),
    ) { uri ->
        if (uri != null) viewModel.onPdfPicked(context, uri)
    }

    val saveOutput = rememberLauncherForActivityResult(
        ActivityResultContracts.CreateDocument("application/pdf"),
    ) { uri ->
        val done = viewModel.stage as? Stage.Done ?: return@rememberLauncherForActivityResult
        if (uri != null) {
            scope.launch {
                try {
                    context.contentResolver.openOutputStream(uri)?.use { it.write(done.result.bytes) }
                } catch (failure: IOException) {
                    viewModel.reportError(failure.message)
                }
            }
        }
    }

    Column(
        modifier = Modifier
            .fillMaxSize()
            .verticalScroll(rememberScrollState())
            .padding(24.dp),
        verticalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.SpaceBetween,
        ) {
            Text(stringResource(R.string.app_name), style = MaterialTheme.typography.headlineMedium)
            TextButton(onClick = { showSettings = true }) {
                Text(stringResource(R.string.settings_title))
            }
        }
        Text(
            stringResource(R.string.engine_status_ready) + " · " + viewModel.enginePing,
            style = MaterialTheme.typography.bodySmall,
        )

        viewModel.error?.let { message ->
            Text(message, color = MaterialTheme.colorScheme.error)
        }

        when (val current = stage) {
            is Stage.Idle -> {
                Button(
                    onClick = { pickPdf.launch(arrayOf("application/pdf")) },
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    Text(stringResource(R.string.pick_pdf))
                }
            }

            is Stage.Analyzing -> {
                Text(stringResource(R.string.analyzing))
                LinearProgressIndicator(modifier = Modifier.fillMaxWidth())
            }

            is Stage.Ready -> {
                AnalysisCard(analysis = current.analysis)
                PresetPanel(
                    table = viewModel.presetTable,
                    recommended = current.analysis.recommendedPreset,
                    choice = settings.presetChoice,
                    onChoice = { choice ->
                        viewModel.qualityOverride = null
                        viewModel.maxEdgeOverride = null
                        scope.launch { settingsRepository.setPresetChoice(choice) }
                    },
                    quality = viewModel.effectiveQuality(current.analysis, settings.presetChoice),
                    onQuality = { viewModel.qualityOverride = it },
                    maxEdgePx = viewModel.maxEdgeOverride,
                    onMaxEdge = { viewModel.maxEdgeOverride = it },
                )
                Button(
                    onClick = {
                        viewModel.runCompress(context, current, current.password, settings.presetChoice)
                    },
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    Text(stringResource(R.string.start_compress))
                }
            }

            is Stage.Compressing -> {
                Text(stringResource(R.string.compressing))
                LinearProgressIndicator(
                    progress = { current.progress.coerceIn(0f, 1f) },
                    modifier = Modifier.fillMaxWidth(),
                )
                OutlinedButton(
                    onClick = { viewModel.cancelCompress() },
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    Text(stringResource(R.string.cancel))
                }
            }

            is Stage.Done -> {
                ResultCard(result = current.result, displayName = current.displayName)
                if (current.result.outputWasSmaller) {
                    Button(
                        onClick = { saveOutput.launch(suggestedOutputName(current.displayName)) },
                        modifier = Modifier.fillMaxWidth(),
                    ) {
                        Text(stringResource(R.string.save_output))
                    }
                    OutlinedButton(
                        onClick = { scope.launch { shareResult(context, current) } },
                        modifier = Modifier.fillMaxWidth(),
                    ) {
                        Text(stringResource(R.string.share_output))
                    }
                } else {
                    Text(stringResource(R.string.result_not_smaller))
                }
            }
        }
    }

    viewModel.pendingPassword?.let { (ready, wasWrong) ->
        var password by rememberSaveable { mutableStateOf("") }
        AlertDialog(
            onDismissRequest = { viewModel.dismissPasswordDialog() },
            title = {
                Text(
                    stringResource(
                        if (wasWrong) R.string.error_wrongPassword else R.string.retry_password,
                    ),
                )
            },
            text = {
                OutlinedTextField(
                    value = password,
                    onValueChange = { password = it },
                    singleLine = true,
                )
            },
            confirmButton = {
                TextButton(onClick = {
                    viewModel.dismissPasswordDialog()
                    viewModel.runCompress(
                        context, ready, password.ifBlank { null }, settings.presetChoice,
                    )
                }) { Text("OK") }
            },
            dismissButton = {
                TextButton(onClick = { viewModel.dismissPasswordDialog() }) {
                    Text(stringResource(R.string.cancel))
                }
            },
        )
    }

    if (showSettings) {
        SettingsDialog(
            snapshot = settings,
            onTheme = { scope.launch { settingsRepository.setThemeMode(it) } },
            onLanguage = { scope.launch { settingsRepository.setLanguage(it) } },
            onDismiss = { showSettings = false },
        )
    }
}

@Composable
private fun AnalysisCard(analysis: FfiAnalysis) {
    Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
        Text(
            "${analysis.pageCount} pages · ${analysis.documentKind}",
            style = MaterialTheme.typography.titleMedium,
        )
        Text("images: ${analysis.imageObjectCount} · edge ${analysis.maxImageEdgePx}px")
        Text("estimated savings: ${analysis.estimatedSavingsPercent.toInt()}%")
        Text("recommended preset: ${presetDisplayName(analysis.recommendedPreset)}")
    }
}

@Composable
private fun ResultCard(result: FfiCompressResult, displayName: String) {
    val context = LocalContext.current
    fun human(bytes: ULong): String =
        android.text.format.Formatter.formatShortFileSize(context, bytes.toLong())
    Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
        Text(displayName, style = MaterialTheme.typography.titleMedium)
        Text(
            stringResource(
                R.string.result_summary,
                human(result.originalSizeBytes),
                human(result.compressedSizeBytes),
                human(result.savedBytes),
            ),
        )
        Text("saved ${result.savingsPercent.toInt()}% · ${result.elapsedMs} ms")
    }
}

fun queryDisplayName(context: Context, uri: Uri): String? =
    runCatching {
        context.contentResolver.query(uri, null, null, null, null)?.use { cursor ->
            val index = cursor.getColumnIndex(android.provider.OpenableColumns.DISPLAY_NAME)
            if (index >= 0 && cursor.moveToFirst()) cursor.getString(index) else null
        }
    }.getOrNull()

private fun suggestedOutputName(displayName: String): String =
    displayName.removeSuffix(".pdf") + "_optimized.pdf"

/** Share the optimized copy through the system sheet. The bytes land in the
 *  FileProvider-backed cache first so the receiver gets a revocable
 *  content:// grant instead of our raw storage. */
private suspend fun shareResult(context: Context, done: Stage.Done) {
    runCatching {
        val name = suggestedOutputName(done.displayName)
        val uri = withContext(Dispatchers.IO) {
            val dir = File(context.cacheDir, "shared").apply { mkdirs() }
            val file = File(dir, name)
            file.writeBytes(done.result.bytes)
            FileProvider.getUriForFile(context, "${context.packageName}.fileprovider", file)
        }
        val send = Intent(Intent.ACTION_SEND).apply {
            type = "application/pdf"
            putExtra(Intent.EXTRA_STREAM, uri)
            putExtra(Intent.EXTRA_SUBJECT, name)
            clipData = ClipData.newRawUri(null, uri)
            addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
        }
        context.startActivity(Intent.createChooser(send, null))
    }
}
