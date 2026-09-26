package com.cosct.pdfcompressor

import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.net.Uri
import androidx.core.app.NotificationCompat
import androidx.core.net.toUri
import androidx.datastore.preferences.core.edit
import androidx.datastore.preferences.core.stringPreferencesKey
import androidx.datastore.preferences.preferencesDataStore
import androidx.documentfile.provider.DocumentFile
import androidx.work.CoroutineWorker
import androidx.work.Data
import androidx.work.ExistingWorkPolicy
import androidx.work.ForegroundInfo
import androidx.work.OneTimeWorkRequest
import androidx.work.OneTimeWorkRequestBuilder
import androidx.work.WorkManager
import androidx.work.WorkerParameters
import androidx.work.workDataOf
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.launch
import kotlinx.coroutines.coroutineScope
import uniffi.pdfcompressor.FfiException
import java.io.IOException
import java.util.UUID

/**
 * One queue item = one work request; the batch is a sequential unique-work
 * chain ([QUEUE_CHAIN]), so a killed process resumes at the interrupted item
 * through WorkManager's retry semantics. A per-item engine failure must NOT
 * return [Result.failure] — that would fail the whole chain — it is folded
 * into a success payload ([OUT_ERROR]) and the chain moves on. Passwords are
 * never persisted, so encrypted inputs fail here by design; the single-file
 * flow handles them interactively.
 */
class QueueWorker(
    context: Context,
    params: WorkerParameters,
) : CoroutineWorker(context, params) {

    companion object {
        const val QUEUE_CHAIN = "pdf-compress-queue"
        const val TAG_ITEM = "pdf-queue-item"
        const val KEY_URI = "uri"
        const val KEY_TREE = "tree"
        const val KEY_PRESET = "preset"
        const val OUT_PERCENT = "percent"
        const val OUT_SAVED = "saved"
        const val OUT_OUTPUT_URI = "outputUri"
        const val OUT_NOT_SMALLER = "notSmaller"
        const val OUT_ERROR = "error"
        private const val CHANNEL_ID = "queue"
        private const val NOTIFICATION_ID = 42

        /** Enqueue a batch and record its display metadata. Suspends because
         *  WorkInfo exposes no input data — names/order live in
         *  [QueueMetaStore] (both sides survive process death). */
        suspend fun enqueueBatch(context: Context, uris: List<Uri>, tree: Uri, presetChoice: String) {
            val batch = System.currentTimeMillis()
            val requests: List<OneTimeWorkRequest> = uris.map { uri ->
                OneTimeWorkRequestBuilder<QueueWorker>()
                    .setInputData(
                        workDataOf(
                            KEY_URI to uri.toString(),
                            KEY_TREE to tree.toString(),
                            KEY_PRESET to presetChoice,
                        ),
                    )
                    .addTag(TAG_ITEM)
                    .build()
            }
            QueueMetaStore.add(
                context,
                uris.mapIndexed { index, uri ->
                    QueueItemMeta(
                        id = requests[index].id,
                        name = queryDisplayName(context, uri) ?: "document.pdf",
                        enqueuedAt = batch + index,
                    )
                },
            )
            WorkManager.getInstance(context)
                .beginUniqueWork(QUEUE_CHAIN, ExistingWorkPolicy.APPEND_OR_REPLACE, requests)
                .enqueue()
        }
    }

    override suspend fun doWork(): Result {
        val uri = inputData.getString(KEY_URI)?.toUri()
            ?: return Result.failure()
        val tree = inputData.getString(KEY_TREE)?.toUri()
            ?: return Result.failure()
        val displayName = queryDisplayName(applicationContext, uri) ?: "document.pdf"
        val presetChoice = inputData.getString(KEY_PRESET) ?: "auto"

        setForeground(foregroundInfo(displayName, 0))

        try {
            val bytes = applicationContext.contentResolver
                .openInputStream(uri)?.use { it.readBytes() }
                ?: throw IOException("empty stream")

            // Auto resolves through the document's own analysis, same as the
            // single-file flow; an explicit preset skips the analysis pass.
            val preset = if (presetChoice == "auto") {
                PdfEngine.analyze(bytes).recommendedPreset
            } else {
                presetChoice
            }
            val progress = PdfEngine.Progress()
            val result = coroutineScope {
                val reporter = launch {
                    progress.latest.collect { update ->
                        if (update != null) {
                            val percent = update.percent.toInt()
                            setProgress(workDataOf(OUT_PERCENT to percent))
                            setForeground(foregroundInfo(displayName, percent))
                        }
                    }
                }
                try {
                    PdfEngine.compress(
                        bytes = bytes,
                        password = null,
                        settings = PdfEngine.presetSettings(preset),
                        progress = progress,
                    )
                } finally {
                    reporter.cancel()
                }
            }

            val output = Data.Builder().putInt(OUT_PERCENT, 100)
            if (!result.outputWasSmaller) {
                output.putBoolean(OUT_NOT_SMALLER, true)
            } else {
                val written = writeToTree(tree, displayName, result.bytes)
                output.putString(OUT_OUTPUT_URI, written.toString())
                output.putLong(OUT_SAVED, result.savedBytes.toLong())
            }
            return Result.success(output.build())
        } catch (cancelled: FfiException.Cancelled) {
            return Result.success(
                workDataOf(OUT_PERCENT to 100, OUT_ERROR to "cancelled"),
            )
        } catch (failure: FfiException) {
            val mapped = engineErrorMessage(failure, applicationContext)
            val message = applicationContext.getString(
                mapped.messageRes,
                *mapped.formatArgs.toTypedArray(),
            )
            return Result.success(
                workDataOf(OUT_PERCENT to 100, OUT_ERROR to message),
            )
        } catch (io: IOException) {
            return Result.success(
                workDataOf(OUT_PERCENT to 100, OUT_ERROR to (io.message ?: "I/O error")),
            )
        }
    }

    /** Write the optimized copy next to the user's chosen output tree. The
     *  DocumentsProvider dedupes name conflicts itself (`name (1).pdf`), so
     *  a half-written temp file can never linger — the write is atomic at
     *  the provider level, matching the desktop's atomic-output discipline. */
    private fun writeToTree(tree: Uri, displayName: String, bytes: ByteArray): Uri {
        val dir = DocumentFile.fromTreeUri(applicationContext, tree)
            ?: throw IOException("output tree not accessible")
        val base = displayName.removeSuffix(".pdf") + "_optimized"
        val doc = dir.createFile("application/pdf", base)
            ?: throw IOException("cannot create output document")
        applicationContext.contentResolver.openOutputStream(doc.uri, "wt")?.use {
            it.write(bytes)
        } ?: throw IOException("cannot open output stream")
        return doc.uri
    }

    private fun foregroundInfo(displayName: String, percent: Int): ForegroundInfo {
        val manager = applicationContext.getSystemService(NotificationManager::class.java)
        if (manager.getNotificationChannel(CHANNEL_ID) == null) {
            manager.createNotificationChannel(
                NotificationChannel(
                    CHANNEL_ID,
                    applicationContext.getString(R.string.queue_notification_channel),
                    NotificationManager.IMPORTANCE_LOW,
                ),
            )
        }
        val openApp = PendingIntent.getActivity(
            applicationContext,
            0,
            Intent(applicationContext, MainActivity::class.java),
            PendingIntent.FLAG_IMMUTABLE,
        )
        val notification = NotificationCompat.Builder(applicationContext, CHANNEL_ID)
            .setSmallIcon(R.mipmap.ic_launcher)
            .setContentTitle(applicationContext.getString(R.string.queue_notification_title))
            .setContentText("$displayName · $percent%")
            .setProgress(100, percent, false)
            .setOngoing(true)
            .setContentIntent(openApp)
            .build()
        return ForegroundInfo(
            NOTIFICATION_ID,
            notification,
            ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC,
        )
    }
}


