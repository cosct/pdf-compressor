package com.cosct.pdfcompressor

import android.content.Context
import android.content.Intent
import android.net.Uri
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlinx.coroutines.Job
import kotlinx.coroutines.launch
import uniffi.pdfcompressor.FfiAnalysis
import uniffi.pdfcompressor.FfiCompressResult
import uniffi.pdfcompressor.FfiException
import uniffi.pdfcompressor.FfiSettings

/** UI flow stages. Held by [CompressViewModel] so configuration changes
 *  (rotation, theme/locale recreation) do not drop a picked document or a
 *  finished result; the raw bytes stay in memory either way. */
sealed interface Stage {
    data object Idle : Stage
    data object Reading : Stage

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

/**
 * Single-file flow state (Phase 3). Outlives the activity across recreation,
 * so an in-flight compression and its result survive rotation and the
 * theme/locale switch recreations. Passwords stay session-memory only.
 */
class CompressViewModel(private val engine: PdfEngineApi = PdfEngine) : ViewModel() {

    var stage by mutableStateOf<Stage>(Stage.Idle)
        private set
    var error by mutableStateOf<String?>(null)
        private set
    var pendingPassword by mutableStateOf<Pair<Stage.Analyzing, Boolean>?>(null)
    // Custom panel overrides; null follows the effective preset's defaults.
    var qualityOverride by mutableStateOf<Int?>(null)
    var maxEdgeOverride by mutableStateOf<Int?>(null)
    /** Target-size search budget in MiB (Phase 5); null runs the plain
     *  quality-driven path. The chosen preset stays the search's start. */
    var targetSizeMb by mutableStateOf<Int?>(null)

    // S2: engine liveness probe, plus the preset table straight from the
    // engine's single source of truth.
    val enginePing = engine.probe()
    val presetTable = engine.presetTable()

    private var compressJob: Job? = null
    private var generation = 0

    private fun beginTask(): Int {
        generation += 1
        compressJob?.cancel()
        pendingPassword = null
        error = null
        return generation
    }

    fun effectivePreset(analysis: FfiAnalysis, presetChoice: String): String =
        if (presetChoice == "auto" || presetChoice !in presetTable) {
            analysis.recommendedPreset
        } else {
            presetChoice
        }

    fun effectiveQuality(analysis: FfiAnalysis, presetChoice: String): Int =
        qualityOverride
            ?: presetTable[effectivePreset(analysis, presetChoice)]?.imageQuality?.toInt()
            ?: 60

    fun currentSettings(analysis: FfiAnalysis, presetChoice: String): FfiSettings =
        PdfEngine.presetSettings(effectivePreset(analysis, presetChoice)).copy(
            imageQuality = effectiveQuality(analysis, presetChoice).toUByte(),
            maxImageSizePx = maxEdgeOverride?.toUShort(),
        )

    fun onPdfPicked(
        context: Context,
        uri: Uri,
        name: String? = null,
        persistable: Boolean = true,
        onFailure: (() -> Unit)? = null,
    ) {
        val run = beginTask()
        stage = Stage.Reading
        compressJob = viewModelScope.launch {
            try {
                val input = withContext(Dispatchers.IO) {
                    if (persistable) {
                        runCatching { context.contentResolver.takePersistableUriPermission(uri, Intent.FLAG_GRANT_READ_URI_PERMISSION) }
                    }
                    val bytes = readPdf(context, uri)
                    val displayName = name ?: queryDisplayName(context, uri) ?: "document.pdf"
                    Stage.Analyzing(uri, bytes, displayName)
                }
                if (run != generation) return@launch
                qualityOverride = null
                maxEdgeOverride = null
                targetSizeMb = null
                analyzeInput(context, input, null, run)
            } catch (cancelled: CancellationException) {
                if (run == generation) stage = Stage.Idle
                throw cancelled
            } catch (failure: Exception) {
                if (run == generation) {
                    error = failure.message ?: failure.toString()
                    stage = Stage.Idle
                    onFailure?.invoke()
                }
            }
        }
    }

    fun retryAnalysis(context: Context, input: Stage.Analyzing, password: String?) {
        val run = beginTask()
        compressJob = viewModelScope.launch { analyzeInput(context, input, password, run) }
    }

    private suspend fun analyzeInput(context: Context, input: Stage.Analyzing, password: String?, run: Int) {
        stage = input
        try {
            val analysis = engine.analyze(input.bytes, password)
            if (run == generation) stage = Stage.Ready(input.uri, input.bytes, input.displayName, analysis, password)
        } catch (cancelled: CancellationException) {
            if (run == generation) stage = Stage.Idle
            throw cancelled
        } catch (failure: FfiException) {
            if (run != generation) return
            stage = Stage.Idle
            if (failure is FfiException.PasswordRequired || failure is FfiException.WrongPassword) {
                pendingPassword = input to (failure is FfiException.WrongPassword)
            } else {
                val mapped = engineErrorMessage(failure, context)
                error = context.getString(mapped.messageRes, *mapped.formatArgs.toTypedArray())
            }
        }
    }

    fun runCompress(context: Context, ready: Stage.Ready, password: String?, presetChoice: String) {
        val run = beginTask()
        val progress = PdfEngine.Progress()
        stage = Stage.Compressing(
            ready.uri, ready.bytes, ready.displayName, ready.analysis, password, 0f,
        )
        compressJob = viewModelScope.launch {
            val collector = launch {
                progress.latest.collect { update ->
                    if (update != null && run == generation) {
                        stage = (stage as? Stage.Compressing)
                            ?.copy(progress = update.percent / 100f)
                            ?: stage
                    }
                }
            }
            try {
                val settings = currentSettings(ready.analysis, presetChoice)
                val target = targetSizeMb
                val result = if (target != null) {
                    engine.compressToTarget(
                        bytes = ready.bytes,
                        targetBytes = (target.toLong() shl 20).toULong(),
                        password = password,
                        settings = settings,
                        progress = progress,
                    )
                } else {
                    engine.compress(
                        bytes = ready.bytes,
                        password = password,
                        settings = settings,
                        progress = progress,
                    )
                }
                if (run == generation) stage = Stage.Done(result, ready.displayName)
            } catch (cancelled: CancellationException) {
                if (run == generation) stage = ready
                throw cancelled
            } catch (cancelled: FfiException.Cancelled) {
                if (run == generation) stage = ready
            } catch (failure: FfiException) {
                if (run != generation) return@launch
                when (failure) {
                    is FfiException.PasswordRequired, is FfiException.WrongPassword -> {
                        pendingPassword = Stage.Analyzing(ready.uri, ready.bytes, ready.displayName) to (failure is FfiException.WrongPassword)
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

    fun cancelCompress() {
        compressJob?.cancel()
    }

    /** Back from a finished run to the idle screen (recents live there). */
    fun resetToIdle() {
        beginTask()
        stage = Stage.Idle
    }

    /** Non-engine failures (SAF I/O) surface through the same error slot. */
    fun reportError(message: String?) {
        error = message
    }

    fun dismissPasswordDialog() {
        pendingPassword = null
    }
}
