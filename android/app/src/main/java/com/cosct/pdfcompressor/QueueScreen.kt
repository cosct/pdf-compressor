package com.cosct.pdfcompressor

import android.Manifest
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Button
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.core.net.toUri
import androidx.documentfile.provider.DocumentFile
import androidx.work.WorkInfo
import androidx.work.WorkManager
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.launch
import androidx.compose.runtime.rememberCoroutineScope

/**
 * Batch queue (Phase 4): multi-pick PDFs, compress them one by one under a
 * foreground-service notification, write optimized copies into the chosen
 * SAF tree. WorkManager persists the chain, so a killed process resumes at
 * the interrupted item. Encrypted inputs fail per item — passwords never
 * leave the interactive single-file flow.
 */
@Composable
fun QueueScreen(
    settings: SettingsSnapshot,
    settingsRepository: SettingsRepository,
) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val workManager = WorkManager.getInstance(context)
    // Null until Room's first real emission — retain-cleanup below must not
    // fire for the placeholder or it would wipe fresh metadata on startup.
    val workInfos by workManager.getWorkInfosByTagFlow(QueueWorker.TAG_ITEM)
        .collectAsState(initial = null)
    val metas by QueueMetaStore.metas(context).collectAsState(initial = emptyMap())
    val items = (workInfos ?: emptyList())
        .sortedBy { metas[it.id]?.enqueuedAt ?: Long.MAX_VALUE }
    LaunchedEffect(workInfos) {
        val live = workInfos ?: return@LaunchedEffect
        QueueMetaStore.retain(context, live.map { it.id }.toSet())
    }

    val pickTree = rememberLauncherForActivityResult(
        ActivityResultContracts.OpenDocumentTree(),
    ) { uri ->
        if (uri != null) {
            runCatching {
                context.contentResolver.takePersistableUriPermission(
                    uri,
                    Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION,
                )
            }
            scope.launch { settingsRepository.setOutputTree(uri.toString()) }
        }
    }

    fun enqueue(uris: List<Uri>) {
        val tree = settings.outputTree?.toUri() ?: return
        uris.forEach { uri ->
            runCatching {
                context.contentResolver.takePersistableUriPermission(
                    uri,
                    Intent.FLAG_GRANT_READ_URI_PERMISSION,
                )
            }
        }
        scope.launch { QueueWorker.enqueueBatch(context, uris, tree, settings.presetChoice) }
    }

    // API 33+: the progress notification needs a runtime grant. The chain
    // runs either way — denied just means no visible notification.
    val requestNotifications = rememberLauncherForActivityResult(
        ActivityResultContracts.RequestPermission(),
    ) { }
    val pickPdfs = rememberLauncherForActivityResult(
        ActivityResultContracts.OpenMultipleDocuments(),
    ) { uris ->
        if (uris.isNotEmpty()) {
            if (Build.VERSION.SDK_INT >= 33 &&
                context.checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) !=
                PackageManager.PERMISSION_GRANTED
            ) {
                requestNotifications.launch(Manifest.permission.POST_NOTIFICATIONS)
            }
            enqueue(uris)
        }
    }

    Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
        val treeName = settings.outputTree
            ?.let { runCatching { DocumentFile.fromTreeUri(context, it.toUri())?.name }.getOrNull() }
        OutlinedButton(
            onClick = { pickTree.launch(null) },
            modifier = Modifier.fillMaxWidth(),
        ) {
            Text(
                if (treeName != null) {
                    stringResource(R.string.queue_output_set, treeName)
                } else {
                    stringResource(R.string.queue_pick_output)
                },
            )
        }

        Button(
            onClick = { pickPdfs.launch(arrayOf("application/pdf")) },
            enabled = settings.outputTree != null,
            modifier = Modifier.fillMaxWidth(),
        ) {
            Text(stringResource(R.string.queue_enqueue))
        }

        if (items.isEmpty()) {
            Text(
                stringResource(R.string.queue_empty),
                style = MaterialTheme.typography.bodySmall,
            )
        } else {
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                TextButton(onClick = { workManager.cancelUniqueWork(QueueWorker.QUEUE_CHAIN) }) {
                    Text(stringResource(R.string.queue_cancel_all))
                }
                TextButton(onClick = { workManager.pruneWork() }) {
                    Text(stringResource(R.string.queue_clear_finished))
                }
            }
        }

        items.forEach { info ->
            QueueItemRow(context, info, metas[info.id]?.name ?: "document.pdf")
            HorizontalDivider()
        }
    }
}

@Composable
private fun QueueItemRow(context: Context, info: WorkInfo, name: String) {
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .padding(vertical = 4.dp),
        verticalArrangement = Arrangement.spacedBy(4.dp),
    ) {
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(name, style = MaterialTheme.typography.bodyLarge)
            QueueItemStatus(context, info)
        }
        if (info.state == WorkInfo.State.RUNNING) {
            val percent = info.progress.getInt(QueueWorker.OUT_PERCENT, 0)
            LinearProgressIndicator(
                progress = { (percent / 100f).coerceIn(0f, 1f) },
                modifier = Modifier.fillMaxWidth(),
            )
        }
    }
}

@Composable
private fun QueueItemStatus(context: Context, info: WorkInfo) {
    when (info.state) {
        WorkInfo.State.ENQUEUED, WorkInfo.State.BLOCKED ->
            StatusText(stringResource(R.string.queue_item_waiting))

        WorkInfo.State.RUNNING -> {
            val percent = info.progress.getInt(QueueWorker.OUT_PERCENT, 0)
            StatusText(stringResource(R.string.queue_item_running, percent))
        }

        WorkInfo.State.SUCCEEDED -> {
            val error = info.outputData.getString(QueueWorker.OUT_ERROR)
            when {
                !error.isNullOrEmpty() ->
                    StatusText(stringResource(R.string.queue_item_failed, error), isError = true)

                info.outputData.getBoolean(QueueWorker.OUT_NOT_SMALLER, false) ->
                    StatusText(stringResource(R.string.queue_item_not_smaller))

                else -> {
                    val saved = info.outputData.getLong(QueueWorker.OUT_SAVED, 0L)
                    val human = android.text.format.Formatter.formatShortFileSize(context, saved)
                    StatusText(stringResource(R.string.queue_item_done, human))
                }
            }
        }

        WorkInfo.State.FAILED ->
            StatusText(stringResource(R.string.queue_item_failed, "worker error"), isError = true)

        WorkInfo.State.CANCELLED ->
            StatusText(stringResource(R.string.queue_item_cancelled))
    }
}

@Composable
private fun StatusText(text: String, isError: Boolean = false) {
    Text(
        text,
        style = MaterialTheme.typography.bodySmall,
        color = if (isError) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.onSurfaceVariant,
    )
}
