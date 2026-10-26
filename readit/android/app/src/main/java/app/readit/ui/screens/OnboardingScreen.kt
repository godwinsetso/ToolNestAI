package app.readit.ui.screens

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.systemBarsPadding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import app.readit.ui.AppViewModel

/** Three short screens: what READIT does, where data lives, and which voice to start with. */
@Composable
fun OnboardingScreen(vm: AppViewModel, onDone: () -> Unit) {
    var step by remember { mutableIntStateOf(0) }
    Surface(Modifier.fillMaxSize()) {
        Column(
            Modifier.fillMaxSize().systemBarsPadding().padding(28.dp).verticalScroll(rememberScrollState()),
            verticalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            Spacer(Modifier.height(24.dp))
            when (step) {
                0 -> {
                    Text("READIT", style = MaterialTheme.typography.displaySmall, color = MaterialTheme.colorScheme.primary)
                    Text("Listen to your documents.", style = MaterialTheme.typography.headlineMedium)
                    Text(
                        "Open a PDF, Word file, presentation or e-book and READIT reads it aloud, highlighting each sentence as it goes. " +
                            "Pause, skip, change speed and pick up exactly where you left off.",
                        style = MaterialTheme.typography.bodyLarge,
                    )
                    Button(onClick = { step = 1 }, modifier = Modifier.fillMaxWidth()) { Text("Next") }
                }
                1 -> {
                    Text("Private by design", style = MaterialTheme.typography.headlineMedium)
                    Text(
                        "No account. Your documents, bookmarks and listening history stay on this phone. " +
                            "READIT only goes online when you choose to download a voice, and never uploads your files.",
                        style = MaterialTheme.typography.bodyLarge,
                    )
                    Button(onClick = { step = 2 }, modifier = Modifier.fillMaxWidth()) { Text("Next") }
                }
                else -> {
                    Text("Choose a voice", style = MaterialTheme.typography.headlineMedium)
                    Text(
                        "You can start right away with your phone's built-in voice. For a much more natural sound, download an offline voice now or later from the Voices tab.",
                        style = MaterialTheme.typography.bodyLarge,
                    )
                    vm.voiceManager.catalog.packs.take(2).forEach { p ->
                        Card(Modifier.fillMaxWidth()) {
                            Column(Modifier.padding(16.dp)) {
                                Text(p.title, style = MaterialTheme.typography.titleMedium)
                                Text("${p.quality} · ${p.downloadBytes / 1_000_000} MB download", style = MaterialTheme.typography.bodySmall)
                                Text(p.requirements, style = MaterialTheme.typography.bodySmall)
                                Spacer(Modifier.height(8.dp))
                                OutlinedButton(onClick = { vm.downloadPack(p.id) }) { Text("Download in background") }
                            }
                        }
                    }
                    Button(onClick = onDone, modifier = Modifier.fillMaxWidth()) { Text("Start reading") }
                }
            }
        }
    }
}
