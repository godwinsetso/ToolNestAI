package app.readit.ui.screens

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Pause
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.material.icons.outlined.FileOpen
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.FilledIconButton
import androidx.compose.material3.Icon
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedCard
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.readit.ui.AppViewModel

@Composable
fun HomeScreen(vm: AppViewModel, onOpen: (String) -> Unit, onNavigate: (String) -> Unit) {
    val recent by vm.recent.collectAsStateWithLifecycle()
    val all by vm.documents.collectAsStateWithLifecycle()
    val now by vm.session.now.collectAsStateWithLifecycle()
    val importing by vm.importing.collectAsStateWithLifecycle()
    val picker = rememberImportLauncher { vm.import(it) { d -> onOpen(d.id) } }

    LazyColumn(Modifier.fillMaxSize()) {
        item { ScreenTitle("READIT", "Your documents, read aloud. Private and on this device.") }

        // Continue listening: one tap back to where you were.
        now.document?.let { d ->
            item {
                Card(
                    Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp).clickable { onNavigate("listen") },
                    colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.primaryContainer),
                ) {
                    Row(Modifier.padding(16.dp), verticalAlignment = Alignment.CenterVertically) {
                        Column(Modifier.weight(1f)) {
                            Text("Continue listening", style = MaterialTheme.typography.labelLarge)
                            Text(d.title, style = MaterialTheme.typography.titleMedium, maxLines = 2, overflow = TextOverflow.Ellipsis)
                            Text(now.locationLabel, style = MaterialTheme.typography.bodySmall)
                        }
                        FilledIconButton(
                            onClick = { vm.session.toggle() },
                            modifier = Modifier.size(56.dp).semantics { contentDescription = if (now.isPlaying) "Pause" else "Play" },
                        ) { Icon(if (now.isPlaying) Icons.Filled.Pause else Icons.Filled.PlayArrow, null) }
                    }
                }
            }
        }

        item {
            OutlinedCard(
                Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp).clickable { picker.launch(IMPORT_MIME_TYPES) },
                shape = RoundedCornerShape(16.dp),
            ) {
                Row(Modifier.padding(20.dp), verticalAlignment = Alignment.CenterVertically) {
                    Icon(Icons.Outlined.FileOpen, null, tint = MaterialTheme.colorScheme.primary, modifier = Modifier.size(32.dp))
                    Spacer(Modifier.width(16.dp))
                    Column {
                        Text("Open a document", style = MaterialTheme.typography.titleMedium)
                        Text(
                            "PDF, Word, PowerPoint, EPUB, RTF, web pages, Markdown or text",
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                }
            }
        }

        importing?.let { p ->
            item {
                Column(Modifier.padding(horizontal = 20.dp, vertical = 8.dp)) {
                    Text(
                        "Reading ${p.name}" + if (p.total > 1) " (${p.index} of ${p.total})" else "",
                        style = MaterialTheme.typography.bodyMedium,
                    )
                    Spacer(Modifier.height(6.dp))
                    LinearProgressIndicator(Modifier.fillMaxWidth())
                }
            }
        }

        if (recent.isNotEmpty()) {
            item { Text("Recent", style = MaterialTheme.typography.titleMedium, modifier = Modifier.padding(start = 20.dp, top = 16.dp, bottom = 4.dp)) }
            items(recent, key = { it.id }) { d -> DocumentRow(d, onClick = { onOpen(d.id) }) }
        } else if (all.isEmpty() && importing == null) {
            item {
                Column(Modifier.fillMaxWidth().padding(32.dp), horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(8.dp)) {
                    Text("Nothing here yet", style = MaterialTheme.typography.titleMedium)
                    Text(
                        "Open a document above, or share one to READIT from any app.",
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
        }
    }
}
