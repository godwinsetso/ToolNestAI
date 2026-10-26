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
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.KeyboardArrowRight
import androidx.compose.material.icons.outlined.Delete
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.Checkbox
import androidx.compose.material3.FilterChip
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.RadioButton
import androidx.compose.material3.Slider
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.readit.core.Bookmark
import app.readit.core.PronunciationRule
import app.readit.core.ReadingProfile
import app.readit.core.StorageReport
import app.readit.core.coreVersion
import app.readit.ui.AppViewModel
import kotlinx.coroutines.launch

@Composable
private fun SectionHeader(text: String) =
    Text(text.uppercase(), style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.primary, modifier = Modifier.padding(start = 20.dp, top = 24.dp, bottom = 6.dp))

@Composable
private fun NavRow(title: String, subtitle: String? = null, onClick: () -> Unit) {
    Row(Modifier.fillMaxWidth().clickable(onClick = onClick).padding(horizontal = 20.dp, vertical = 14.dp), verticalAlignment = Alignment.CenterVertically) {
        Column(Modifier.weight(1f)) {
            Text(title, style = MaterialTheme.typography.bodyLarge)
            subtitle?.let { Text(it, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant) }
        }
        Icon(Icons.AutoMirrored.Filled.KeyboardArrowRight, null)
    }
}

@Composable
private fun SwitchRow(title: String, subtitle: String?, checked: Boolean, onChange: (Boolean) -> Unit) {
    Row(Modifier.fillMaxWidth().clickable { onChange(!checked) }.padding(horizontal = 20.dp, vertical = 10.dp), verticalAlignment = Alignment.CenterVertically) {
        Column(Modifier.weight(1f)) {
            Text(title, style = MaterialTheme.typography.bodyLarge)
            subtitle?.let { Text(it, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant) }
        }
        Switch(checked = checked, onCheckedChange = onChange)
    }
}

@Composable
fun SettingsScreen(vm: AppViewModel, onNavigate: (String) -> Unit) {
    val s by vm.settings.collectAsStateWithLifecycle()
    var confirmErase by remember { mutableStateOf(false) }
    Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState())) {
        ScreenTitle("Settings")

        SectionHeader("Listening")
        NavRow("Reading profiles", "Active: ${s.activeProfile}. Study, Work, Relaxed, Accessibility, My Reader") { onNavigate("profiles") }
        NavRow("Pronunciation", "Teach READIT how to say names, terms and abbreviations") { onNavigate("pronunciation") }
        Column(Modifier.padding(horizontal = 20.dp, vertical = 8.dp)) {
            Text("Skip forward and back by ${s.skipSeconds} seconds", style = MaterialTheme.typography.bodyLarge)
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                listOf(5, 10, 15, 30, 60).forEach { n ->
                    FilterChip(selected = s.skipSeconds == n, onClick = { vm.setSetting("skip_seconds", n.toString()) }, label = { Text("$n s") })
                }
            }
        }

        SectionHeader("Appearance and accessibility")
        Column(Modifier.padding(horizontal = 20.dp)) {
            Text("Theme", style = MaterialTheme.typography.bodyLarge)
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                listOf("system" to "Match phone", "light" to "Light", "dark" to "Dark").forEach { (k, l) ->
                    FilterChip(selected = s.themeMode == k, onClick = { vm.setSetting("theme", k) }, label = { Text(l) })
                }
            }
            Spacer(Modifier.height(8.dp))
            Text("Text size ${(s.textScale * 100).toInt()}%", style = MaterialTheme.typography.bodyLarge)
            var scale by remember(s.textScale) { mutableStateOf(s.textScale) }
            Slider(value = scale, onValueChange = { scale = it }, onValueChangeFinished = { vm.setSetting("text_scale", "%.2f".format(scale)) }, valueRange = 0.85f..1.6f, steps = 14)
        }
        SwitchRow("High contrast", "Stronger text and outlines", s.highContrast) { vm.setSetting("high_contrast", if (it) "1" else "0") }
        SwitchRow("Large playback controls", "Bigger buttons that are easier to hit", s.largeControls) { vm.setSetting("large_controls", if (it) "1" else "0") }

        SectionHeader("Storage and privacy")
        NavRow("Storage", "Documents, voices and generated audio on this phone") { onNavigate("storage") }
        Text(
            "READIT works without an account. Your documents, reading history, bookmarks and voices stay on this phone. " +
                "The only network use is downloading a voice you choose. Nothing is uploaded, and there are no analytics.",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.padding(horizontal = 20.dp, vertical = 8.dp),
        )
        TextButton(onClick = { confirmErase = true }, modifier = Modifier.padding(horizontal = 12.dp)) {
            Text("Erase all READIT data on this phone", color = MaterialTheme.colorScheme.error)
        }

        SectionHeader("About")
        NavRow("Help and troubleshooting") { onNavigate("help") }
        Text(
            "READIT 0.1.0 · ${runCatching { coreVersion() }.getOrDefault("")}\nSpeech: sherpa-onnx 1.13.8 (Kokoro, Piper) and the phone's own voices. PDF: PDFium.",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.padding(20.dp),
        )
    }

    if (confirmErase) {
        AlertDialog(
            onDismissRequest = { confirmErase = false },
            title = { Text("Erase everything?") },
            text = {
                Text(
                    "This deletes READIT's library, its copies of your documents, bookmarks, notes, history, settings, " +
                        "pronunciations and downloaded voices from this phone. Original files elsewhere on your phone are not touched. This cannot be undone.",
                )
            },
            confirmButton = { TextButton(onClick = { vm.eraseEverything(); confirmErase = false }) { Text("Erase", color = MaterialTheme.colorScheme.error) } },
            dismissButton = { TextButton(onClick = { confirmErase = false }) { Text("Cancel") } },
        )
    }
}

