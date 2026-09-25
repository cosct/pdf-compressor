package com.cosct.pdfcompressor

import android.content.Context
import android.net.Uri
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.launch
import uniffi.pdfcompressor.FfiAnalysis
import uniffi.pdfcompressor.FfiCompressResult
import uniffi.pdfcompressor.FfiException
import uniffi.pdfcompressor.FfiSettings
import java.io.IOException

/** UI flow stages. Held by [CompressViewModel] so configuration changes
 *  (rotation, theme/locale recreation) do not drop a picked document or a
 *  finished result; the raw bytes stay in memory either way. */
sealed interface Stage {
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

/**
 * Single-file flow state (Phase 3). Outlives the activity across recreation,
 * so an in-flight compression and its result survive rotation and the
 * theme/locale switch recreations. Passwords stay session-memory only.
 */
class CompressViewModel : ViewModel() {

    var stage by mutableStateOf<Stage>(Stage.Idle)
        private set
    var error by mutableStateOf<String?>(null)
        private set
    var pendingPassword by mutableStateOf<Pair<Stage.Ready, Boolean>?>(null)
    // Custom panel overrides; null follows the effective preset's defaults.
    var qualityOverride by mutableStateOf<Int?>(null)
    var maxEdgeOverride by mutableStateOf<Int?>(null)

    // S2: engine liveness probe, plus the preset table straight from the
    // engine's single source of truth.
    val enginePing = PdfEngine.probe()
    val presetTable = PdfEngine.presetTable()

    private var compressJob: Job? = null

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

    fun onPdfPicked(context: Context, uri: Uri) {
        viewModelScope.launch {
            try {
                val bytes = context.contentResolver.openInputStream(uri)?.use { it.readBytes() }
                    ?: throw IOException("empty stream")
                val displayName = queryDisplayName(context, uri) ?: "document.pdf"
                // Fresh document: the custom panel follows its own analysis.
                qualityOverride = null
                maxEdgeOverride = null
                stage = Stage.Analyzing(uri, bytes, displayName)
                val analysis = PdfEngine.analyze(bytes)
                stage = Stage.Ready(uri, bytes, displayName, analysis, null)
            } catch (failure: Exception) {
                error = failure.message ?: failure.toString()
                stage = Stage.Idle
            }
        }
    }

    fun runCompress(context: Context, ready: Stage.Ready, password: String?, presetChoice: String) {
        val progress = PdfEngine.Progress()
        stage = Stage.Compressing(
            ready.uri, ready.bytes, ready.displayName, ready.analysis, password, 0f,
        )
        compressJob = viewModelScope.launch {
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
                    settings = currentSettings(ready.analysis, presetChoice),
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

    fun cancelCompress() {
        compressJob?.cancel()
    }

    /** Non-engine failures (SAF I/O) surface through the same error slot. */
    fun reportError(message: String?) {
        error = message
    }

    fun dismissPasswordDialog() {
        pendingPassword = null
    }
}
