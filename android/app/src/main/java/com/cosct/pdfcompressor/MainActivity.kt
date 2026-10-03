package com.cosct.pdfcompressor

import android.content.ClipData
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.os.Bundle
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.compose.setContent
import androidx.activity.result.ActivityResultLauncher
import androidx.activity.result.contract.ActivityResultContracts
import androidx.appcompat.app.AppCompatActivity
import androidx.appcompat.app.AppCompatDelegate
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.FilterChip
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
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.core.content.FileProvider
import androidx.core.net.toUri
import androidx.core.os.LocaleListCompat
import androidx.lifecycle.viewmodel.compose.viewModel
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
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

    /** PDF handed in via ACTION_SEND (share-into-app); consumed once by the
     *  single-file flow. The grant is transient, so these URIs are read
     *  immediately and never recorded in recents. */
    private val sharedPdf = MutableStateFlow<Uri?>(null)

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        handleSendIntent(intent)
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
                    CompressScreen(settingsRepository, snapshot, sharedPdf)
                }
            }
        }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        handleSendIntent(intent)
    }

    private fun handleSendIntent(intent: Intent?) {
        if (intent?.action != Intent.ACTION_SEND) return
        val uri = if (Build.VERSION.SDK_INT >= 33) {
            intent.getParcelableExtra(Intent.EXTRA_STREAM, Uri::class.java)
        } else {
            @Suppress("DEPRECATION")
            intent.getParcelableExtra(Intent.EXTRA_STREAM)
        }
        if (uri != null) sharedPdf.value = uri
    }
}