@Composable
fun ProfilesScreen(vm: AppViewModel, onBack: () -> Unit) {
    val s by vm.settings.collectAsStateWithLifecycle()
    var profiles by remember { mutableStateOf<List<ReadingProfile>>(emptyList()) }
    var editing by remember { mutableStateOf<ReadingProfile?>(null) }
    LaunchedEffect(Unit) { profiles = vm.profiles() }
    Column(Modifier.fillMaxSize()) {
        BackBar("Reading profiles", onBack)
        LazyColumn {
            items(profiles, key = { it.id }) { p ->
                Row(Modifier.fillMaxWidth().clickable { vm.activateProfile(p) }.padding(horizontal = 12.dp, vertical = 8.dp), verticalAlignment = Alignment.CenterVertically) {
                    RadioButton(selected = s.activeProfile == p.name, onClick = { vm.activateProfile(p) })
                    Column(Modifier.weight(1f)) {
                        Text(p.name, style = MaterialTheme.typography.bodyLarge)
                        Text(
                            "${"%.2f".format(p.speed)}× · paragraph pause ${p.paragraphPauseMs} ms · " +
                                (if (p.skipHeadersFooters) "skips headers/footers" else "reads headers/footers") +
                                (if (p.readSpeakerNotes) " · reads speaker notes" else ""),
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                    TextButton(onClick = { editing = p }) { Text("Edit") }
                }
            }
        }
    }
    editing?.let { p ->
        var speed by remember(p.id) { mutableStateOf(p.speed) }
        var pause by remember(p.id) { mutableStateOf(p.paragraphPauseMs.toFloat()) }
        var heading by remember(p.id) { mutableStateOf(p.headingPauseMs.toFloat()) }
        var notes by remember(p.id) { mutableStateOf(p.readSpeakerNotes) }
        var slides by remember(p.id) { mutableStateOf(p.readSlideContent) }
        var furniture by remember(p.id) { mutableStateOf(p.skipHeadersFooters) }
        var useCurrentVoice by remember(p.id) { mutableStateOf(false) }
        AlertDialog(
            onDismissRequest = { editing = null },
            title = { Text(p.name) },
            text = {
                Column(Modifier.verticalScroll(rememberScrollState())) {
                    Text("Speed ${"%.2f".format(speed)}×")
                    Slider(speed, { speed = (it * 20).toInt() / 20f }, valueRange = 0.5f..3f)
                    Text("Pause after paragraphs ${pause.toInt()} ms")
                    Slider(pause, { pause = it }, valueRange = 0f..2000f)
                    Text("Pause after headings ${heading.toInt()} ms")
                    Slider(heading, { heading = it }, valueRange = 0f..3000f)
                    Row(verticalAlignment = Alignment.CenterVertically) { Checkbox(furniture, { furniture = it }); Text("Skip page headers, footers and numbers") }
                    Text("Presentations", style = MaterialTheme.typography.labelLarge)
                    Row(verticalAlignment = Alignment.CenterVertically) { Checkbox(slides, { slides = it || !notes }); Text("Read slide content") }
                    Row(verticalAlignment = Alignment.CenterVertically) { Checkbox(notes, { notes = it || !slides }); Text("Read speaker notes") }
                    Row(verticalAlignment = Alignment.CenterVertically) { Checkbox(useCurrentVoice, { useCurrentVoice = it }); Text("Use the current voice for this profile") }
                }
            },
            confirmButton = {
                TextButton(onClick = {
                    vm.saveProfile(
                        p.copy(
                            speed = speed, paragraphPauseMs = pause.toUInt(), headingPauseMs = heading.toUInt(),
                            readSpeakerNotes = notes, readSlideContent = slides, skipHeadersFooters = furniture,
                            voiceId = if (useCurrentVoice) vm.session.now.value.voiceId else p.voiceId,
                        ),
                    )
                    editing = null
                    profiles = profiles.map { if (it.id == p.id) it.copy(speed = speed) else it }
                }) { Text("Save") }
            },
            dismissButton = { TextButton(onClick = { editing = null }) { Text("Cancel") } },
        )
    }
}

@Composable
fun PronunciationScreen(vm: AppViewModel, onBack: () -> Unit) {
    val scope = rememberCoroutineScope()
    var rules by remember { mutableStateOf<List<PronunciationRule>>(emptyList()) }
    var term by remember { mutableStateOf("") }
    var say by remember { mutableStateOf("") }
    var exact by remember { mutableStateOf(false) }
    var tryText by remember { mutableStateOf("Dr. Mensah paid GH₵ 1,250.50 on 01/03/2026 under KYC rule 4.") }
    var spoken by remember { mutableStateOf("") }
    val reload: () -> Unit = { scope.launch { rules = vm.pronunciations(); spoken = vm.previewSpoken(tryText) } }
    LaunchedEffect(Unit) { reload() }
    Column(Modifier.fillMaxSize()) {
        BackBar("Pronunciation", onBack)
        LazyColumn(Modifier.weight(1f)) {
            item {
                Column(Modifier.padding(20.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                    Text("Add a word", style = MaterialTheme.typography.titleMedium)
                    OutlinedTextField(term, { term = it }, label = { Text("Written as") }, singleLine = true, modifier = Modifier.fillMaxWidth())
                    OutlinedTextField(say, { say = it }, label = { Text("Say it as (e.g. SET-so-AH-fee-ah)") }, singleLine = true, modifier = Modifier.fillMaxWidth())
                    Row(verticalAlignment = Alignment.CenterVertically) { Checkbox(exact, { exact = it }); Text("Match capital letters exactly") }
                    Button(enabled = term.isNotBlank() && say.isNotBlank(), onClick = {
                        vm.addPronunciation(term.trim(), say.trim(), exact)
                        term = ""; say = ""
                        scope.launch { kotlinx.coroutines.delay(200); reload() }
                    }) { Text("Add") }
                    HorizontalDivider()
                    Text("Try it", style = MaterialTheme.typography.titleMedium)
                    OutlinedTextField(tryText, { tryText = it; reload() }, modifier = Modifier.fillMaxWidth())
                    Text("READIT will say: $spoken", style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                    HorizontalDivider()
                    Text("Your words (${rules.size})", style = MaterialTheme.typography.titleMedium)
                }
            }
            items(rules, key = { it.id }) { r ->
                Row(Modifier.fillMaxWidth().padding(horizontal = 20.dp, vertical = 4.dp), verticalAlignment = Alignment.CenterVertically) {
                    Column(Modifier.weight(1f)) {
                        Text(r.term, style = MaterialTheme.typography.bodyLarge)
                        Text("says \"${r.replacement}\"", style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                    }
                    IconButton(onClick = { vm.deletePronunciation(r.id); scope.launch { kotlinx.coroutines.delay(200); reload() } }) {
                        Icon(Icons.Outlined.Delete, "Delete ${r.term}")
                    }
                }
            }
        }
    }
}

@Composable
fun StorageScreen(vm: AppViewModel, onBack: () -> Unit) {
    var report by remember { mutableStateOf<StorageReport?>(null) }
    val scope = rememberCoroutineScope()
    LaunchedEffect(Unit) { report = vm.storage() }
    Column(Modifier.fillMaxSize()) {
        BackBar("Storage", onBack)
        val r = report
        if (r != null) {
            Column(Modifier.padding(20.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
                listOf(
                    "Documents (READIT's private copies)" to r.documentsBytes,
                    "Downloaded voices" to r.voicesBytes,
                    "Generated audio cache" to r.audioCacheBytes,
                    "Library database" to r.databaseBytes,
                ).forEach { (label, b) ->
                    Row { Text(label, Modifier.weight(1f)); Text(sizeLabel(b)) }
                }
                Text("Free on this phone: ${vm.voiceManager.freeBytes() / 1_000_000} MB", style = MaterialTheme.typography.bodySmall)
                OutlinedButton(onClick = { vm.clearAudioCache(); scope.launch { report = vm.storage() } }) { Text("Clear generated audio") }
                Text(
                    "Clearing generated audio never deletes documents or voices. Remove voices from the Voices screen; remove documents from the Library.",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
    }
}

@Composable
fun BookmarksScreen(vm: AppViewModel, onBack: () -> Unit) {
    val now by vm.session.now.collectAsStateWithLifecycle()
    val doc = now.document
    val scope = rememberCoroutineScope()
    var list by remember { mutableStateOf<List<Bookmark>>(emptyList()) }
    var editing by remember { mutableStateOf<Bookmark?>(null) }
    LaunchedEffect(doc?.id) { list = doc?.let { vm.bookmarks(it.id) }.orEmpty() }
    Column(Modifier.fillMaxSize()) {
        BackBar("Bookmarks and notes", onBack)
        if (doc == null || list.isEmpty()) {
            Text(
                if (doc == null) "Open a document to see its bookmarks." else "No bookmarks yet. Tap Bookmark while listening to save your place.",
                modifier = Modifier.padding(24.dp),
                textAlign = TextAlign.Start,
            )
        }
        LazyColumn {
            items(list, key = { it.id }) { b ->
                Row(
                    Modifier.fillMaxWidth().clickable { vm.session.seekToSection(b.sectionOrdinal, b.segmentIndex); onBack() }.padding(horizontal = 20.dp, vertical = 10.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Column(Modifier.weight(1f)) {
                        Text(b.label, style = MaterialTheme.typography.bodyLarge, maxLines = 2)
                        b.note?.let { Text(it, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.primary) }
                        Text(dateLabel(b.createdAt), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                    }
                    TextButton(onClick = { editing = b }) { Text("Note") }
                    IconButton(onClick = { vm.deleteBookmark(b.id); list = list - b }) { Icon(Icons.Outlined.Delete, "Delete bookmark") }
                }
            }
        }
    }
    editing?.let { b ->
        var note by remember(b.id) { mutableStateOf(b.note.orEmpty()) }
        AlertDialog(
            onDismissRequest = { editing = null },
            title = { Text("Note") },
            text = { OutlinedTextField(note, { note = it }, minLines = 3) },
            confirmButton = {
                TextButton(onClick = {
                    vm.updateBookmarkNote(b.id, note.ifBlank { null })
                    list = list.map { if (it.id == b.id) it.copy(note = note.ifBlank { null }) else it }
                    editing = null
                }) { Text("Save") }
            },
            dismissButton = { TextButton(onClick = { editing = null }) { Text("Cancel") } },
        )
    }
}

@Composable
fun HelpScreen(onBack: () -> Unit) {
    val topics = listOf(
        "A document says it needs OCR" to "It is a scan: the pages are pictures of text. On-device OCR is planned for the next version. Until then, export a text-based PDF or Word file from the original if you can.",
        "A Word .doc or PowerPoint .ppt file won't open" to "These are older Office formats. Open the file in Word, PowerPoint, LibreOffice or Google Docs, save it as .docx or .pptx, then import that copy.",
        "\"Password-protected\"" to "READIT cannot read encrypted files. Remove the password in the app that created the file, then import it again.",
        "Speech pauses between sentences" to "The voice is too heavy for this phone. Use Voices > Test to check, then pick a Piper voice or the phone's built-in voice, which are much lighter.",
        "Reading stops when I lock the phone" to "Allow READIT's notification, and on some phones turn off battery optimisation for READIT (Settings > Apps > READIT > Battery > Unrestricted).",
        "A word is mispronounced" to "Settings > Pronunciation lets you add how a word should be said. It applies to every document.",
        "Headers or page numbers are read aloud" to "Settings > Reading profiles > Edit, and tick \"Skip page headers, footers and numbers\". They stay visible on screen but are not spoken.",
        "Where is my data?" to "Only on this phone, in READIT's private storage. Nothing is uploaded. Settings > Storage shows what is used; Settings > Erase removes all of it.",
    )
    Column(Modifier.fillMaxSize()) {
        BackBar("Help", onBack)
        LazyColumn {
            items(topics) { (q, a) ->
                Column(Modifier.padding(horizontal = 20.dp, vertical = 10.dp)) {
                    Text(q, style = MaterialTheme.typography.titleSmall)
                    Text(a, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
            }
        }
    }
}
