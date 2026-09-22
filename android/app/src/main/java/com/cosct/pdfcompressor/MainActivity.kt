package com.cosct.pdfcompressor

import android.content.Intent
import android.net.Uri
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.compose.setContent
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
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
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.Job
import kotlinx.coroutines.launch
import uniffi.pdfcompressor.FfiAnalysis
import uniffi.pdfcompressor.FfiCompressResult
import uniffi.pdfcompressor.FfiException
import java.io.IOException

/**
 * Skeleton single-file flow (spike S3 scope): SAF pick → analyze → compress
 * with live progress + cancel → SAF save of the optimized copy. Password
 * retry keeps the password in memory only — never persisted (desktop
 * discipline). The MVP queue/presets/settings screens grow from here.
 */
class MainActivity : ComponentActivity() {

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContent {
            AppTheme {
                Surface(modifier = Modifier.fillMaxSize()) {
                    CompressScreen()
                }
            }
        }
    }
}

private sealed interface Stage {
    data object Idle : Stage

    data class Analyzing(val uri: Uri, val bytes: ByteArray, val displayName: String) : Stage

    data class Ready(
        val uri: Uri,
        val bytes: ByteArray,
        val displayName: String,
        val analysis: FfiAnalysis,
        val password: String?,
    ) : Stage

    data class Compressing(
        val uri: Uri,
        val bytes: ByteArray,
        val displayName: String,
        val analysis: FfiAnalysis,
        val password: String?,
        val progress: Float,
    ) : Stage

    data class Done(
        val result: FfiCompressResult,
        val displayName: String,
    ) : Stage
}