@Composable
private fun CompressScreen(
    settingsRepository: SettingsRepository,
    settings: SettingsSnapshot,
    sharedPdf: MutableStateFlow<Uri?>,
    viewModel: CompressViewModel = viewModel(),
) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var showSettings by rememberSaveable { mutableStateOf(false) }
    var showQueue by rememberSaveable { mutableStateOf(false) }
    val stage = viewModel.stage

    // Share-into-app intake: analyze straight away, then consume so a
    // recreation does not re-trigger.
    val shared by sharedPdf.collectAsState()
    LaunchedEffect(shared) {
        val uri = shared ?: return@LaunchedEffect
        viewModel.onPdfPicked(context, uri, persistable = false)
        sharedPdf.value = null
    }

    val pickPdf = rememberLauncherForActivityResult(
        ActivityResultContracts.OpenDocument(),
    ) { uri ->
        if (uri != null) {
            viewModel.onPdfPicked(context, uri)
            scope.launch {
                settingsRepository.addRecent(
                    uri.toString(),
                    queryDisplayName(context, uri) ?: "document.pdf",
                )
            }
        }
    }

    val saveOutput = rememberLauncherForActivityResult(
        ActivityResultContracts.CreateDocument("application/pdf"),
    ) { uri ->
        val done = viewModel.stage as? Stage.Done ?: return@rememberLauncherForActivityResult
        if (uri != null) {
            scope.launch {
                try {
                    writeNewPdf(context, uri, done.result.bytes)
                } catch (cancelled: kotlinx.coroutines.CancellationException) {
                    throw cancelled
                } catch (failure: Exception) {
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

        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            FilterChip(
                selected = !showQueue,
                onClick = { showQueue = false },
                label = { Text(stringResource(R.string.tab_single)) },
            )
            FilterChip(
                selected = showQueue,
                onClick = { showQueue = true },
                label = { Text(stringResource(R.string.tab_queue)) },
            )
        }

        if (showQueue) {
            QueueScreen(settings = settings, settingsRepository = settingsRepository)
        } else {
            SingleFileFlow(
                viewModel = viewModel,
                settings = settings,
                settingsRepository = settingsRepository,
                stage = stage,
                pickPdf = { pickPdf.launch(arrayOf("application/pdf")) },
                saveOutput = saveOutput,
            )
        }
    }

    viewModel.pendingPassword?.let { (ready, wasWrong) ->
        var password by remember(ready) { mutableStateOf("") }
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
                    visualTransformation = androidx.compose.ui.text.input.PasswordVisualTransformation(),
                    singleLine = true,
                )
            },
            confirmButton = {
                TextButton(onClick = {
                    viewModel.dismissPasswordDialog()
                    viewModel.retryAnalysis(context, ready, password.ifBlank { null })
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

/** The interactive single-document flow (idle → analyzing → ready →
 *  compressing → done), extracted so the batch queue can take over the
 *  screen without unmounting the shared header. */
@Composable
private fun SingleFileFlow(
    viewModel: CompressViewModel,
    settings: SettingsSnapshot,
    settingsRepository: SettingsRepository,
    stage: Stage,
    pickPdf: () -> Unit,
    saveOutput: ActivityResultLauncher<String>,
) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()

    viewModel.error?.let { message ->
        Text(message, color = MaterialTheme.colorScheme.error)
    }

    when (val current = stage) {
        is Stage.Idle -> {
            Button(
                onClick = { pickPdf() },
                modifier = Modifier.fillMaxWidth(),
            ) {
                Text(stringResource(R.string.pick_pdf))
            }
            val recents by settingsRepository.recents.collectAsState(initial = emptyList())
            if (recents.isNotEmpty()) {
                Text(
                    stringResource(R.string.recent_files),
                    style = MaterialTheme.typography.titleSmall,
                )
                recents.forEach { recent ->
                    TextButton(
                        onClick = {
                            val uri = recent.uri.toUri()
                            viewModel.onPdfPicked(context, uri, recent.name) {
                                scope.launch { settingsRepository.removeRecent(recent.uri) }
                                viewModel.reportError(
                                    context.getString(R.string.recent_open_failed),
                                )
                            }
                        },
                        modifier = Modifier.fillMaxWidth(),
                    ) {
                        Text(
                            recent.name,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                            modifier = Modifier.fillMaxWidth(),
                        )
                    }
                }
            }
        }

        is Stage.Reading, is Stage.Analyzing -> {
            Text(stringResource(R.string.analyzing))
            LinearProgressIndicator(modifier = Modifier.fillMaxWidth())
            TextButton(onClick = { viewModel.cancelCompress() }) { Text(stringResource(R.string.cancel)) }
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
            Text(
                stringResource(R.string.target_size_label),
                style = MaterialTheme.typography.titleSmall,
            )
            Row(
                horizontalArrangement = Arrangement.spacedBy(8.dp),
                modifier = Modifier.horizontalScroll(rememberScrollState()),
            ) {
                FilterChip(
                    selected = viewModel.targetSizeMb == null,
                    onClick = { viewModel.targetSizeMb = null },
                    label = { Text(stringResource(R.string.target_off)) },
                )
                listOf(2, 5, 10).forEach { mb ->
                    FilterChip(
                        selected = viewModel.targetSizeMb == mb,
                        onClick = { viewModel.targetSizeMb = mb },
                        label = { Text("$mb MB") },
                    )
                }
            }
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
            TextButton(
                onClick = { viewModel.resetToIdle() },
                modifier = Modifier.fillMaxWidth(),
            ) {
                Text(stringResource(R.string.pick_another))
            }
        }
    }
}

@Composable
private fun AnalysisCard(analysis: FfiAnalysis) {
    val context = LocalContext.current
    var showDetails by rememberSaveable { mutableStateOf(false) }
    Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
        Text(
            stringResource(
                R.string.analysis_summary,
                analysis.pageCount.toInt(),
                documentKindName(analysis.documentKind),
            ),
            style = MaterialTheme.typography.titleMedium,
        )
        Text(
            stringResource(
                R.string.analysis_images,
                analysis.imageObjectCount.toInt(),
                analysis.maxImageEdgePx.toInt(),
            ),
        )
        Text(
            stringResource(
                R.string.analysis_estimated,
                analysis.estimatedSavingsPercent.toInt(),
            ),
        )
        Text(
            stringResource(
                R.string.analysis_recommended,
                presetDisplayName(analysis.recommendedPreset),
            ),
        )
        TextButton(onClick = { showDetails = !showDetails }) {
            Text(stringResource(R.string.details_expand) + if (showDetails) " ▴" else " ▾")
        }
        if (showDetails) {
            Text(
                stringResource(
                    R.string.detail_file_size,
                    android.text.format.Formatter.formatShortFileSize(
                        context,
                        analysis.fileSizeBytes.toLong(),
                    ),
                ),
            )
            Text(
                stringResource(
                    R.string.detail_confidence,
                    analysis.scannedConfidence.toInt(),
                ),
            )
            Text(
                stringResource(R.string.detail_coverage, analysis.imageCoverage.toInt()),
            )
            Text(
                stringResource(
                    R.string.detail_recommended_params,
                    analysis.recommendedImageQuality.toInt(),
                    analysis.recommendedMaxImageSizePx.toInt(),
                ),
            )
            if (analysis.notices.isNotEmpty()) {
                Text(stringResource(R.string.detail_notices))
                analysis.notices.forEach { notice ->
                    Text(
                        "· ${notice.fallback}",
                        style = MaterialTheme.typography.bodySmall,
                    )
                }
            }
        }
    }
}

@Composable
fun documentKindName(kind: String): String = when (kind) {
    "text-native" -> stringResource(R.string.kind_text_native)
    "mixed" -> stringResource(R.string.kind_mixed)
    "scan-heavy" -> stringResource(R.string.kind_scan_heavy)
    else -> kind
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
        Text(
            stringResource(
                R.string.result_saved_line,
                result.savingsPercent.toInt(),
                result.elapsedMs.toInt(),
            ),
        )
        // e.g. an unmet target-size budget surfaces as an engine notice.
        result.notices.forEach { notice ->
            Text(
                "· ${notice.fallback}",
                style = MaterialTheme.typography.bodySmall,
            )
        }
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
