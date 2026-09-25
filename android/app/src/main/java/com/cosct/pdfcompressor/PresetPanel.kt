package com.cosct.pdfcompressor

import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.rememberScrollState
import androidx.compose.material3.FilterChip
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Slider
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import uniffi.pdfcompressor.FfiPresetProfile
import kotlin.math.roundToInt

@Composable
fun presetDisplayName(name: String): String = when (name) {
    "conservative" -> stringResource(R.string.preset_conservative)
    "balanced" -> stringResource(R.string.preset_balanced)
    "maximum" -> stringResource(R.string.preset_maximum)
    else -> name
}

/**
 * Preset chips (Auto + the engine's three rows) plus the simplified custom
 * panel — image quality and an absolute edge cap, the two levers the plan
 * exposes on mobile. The table itself comes from `preset_defaults()`, the
 * single source of truth.
 */
@Composable
fun PresetPanel(
    table: Map<String, FfiPresetProfile>,
    recommended: String,
    choice: String,
    onChoice: (String) -> Unit,
    quality: Int,
    onQuality: (Int) -> Unit,
    maxEdgePx: Int?,
    onMaxEdge: (Int?) -> Unit,
) {
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Text(
            stringResource(R.string.preset_group_label),
            style = MaterialTheme.typography.titleSmall,
        )
        Row(
            horizontalArrangement = Arrangement.spacedBy(8.dp),
            modifier = Modifier.horizontalScroll(rememberScrollState()),
        ) {
            FilterChip(
                selected = choice == "auto",
                onClick = { onChoice("auto") },
                label = {
                    val auto = stringResource(R.string.preset_auto)
                    val badge = stringResource(R.string.preset_recommended_badge)
                    Text("$auto · $badge")
                },
            )
            table.values.forEach { profile ->
                FilterChip(
                    selected = choice == profile.name,
                    onClick = { onChoice(profile.name) },
                    label = {
                        val label = presetDisplayName(profile.name)
                        val badge = stringResource(R.string.preset_recommended_badge)
                        val suffix = if (profile.name == recommended) " · $badge" else ""
                        Text("$label$suffix")
                    },
                )
            }
        }

        val effective = table[choice] ?: table[recommended]
        if (effective != null) {
            Text(
                "Q${effective.imageQuality.toInt()} · ≤${effective.maxImageSizePercent.toInt()}%",
                style = MaterialTheme.typography.bodySmall,
            )
        }

        Text(stringResource(R.string.custom_quality_label, quality))
        Slider(
            value = quality.toFloat(),
            onValueChange = { onQuality(it.roundToInt()) },
            valueRange = 30f..95f,
            steps = 12,
            modifier = Modifier.fillMaxWidth(),
        )

        Text(
            stringResource(R.string.custom_max_edge_label),
            style = MaterialTheme.typography.titleSmall,
        )
        Row(
            horizontalArrangement = Arrangement.spacedBy(8.dp),
            modifier = Modifier.horizontalScroll(rememberScrollState()),
        ) {
            listOf<Int?>(null, 2048, 1600, 1024).forEach { option ->
                FilterChip(
                    selected = maxEdgePx == option,
                    onClick = { onMaxEdge(option) },
                    label = {
                        Text(
                            option?.let { "$it px" }
                                ?: stringResource(R.string.max_edge_follow_preset),
                        )
                    },
                )
            }
        }
    }
}
