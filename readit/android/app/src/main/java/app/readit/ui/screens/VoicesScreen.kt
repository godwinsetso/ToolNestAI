package app.readit.ui.screens

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.CheckCircle
import androidx.compose.material.icons.outlined.CloudOff
import androidx.compose.material.icons.outlined.Wifi
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.readit.tts.DownloadState
import app.readit.tts.VoiceInfo
import app.readit.tts.VoicePack
import app.readit.ui.AppViewModel

@Composable
fun VoicesScreen(vm: AppViewModel) {
    val now by vm.session.now.collectAsStateWithLifecycle()
    val installed by vm.installedVoices.collectAsStateWithLifecycle()
    val system by vm.systemVoices.collectAsStateWithLifecycle()
    val rtf = remember { mutableStateMapOf<String, Double>() }
    var confirmRemove by remember { mutableStateOf<VoicePack?>(null) }
    LaunchedEffect(Unit) {
        vm.refreshVoices()
        vm.voiceManager.catalog.packs.flatMap { it.voices }.forEach { v -> vm.voiceManager.measuredRtf(v.id)?.let { rtf[v.id] = it } }
    }
    val installedIds = installed.map { it.id }.toSet()

    LazyColumn(Modifier.fillMaxSize()) {
        item { ScreenTitle("Voices", "Every voice here is labelled: offline once downloaded, or needs internet.") }

        // Downloaded neural voices, ready to use offline.
        val packs = vm.voiceManager.catalog.packs
        packs.forEach { pack ->
            item(key = "pack-" + pack.id) {
                val state by vm.voiceManager.state(pack.id).collectAsState()
                PackCard(
                    pack, state,
                    onDownload = { vm.downloadPack(pack.id) },
                    onPause = { vm.pauseDownload(pack.id) },
                    onRemove = { confirmRemove = pack },
                )
            }
            if (pack.voices.any { it.id in installedIds }) {
                items(pack.voices, key = { it.id }) { v ->
                    VoiceRow(
                        v,
                        selected = now.voiceId == v.id,
                        rtf = rtf[v.id],
                        onSelect = { vm.session.selectVoice(v.id) },
                        onPreview = { vm.preview(v.id) },
                        onTest = { vm.benchmark(v.id) { r -> rtf[v.id] = r } },
                    )
                }
            }
        }

        item {
            Text("Phone's built-in voices", style = MaterialTheme.typography.titleMedium, modifier = Modifier.padding(start = 20.dp, top = 24.dp))
            Text(
                "Provided by your phone's text-to-speech app. Always available as a fallback. Pitch can be adjusted.",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(horizontal = 20.dp),
            )
        }
        val sys = system
        if (sys == null) {
            item { TextButton(onClick = { vm.loadSystemVoices() }, modifier = Modifier.padding(horizontal = 12.dp)) { Text("Show built-in voices") } }
        } else if (sys.isEmpty()) {
            item { Text("No built-in text-to-speech service was found.", modifier = Modifier.padding(20.dp)) }
        } else {
            items(sys.take(40), key = { "sys-" + it.name }) { v ->
                val id = "system:" + v.name
                Row(Modifier.fillMaxWidth().padding(horizontal = 20.dp, vertical = 6.dp), verticalAlignment = Alignment.CenterVertically) {
                    Icon(if (v.needsNetwork) Icons.Outlined.Wifi else Icons.Outlined.CloudOff, null, tint = MaterialTheme.colorScheme.onSurfaceVariant)
                    Spacer(Modifier.width(12.dp))
                    Column(Modifier.weight(1f)) {
                        Text(v.locale.displayName, style = MaterialTheme.typography.bodyLarge)
                        Text(
                            (if (v.needsNetwork) "Needs internet" else "Offline") + " · " + v.name,
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                    if (now.voiceId == id) Icon(Icons.Filled.CheckCircle, "Selected", tint = MaterialTheme.colorScheme.primary)
                    else TextButton(onClick = { vm.session.selectVoice(id) }) { Text("Use") }
                }
            }
        }

        item {
            Card(
                Modifier.fillMaxWidth().padding(16.dp),
                colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceContainer),
            ) {
                Column(Modifier.padding(16.dp)) {
                    Text("My Voice", style = MaterialTheme.typography.titleMedium)
                    Text(
                        "Personal voices (your own voice, or someone who has agreed to it) are planned for a later version. " +
                            "They will need your explicit consent, will be labelled as synthetic, and can be deleted at any time. " +
                            "This version cannot create a personal voice.",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
        }
    }

    confirmRemove?.let { p ->
        AlertDialog(
            onDismissRequest = { confirmRemove = null },
            title = { Text("Remove ${p.title}?") },
            text = { Text("This frees about ${p.installedBytes / 1_000_000} MB. You can download it again later. Your documents are not affected.") },
            confirmButton = { TextButton(onClick = { vm.removePack(p.id); confirmRemove = null }) { Text("Remove") } },
            dismissButton = { TextButton(onClick = { confirmRemove = null }) { Text("Cancel") } },
        )
    }
}

@Composable
private fun PackCard(pack: VoicePack, state: DownloadState, onDownload: () -> Unit, onPause: () -> Unit, onRemove: () -> Unit) {
    Card(Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp)) {
        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            Text(pack.title, style = MaterialTheme.typography.titleMedium)
            Text(pack.quality, style = MaterialTheme.typography.bodyMedium)
            Text(
                "${pack.voices.size} voice${if (pack.voices.size == 1) "" else "s"} · download ${pack.downloadBytes / 1_000_000} MB · " +
                    "about ${pack.installedBytes / 1_000_000} MB on the phone · licence ${pack.licence}",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Text(pack.requirements, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
            Spacer(Modifier.height(6.dp))
            when (state) {
                DownloadState.NotInstalled -> Button(onClick = onDownload) { Text("Download for offline use") }
                is DownloadState.Downloading -> {
                    LinearProgressIndicator(progress = { state.bytes.toFloat() / state.total }, modifier = Modifier.fillMaxWidth())
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Text("${state.bytes / 1_000_000} of ${state.total / 1_000_000} MB", Modifier.weight(1f), style = MaterialTheme.typography.bodySmall)
                        TextButton(onClick = onPause) { Text("Pause") }
                    }
                }
                is DownloadState.Paused -> Row(verticalAlignment = Alignment.CenterVertically) {
                    Text("Paused at ${state.bytes / 1_000_000} MB", Modifier.weight(1f), style = MaterialTheme.typography.bodySmall)
                    Button(onClick = onDownload) { Text("Resume") }
                    TextButton(onClick = onRemove) { Text("Cancel") }
                }
                DownloadState.Verifying -> Text("Checking the download…", style = MaterialTheme.typography.bodySmall)
                DownloadState.Installing -> Text("Installing…", style = MaterialTheme.typography.bodySmall)
                DownloadState.Installed -> Row(verticalAlignment = Alignment.CenterVertically) {
                    Icon(Icons.Outlined.CloudOff, null, tint = MaterialTheme.colorScheme.primary)
                    Text("  Downloaded: works offline", Modifier.weight(1f), style = MaterialTheme.typography.bodySmall)
                    TextButton(onClick = onRemove) { Text("Remove") }
                }
                is DownloadState.Failed -> Column {
                    Text(state.message, color = MaterialTheme.colorScheme.error, style = MaterialTheme.typography.bodySmall)
                    Button(onClick = onDownload) { Text(if (state.bytes > 0) "Retry (resumes)" else "Retry") }
                }
            }
        }
    }
}

/** Plain-language verdict from the on-device speed test. */
private fun rtfVerdict(rtf: Double): String = when {
    rtf < 0.5 -> "Runs smoothly on this phone"
    rtf < 0.9 -> "Keeps up on this phone"
    else -> "Too slow on this phone: expect pauses between sentences"
}

@Composable
private fun VoiceRow(v: VoiceInfo, selected: Boolean, rtf: Double?, onSelect: () -> Unit, onPreview: () -> Unit, onTest: () -> Unit) {
    var testing by remember { mutableStateOf(false) }
    Row(Modifier.fillMaxWidth().padding(start = 32.dp, end = 16.dp, top = 4.dp, bottom = 4.dp), verticalAlignment = Alignment.CenterVertically) {
        Column(Modifier.weight(1f)) {
            Text(v.name, style = MaterialTheme.typography.bodyLarge)
            Text("${v.accent} · ${v.gender} · offline", style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
            when {
                testing && rtf == null -> Text("Testing speed…", style = MaterialTheme.typography.bodySmall)
                rtf != null -> Text(rtfVerdict(rtf), style = MaterialTheme.typography.bodySmall, color = if (rtf < 0.9) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.error)
            }
        }
        TextButton(onClick = onPreview) { Text("Preview") }
        if (rtf == null) TextButton(onClick = { testing = true; onTest() }) { Text("Test") }
        if (selected) Icon(Icons.Filled.CheckCircle, "Selected", tint = MaterialTheme.colorScheme.primary)
        else OutlinedButton(onClick = onSelect) { Text("Use") }
    }
}
