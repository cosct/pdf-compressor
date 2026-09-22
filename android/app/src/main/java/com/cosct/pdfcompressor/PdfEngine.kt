package com.cosct.pdfcompressor

import android.content.Context
import androidx.annotation.StringRes
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.suspendCancellableCoroutine
import kotlinx.coroutines.isActive
import java.util.concurrent.ExecutorService
import java.util.concurrent.Executors
import kotlin.coroutines.resume
import uniffi.pdfcompressor.FfiAnalysis
import uniffi.pdfcompressor.FfiCancelHandle
import uniffi.pdfcompressor.FfiCompressResult
import uniffi.pdfcompressor.FfiException
import uniffi.pdfcompressor.FfiProgress
import uniffi.pdfcompressor.FfiProgressUpdate
import uniffi.pdfcompressor.FfiSettings
import uniffi.pdfcompressor.analyze as nativeAnalyze
import uniffi.pdfcompressor.compress as nativeCompress
import uniffi.pdfcompressor.compressToTarget as nativeCompressToTarget
import uniffi.pdfcompressor.ping as nativePing
import uniffi.pdfcompressor.presetDefaults as nativePresetDefaults
import uniffi.pdfcompressor.uniffiEnsureInitialized

/**
 * Coroutine facade over the blocking UniFFI entries (plan decision: blocking
 * Rust API + Kotlin coroutine wrapper, no UniFFI async).
 *
 * Native calls run serialized on one background thread; coroutine
 * cancellation is wired to the engine's cooperative cancel handle via
 * [kotlinx.coroutines.suspendCancellableCoroutine.invokeOnCancellation],
 * so cancelling the scope aborts the run at the engine's next check.
 */
object PdfEngine {

    /** Progress sink handed to the engine; thread-safe via StateFlow. */
    class Progress : FfiProgress {
        val latest = MutableStateFlow<FfiProgressUpdate?>(null)
        override fun onProgress(update: FfiProgressUpdate) {
            latest.value = update
        }
    }

    private val executor: ExecutorService =
        Executors.newSingleThreadExecutor { runnable ->
            Thread(runnable, "pdf-engine").apply { priority = Thread.NORM_PRIORITY - 1 }
        }

    /** Liveness probe (spike S2): shows on the skeleton screen. */
    fun probe(): String {
        ensureLoaded()
        return nativePing()
    }

    /** Preset table read from the engine's single source of truth. */
    fun presetTable() = nativePresetDefaults().associateBy { it.name }

    /** A settings payload selecting a preset and nothing else (the
     *  generated FfiSettings constructor has no default arguments). */
    fun presetSettings(preset: String): FfiSettings = FfiSettings(
        preset = preset,
        imageQuality = null,
        maxImageSizePx = null,
        maxImageSizePercent = null,
        optimizeImages = null,
        compressStreams = null,
        stripMetadata = null,
        grayscale = null,
        bilevelCodec = null,
        subsetFonts = null,
        cmykConversion = null,
    )

    suspend fun analyze(
        bytes: ByteArray,
        password: String? = null,
        progress: Progress = Progress(),
    ): FfiAnalysis = native(progress) { cancel, cb ->
        nativeAnalyze(bytes, password, null, cancel, cb)
    }

    suspend fun compress(
        bytes: ByteArray,
        password: String? = null,
        settings: FfiSettings,
        progress: Progress = Progress(),
    ): FfiCompressResult = native(progress) { cancel, cb ->
        nativeCompress(bytes, password, settings, cancel, cb)
    }

    suspend fun compressToTarget(
        bytes: ByteArray,
        targetBytes: ULong,
        password: String? = null,
        settings: FfiSettings,
        progress: Progress = Progress(),
    ): FfiCompressResult = native(progress) { cancel, cb ->
        nativeCompressToTarget(bytes, password, targetBytes, settings, cancel, cb)
    }

    private fun ensureLoaded() {
        // Loads libpdf_core_ffi.so through JNA and checks the UniFFI
        // contract/checksums against the committed bindings.
        uniffiEnsureInitialized()
    }

    private suspend fun <T> native(
        progress: Progress,
        block: (FfiCancelHandle, FfiProgress) -> T,
    ): T = suspendCancellableCoroutine { continuation ->
        ensureLoaded()
        val handle = FfiCancelHandle()
        executor.execute {
            try {
                val result = block(handle, progress)
                if (continuation.isActive) continuation.resume(result)
            } catch (throwable: Throwable) {
                if (continuation.isActive) continuation.resumeWithException(throwable)
            }
        }
        continuation.invokeOnCancellation { handle.cancel() }
    }
}

/** A resolved strings.xml entry for an engine failure. */
data class EngineError(
    @StringRes val messageRes: Int,
    val formatArgs: List<String> = emptyList(),
)

/** Map the typed ffi exception onto the shared strings.xml error set. */
fun engineErrorMessage(error: FfiException, context: Context): EngineError {
    fun human(bytes: ULong): String =
        android.text.format.Formatter.formatShortFileSize(context, bytes.toLong())
    return when (error) {
        is FfiException.MissingInput -> EngineError(R.string.error_missingInput)
        is FfiException.InvalidPdf -> EngineError(R.string.error_invalidPdfPath)
        is FfiException.InputTooLarge -> EngineError(
            R.string.error_inputTooLarge,
            listOf(human(error.sizeBytes), human(error.limitBytes)),
        )
        is FfiException.EncryptedPdf -> EngineError(R.string.error_encryptedPdf)
        is FfiException.PasswordRequired -> EngineError(R.string.error_passwordRequired)
        is FfiException.WrongPassword -> EngineError(R.string.error_wrongPassword)
        is FfiException.Cancelled -> EngineError(R.string.error_cancelled)
        is FfiException.Engine -> EngineError(
            R.string.error_pdfBuild,
            listOf(error.detail),
        )
    }
}
