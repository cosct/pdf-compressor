package com.cosct.pdfcompressor

import android.content.Context
import android.net.Uri
import android.provider.OpenableColumns
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.ensureActive
import kotlinx.coroutines.withContext
import uniffi.pdfcompressor.FfiException
import uniffi.pdfcompressor.maxMobileInputBytes
import java.io.ByteArrayOutputStream
import java.io.InputStream
import java.io.IOException

/** Reads unknown-length providers without allocating the entire stream first. */
internal fun readLimited(input: InputStream, limit: Int, checkCancelled: () -> Unit = {}): ByteArray {
    require(limit > 0)
    val output = ByteArrayOutputStream(minOf(limit, 8192))
    val buffer = ByteArray(8192)
    while (true) {
        checkCancelled()
        val count = input.read(buffer, 0, minOf(buffer.size.toLong(), limit.toLong() - output.size() + 1).toInt())
        if (count < 0) break
        if (count == 0) continue
        if (output.size().toLong() + count > limit) {
            throw FfiException.InputTooLarge((output.size().toLong() + count).toULong(), limit.toULong())
        }
        output.write(buffer, 0, count)
    }
    return output.toByteArray()
}

internal suspend fun readPdf(context: Context, uri: Uri): ByteArray = withContext(Dispatchers.IO) {
    // Leave room for Kotlin growth/copies, FFI buffers and native parsing.
    // This input policy is not a guarantee on native decoder peak RSS.
    val limit = minOf(maxMobileInputBytes().toLong(), Runtime.getRuntime().maxMemory() / 8).coerceAtLeast(1).toInt()
    context.contentResolver.query(uri, arrayOf(OpenableColumns.SIZE), null, null, null)?.use { cursor ->
        if (cursor.moveToFirst() && !cursor.isNull(0) && cursor.getLong(0) > limit) {
            throw FfiException.InputTooLarge(cursor.getLong(0).toULong(), limit.toULong())
        }
    }
    val coroutine = currentCoroutineContext()
    context.contentResolver.openInputStream(uri)?.use { readLimited(it, limit) { coroutine.ensureActive() } }
        ?: throw IOException("Cannot open input document")
}

/** The caller owns this newly created URI. Failure cleans it best-effort;
 * DocumentsProvider does not promise atomic writes or rename support. */
internal suspend fun writeNewPdf(context: Context, uri: Uri, bytes: ByteArray) = withContext(Dispatchers.IO) {
    val coroutine = currentCoroutineContext()
    writeOwnedOutput(
        bytes,
        open = { context.contentResolver.openOutputStream(uri, "wt") },
        delete = { android.provider.DocumentsContract.deleteDocument(context.contentResolver, uri) },
        checkCancelled = { coroutine.ensureActive() },
    )
}

internal fun writeOwnedOutput(
    bytes: ByteArray,
    open: () -> java.io.OutputStream?,
    delete: () -> Unit,
    checkCancelled: () -> Unit = {},
) {
    try {
        open()?.use { output ->
            var offset = 0
            while (offset < bytes.size) {
                checkCancelled()
                val count = minOf(64 * 1024, bytes.size - offset)
                output.write(bytes, offset, count)
                offset += count
            }
        } ?: throw IOException("Cannot open output document")
    } catch (failure: Exception) {
        runCatching { delete() }
        throw failure
    }
}