@Composable
private fun CompressScreen() {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var stage by remember { mutableStateOf<Stage>(Stage.Idle) }
    // S2: engine liveness on first composition.
    val enginePing = remember { PdfEngine.probe() }
    var error by remember { mutableStateOf<String?>(null) }
    var pendingPassword by remember { mutableStateOf<Pair<Stage.Ready, Boolean>?>(null) }
    var compressJob by remember { mutableStateOf<Job?>(null) }

    fun runCompress(ready: Stage.Ready, password: String?) {
        val progress = PdfEngine.Progress()
        stage = Stage.Compressing(
            ready.uri, ready.bytes, ready.displayName, ready.analysis, password, 0f,
        )
        compressJob = scope.launch {
            val collector = launch {
                progress.latest.collect { update ->
                    if (update != null) {
                        stage = (stage as? Stage.Compressing)
                            ?.copy(progress = update.percent / 100f)
                            ?: stage
                    }
                }
            }
            try {
                val result = PdfEngine.compress(
                    bytes = ready.bytes,
                    password = password,
                    settings = PdfEngine.presetSettings(ready.analysis.recommendedPreset),
                    progress = progress,
                )
                stage = Stage.Done(result, ready.displayName)
            } catch (cancelled: FfiException.Cancelled) {
                stage = Stage.Idle
            } catch (failure: FfiException) {
                when (failure) {
                    is FfiException.PasswordRequired, is FfiException.WrongPassword -> {
                        pendingPassword = ready to (failure is FfiException.WrongPassword)
                        stage = Stage.Ready(
                            ready.uri, ready.bytes, ready.displayName, ready.analysis, password,
                        )
                    }

                    else -> {
                        val mapped = engineErrorMessage(failure, context)
                        error = context.getString(
                            mapped.messageRes,
                            *mapped.formatArgs.toTypedArray(),
                        )
                        stage = Stage.Ready(
                            ready.uri, ready.bytes, ready.displayName, ready.analysis, password,
                        )
                    }
                }
            } finally {
                collector.cancel()
            }
        }
    }

    val pickPdf = rememberLauncherForActivityResult(
        ActivityResultContracts.OpenDocument(),
    ) { uri ->
        if (uri == null) return@rememberLauncherForActivityResult
        scope.launch {
            try {
                val bytes = context.contentResolver.openInputStream(uri)?.use { it.readBytes() }
                    ?: throw IOException("empty stream")
                val displayName = queryDisplayName(context, uri) ?: "document.pdf"
                stage = Stage.Analyzing(uri, bytes, displayName)
                val analysis = PdfEngine.analyze(bytes)
                stage = Stage.Ready(uri, bytes, displayName, analysis, null)
            } catch (failure: Exception) {
                error = failure.message ?: failure.toString()
                stage = Stage.Idle
            }
        }
    }

    val saveOutput = rememberLauncherForActivityResult(
        ActivityResultContracts.CreateDocument("application/pdf"),
    ) { uri ->
        val done = stage as? Stage.Done ?: return@rememberLauncherForActivityResult
        if (uri != null) {
            scope.launch {
                try {
                    context.contentResolver.openOutputStream(uri)?.use { it.write(done.result.bytes) }
                } catch (failure: IOException) {
                    error = failure.message
                }
            }
        }
    }

    Column(
        modifier = Modifier
            .fillMaxSize()
            .padding(24.dp),
        verticalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        Text(stringResource(R.string.app_name), style = MaterialTheme.typography.headlineMedium)
        Text(
            stringResource(R.string.engine_status_ready) + " · " + enginePing,
            style = MaterialTheme.typography.bodySmall,
        )

        error?.let { message ->
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
                AnalysisCard(analysis = current.analysis, fileSizeBytes = current.bytes.size.toULong())
                Button(
                    onClick = { runCompress(current, current.password) },
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    Text(stringResource(R.string.compressing))
                }
            }

            is Stage.Compressing -> {
                Text(stringResource(R.string.compressing))
                LinearProgressIndicator(
                    progress = { current.progress.coerceIn(0f, 1f) },
                    modifier = Modifier.fillMaxWidth(),
                )
                OutlinedButton(
                    onClick = { compressJob?.cancel() },
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
                        onClick = { shareResult(context, current) },
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

    pendingPassword?.let { (ready, wasWrong) ->
        var password by remember { mutableStateOf("") }
        AlertDialog(
            onDismissRequest = { pendingPassword = null },
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
                    pendingPassword = null
                    runCompress(ready, password.ifBlank { null })
                }) { Text("OK") }
            },
            dismissButton = {
                TextButton(onClick = { pendingPassword = null }) {
                    Text(stringResource(R.string.cancel))
                }
            },
        )
    }
}

@Composable
private fun AnalysisCard(analysis: FfiAnalysis, fileSizeBytes: ULong) {
    Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
        Text(
            "${analysis.pageCount} pages · ${analysis.documentKind}",
            style = MaterialTheme.typography.titleMedium,
        )
        Text("images: ${analysis.imageObjectCount} · edge ${analysis.maxImageEdgePx}px")
        Text("estimated savings: ${analysis.estimatedSavingsPercent.toInt()}%")
        Text("recommended preset: ${analysis.recommendedPreset}")
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

private fun queryDisplayName(context: android.content.Context, uri: Uri): String? =
    runCatching {
        context.contentResolver.query(uri, null, null, null, null)?.use { cursor ->
            val index = cursor.getColumnIndex(android.provider.OpenableColumns.DISPLAY_NAME)
            if (index >= 0 && cursor.moveToFirst()) cursor.getString(index) else null
        }
    }.getOrNull()

private fun suggestedOutputName(displayName: String): String =
    displayName.removeSuffix(".pdf") + "_optimized.pdf"

private fun shareResult(context: android.content.Context, done: Stage.Done) {
    // Placeholder intent until FileProvider-backed cache sharing lands with
    // the MVP batch; the chooser opens without the payload attached.
    runCatching {
        val send = Intent(Intent.ACTION_SEND).apply {
            type = "application/pdf"
            putExtra(Intent.EXTRA_SUBJECT, suggestedOutputName(done.displayName))
        }
        context.startActivity(Intent.createChooser(send, null))
    }
}