/** Display metadata for one queued item. WorkInfo deliberately exposes no
 *  input data, so names and insertion order live here, persisted alongside
 *  WorkManager's own DB — both survive process death. */
data class QueueItemMeta(
    val id: UUID,
    val name: String,
    val enqueuedAt: Long,
)

private val Context.queueMetaDataStore by preferencesDataStore(name = "queue_meta")

object QueueMetaStore {

    private val KEY = stringPreferencesKey("items")

    private fun encode(list: List<QueueItemMeta>): String =
        list.joinToString("\n") { meta ->
            "${meta.id}|${meta.enqueuedAt}|${meta.name.replace("|", "/")}"
        }

    private fun decode(raw: String): List<QueueItemMeta> =
        raw.lines().mapNotNull { line ->
            val parts = line.split("|", limit = 3)
            if (parts.size == 3) {
                runCatching {
                    QueueItemMeta(UUID.fromString(parts[0]), parts[2], parts[1].toLong())
                }.getOrNull()
            } else {
                null
            }
        }

    fun metas(context: Context): Flow<Map<UUID, QueueItemMeta>> =
        context.queueMetaDataStore.data.map { prefs ->
            decode(prefs[KEY] ?: "").associateBy { it.id }
        }

    suspend fun add(context: Context, items: List<QueueItemMeta>) {
        context.queueMetaDataStore.edit { prefs ->
            prefs[KEY] = encode(decode(prefs[KEY] ?: "") + items)
        }
    }

    /** Drop entries whose work is gone (pruned/cancelled queue), so the
     *  store tracks WorkManager's DB instead of growing forever. */
    suspend fun retain(context: Context, liveIds: Set<UUID>) {
        context.queueMetaDataStore.edit { prefs ->
            prefs[KEY] = encode(decode(prefs[KEY] ?: "").filter { it.id in liveIds })
        }
    }
}
