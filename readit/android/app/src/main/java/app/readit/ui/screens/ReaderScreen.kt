package app.readit.ui.screens

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.FormatListBulleted
import androidx.compose.material.icons.filled.FastForward
import androidx.compose.material.icons.filled.FastRewind
import androidx.compose.material.icons.filled.Pause
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.material.icons.filled.SkipNext
import androidx.compose.material.icons.filled.SkipPrevious
import androidx.compose.material.icons.outlined.Bedtime
import androidx.compose.material.icons.outlined.BookmarkAdd
import androidx.compose.material.icons.outlined.Bookmarks
import androidx.compose.material.icons.outlined.MyLocation
import androidx.compose.material.icons.outlined.RecordVoiceOver
import androidx.compose.material.icons.outlined.Search
import androidx.compose.material.icons.outlined.Speed
import androidx.compose.material.icons.outlined.Tune
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.AssistChip
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.FilledIconButton
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.IconButtonDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Slider
import androidx.compose.material3.SuggestionChip
import androidx.compose.material3.Surface
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
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.readit.core.Document
import app.readit.core.LocationKind
import app.readit.core.Section
import app.readit.core.SectionKind
import app.readit.playback.NowPlaying
import app.readit.playback.PlayState
import app.readit.ui.AppViewModel
import app.readit.ui.theme.ReadingStyle
import kotlinx.coroutines.launch

@Composable
fun ReaderScreen(vm: AppViewModel, onNavigate: (String) -> Unit) {
    val now by vm.session.now.collectAsStateWithLifecycle()
    val settings by vm.settings.collectAsStateWithLifecycle()
    val doc = now.document
    if (doc == null) {
        Column(Modifier.fillMaxSize().padding(32.dp), verticalArrangement = Arrangement.Center, horizontalAlignment = Alignment.CenterHorizontally) {
            Text("Nothing is open", style = MaterialTheme.typography.titleLarge)
            Spacer(Modifier.height(8.dp))
            Text(
                "Choose a document from Home or Library and READIT will start reading it here.",
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Spacer(Modifier.height(16.dp))
            TextButton(onClick = { onNavigate("library") }) { Text("Go to Library") }
        }
        return
    }

    var dialog by remember { mutableStateOf<String?>(null) }
    Column(Modifier.fillMaxSize()) {
        ReaderHeader(doc, now)
        Box(Modifier.weight(1f)) {
            if (doc.sectionCount == 0u) NeedsOcr(doc) else DocumentText(vm, doc, now)
        }
        PlayerPanel(vm, doc, now, settings.largeControls, settings.skipSeconds, onDialog = { dialog = it }, onNavigate = onNavigate)
    }

    when (dialog) {
        "speed" -> SpeedDialog(now, onSpeed = vm.session::setSpeed, onPitch = vm.session::setPitch, onDismiss = { dialog = null })
        "sleep" -> SleepDialog(doc, onPick = { m ->
            if (m == -1) vm.session.sleepAtEndOfLocation() else vm.session.setSleepTimer(m)
            dialog = null
        }, onDismiss = { dialog = null })
        "contents" -> ContentsDialog(vm, doc, onPick = { vm.session.seekToSection(it); dialog = null }, onDismiss = { dialog = null })
        "search" -> SearchDialog(vm, doc, onPick = { vm.session.seekToSection(it); dialog = null }, onDismiss = { dialog = null })
        "goto" -> GoToDialog(doc, onGo = { vm.session.goToLocation(it); dialog = null }, onDismiss = { dialog = null })
    }
}

@Composable
private fun ReaderHeader(doc: Document, now: NowPlaying) {
    Column(Modifier.fillMaxWidth().padding(start = 20.dp, end = 20.dp, top = 16.dp, bottom = 8.dp)) {
        Text(doc.title, style = MaterialTheme.typography.titleLarge, maxLines = 2, overflow = TextOverflow.Ellipsis, modifier = Modifier.semantics { heading() })
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(
                listOf(now.locationLabel, formatLabel(doc.format)).filter { it.isNotEmpty() }.joinToString(" · "),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            if (now.voiceLoading || now.state == PlayState.PREPARING) {
                Spacer(Modifier.size(8.dp))
                CircularProgressIndicator(Modifier.size(12.dp), strokeWidth = 2.dp)
                Text(
                    if (now.voiceLoading) "  Loading voice" else "  Preparing speech",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
    }
}

@Composable
private fun NeedsOcr(doc: Document) {
    Column(Modifier.fillMaxSize().padding(28.dp), verticalArrangement = Arrangement.Center) {
        Text("No readable text yet", style = MaterialTheme.typography.titleLarge)
        Spacer(Modifier.height(8.dp))
        Text(doc.warnings.firstOrNull() ?: "This document has no text layer.", style = MaterialTheme.typography.bodyLarge)
        Spacer(Modifier.height(8.dp))
        Text(
            "Reading scanned pages needs OCR (text recognition). On-device OCR is planned for the next phase " +
                "and is not available in this version, so READIT cannot read this file aloud yet.",
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}

private fun locationName(kind: LocationKind) = when (kind) {
    LocationKind.PAGE -> "Page"
    LocationKind.SLIDE -> "Slide"
    LocationKind.CHAPTER -> "Chapter"
    LocationKind.NONE -> ""
}

@Composable
private fun DocumentText(vm: AppViewModel, doc: Document, now: NowPlaying) {
    val list = rememberLazyListState()
    val scope = rememberCoroutineScope()
    var follow by remember(doc.id) { mutableStateOf(true) }
    val current = now.section?.ordinal?.toInt()

    // Keep the sentence being read in view, unless the reader has scrolled away to browse.
    LaunchedEffect(current, follow) {
        if (follow && current != null) list.animateScrollToItem(current, scrollOffset = -160)
    }
    // Only the user's own drags emit interactions (our animated scrolls do not), so this turns following off.
    LaunchedEffect(list) {
        list.interactionSource.interactions.collect { follow = false }
    }

    Box(Modifier.fillMaxSize()) {
        LazyColumn(state = list, modifier = Modifier.fillMaxSize(), contentPadding = androidx.compose.foundation.layout.PaddingValues(bottom = 24.dp)) {
            items(doc.sectionCount.toInt(), key = { it }) { i ->
                val s = vm.section(doc.id, i)
                if (s == null) {
                    Box(Modifier.fillMaxWidth().height(48.dp))
                } else {
                    val prev = if (i > 0) vm.section(doc.id, i - 1) else null
                    if (doc.locationKind != LocationKind.NONE && (prev == null || prev.location != s.location)) {
                        LocationDivider("${locationName(doc.locationKind)} ${s.location}")
                    }
                    SectionView(
                        s,
                        isCurrent = s.ordinal.toInt() == current,
                        highlight = if (s.ordinal.toInt() == current) now.segment?.let { it.startUtf16.toInt() until it.endUtf16.toInt() } else null,
                        onClick = { vm.session.seekToSection(s.ordinal); follow = true },
                    )
                }
            }
        }
        if (!follow && current != null) {
            AssistChip(
                onClick = { follow = true; scope.launch { list.animateScrollToItem(current, -160) } },
                label = { Text("Back to reading position") },
                leadingIcon = { Icon(Icons.Outlined.MyLocation, null) },
                modifier = Modifier.align(Alignment.BottomCenter).padding(12.dp),
            )
        }
    }
}

@Composable
private fun LocationDivider(label: String) {
    Row(Modifier.fillMaxWidth().padding(horizontal = 20.dp, vertical = 10.dp), verticalAlignment = Alignment.CenterVertically) {
        Text(label.uppercase(), style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.outline)
        Spacer(Modifier.size(8.dp))
        HorizontalDivider(Modifier.weight(1f), color = MaterialTheme.colorScheme.surfaceVariant)
    }
}

@Composable
private fun SectionView(s: Section, isCurrent: Boolean, highlight: IntRange?, onClick: () -> Unit) {
    val cs = MaterialTheme.colorScheme
    val base = when (s.kind) {
        SectionKind.HEADING -> ReadingStyle.copy(
            fontFamily = FontFamily.SansSerif,
            fontWeight = FontWeight.SemiBold,
            fontSize = when (s.level.toInt()) { 1 -> 26.sp; 2 -> 22.sp; else -> 20.sp },
            lineHeight = 32.sp,
        )
        SectionKind.CODE -> ReadingStyle.copy(fontFamily = FontFamily.Monospace, fontSize = 14.sp, lineHeight = 20.sp)
        SectionKind.PAGE_FURNITURE -> ReadingStyle.copy(fontSize = 13.sp, fontStyle = FontStyle.Italic, color = cs.outline)
        SectionKind.TABLE_ROW -> ReadingStyle.copy(fontFamily = FontFamily.SansSerif, fontSize = 16.sp)
        else -> ReadingStyle
    }
    val text = buildAnnotatedString {
        if (s.kind == SectionKind.LIST_ITEM) append("•  ")
        val offset = length
        append(s.text)
        if (highlight != null && highlight.first >= 0 && highlight.last < s.text.length) {
            addStyle(SpanStyle(background = cs.primaryContainer, color = cs.onPrimaryContainer), offset + highlight.first, offset + highlight.last + 1)
        }
    }
    val bg = when {
        isCurrent -> cs.surfaceContainer
        s.kind == SectionKind.SPEAKER_NOTES -> cs.tertiaryContainer.copy(alpha = 0.35f)
        else -> Color.Transparent
    }
    Column(
        Modifier.fillMaxWidth()
            .clickable(onClickLabel = "Read from here", onClick = onClick)
            .background(bg)
            .padding(horizontal = 20.dp, vertical = if (s.kind == SectionKind.HEADING) 12.dp else 6.dp)
            .semantics { if (s.kind == SectionKind.HEADING) heading() },
    ) {
        when (s.kind) {
            SectionKind.SPEAKER_NOTES -> Text("SPEAKER NOTES", style = MaterialTheme.typography.labelSmall, color = cs.tertiary)
            SectionKind.PAGE_FURNITURE -> Text("HEADER / FOOTER", style = MaterialTheme.typography.labelSmall, color = cs.outline)
            else -> {}
        }
        Text(text, style = base.copy(color = if (base.color == Color.Unspecified) cs.onSurface else base.color))
    }
}

@Composable
private fun PlayerPanel(
    vm: AppViewModel,
    doc: Document,
    now: NowPlaying,
    large: Boolean,
    skip: Int,
    onDialog: (String) -> Unit,
    onNavigate: (String) -> Unit,
) {
    val session = vm.session
    val big = if (large) 84.dp else 68.dp
    val small = if (large) 60.dp else 48.dp
    Surface(color = MaterialTheme.colorScheme.surfaceContainer, tonalElevation = 2.dp) {
        Column(Modifier.fillMaxWidth().padding(top = 6.dp, bottom = 10.dp)) {
            // Position in the document; drag to jump.
            val total = doc.sectionCount.toFloat().coerceAtLeast(1f)
            var dragging by remember { mutableStateOf<Float?>(null) }
            val pos = dragging ?: ((now.section?.ordinal?.toFloat() ?: 0f) / total)
            Slider(
                value = pos,
                onValueChange = { dragging = it },
                onValueChangeFinished = {
                    dragging?.let { session.seekToSection((it * (total - 1)).toInt().coerceAtLeast(0).toUInt()) }
                    dragging = null
                },
                modifier = Modifier.padding(horizontal = 16.dp).semantics { stateDescription = now.locationLabel },
            )
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceEvenly, verticalAlignment = Alignment.CenterVertically) {
                IconButton(onClick = session::previousParagraph, modifier = Modifier.size(small)) { Icon(Icons.Filled.SkipPrevious, "Previous paragraph") }
                IconButton(onClick = { session.skipSeconds(-skip) }, modifier = Modifier.size(small)) { Icon(Icons.Filled.FastRewind, "Back $skip seconds") }
                FilledIconButton(
                    onClick = session::toggle,
                    modifier = Modifier.size(big).semantics { contentDescription = if (now.isPlaying) "Pause" else "Play" },
                    colors = IconButtonDefaults.filledIconButtonColors(),
                ) {
                    Icon(if (now.isPlaying) Icons.Filled.Pause else Icons.Filled.PlayArrow, null, Modifier.size(big / 2))
                }
                IconButton(onClick = { session.skipSeconds(skip) }, modifier = Modifier.size(small)) { Icon(Icons.Filled.FastForward, "Forward $skip seconds") }
                IconButton(onClick = session::nextParagraph, modifier = Modifier.size(small)) { Icon(Icons.Filled.SkipNext, "Next paragraph") }
            }
            Row(
                Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()).padding(horizontal = 12.dp),
                horizontalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                SuggestionChip(onClick = { onDialog("speed") }, label = { Text(speedLabel(now.speed)) }, icon = { Icon(Icons.Outlined.Speed, null) })
                SuggestionChip(
                    onClick = { onNavigate("voices") },
                    label = { Text(voiceLabel(vm, now.voiceId), maxLines = 1, modifier = Modifier.widthIn(max = 140.dp), overflow = TextOverflow.Ellipsis) },
                    icon = { Icon(Icons.Outlined.RecordVoiceOver, null) },
                )
                SuggestionChip(
                    onClick = { onDialog("sleep") },
                    label = { Text(sleepLabel(now)) },
                    icon = { Icon(Icons.Outlined.Bedtime, null) },
                )
                SuggestionChip(onClick = { session.bookmarkHere() }, label = { Text("Bookmark") }, icon = { Icon(Icons.Outlined.BookmarkAdd, null) })
                SuggestionChip(onClick = { onDialog("contents") }, label = { Text("Contents") }, icon = { Icon(Icons.AutoMirrored.Filled.FormatListBulleted, null) })
                SuggestionChip(onClick = { onDialog("search") }, label = { Text("Search") }, icon = { Icon(Icons.Outlined.Search, null) })
                if (doc.locationKind != LocationKind.NONE) {
                    SuggestionChip(onClick = { onDialog("goto") }, label = { Text("Go to ${locationName(doc.locationKind).lowercase()}") }, icon = { Icon(Icons.Outlined.Tune, null) })
                }
                SuggestionChip(onClick = { onNavigate("bookmarks") }, label = { Text("Notes") }, icon = { Icon(Icons.Outlined.Bookmarks, null) })
            }
        }
    }
}

private fun speedLabel(v: Float): String = "%.2f".format(v).trimEnd('0').trimEnd('.') + "×"

private fun voiceLabel(vm: AppViewModel, id: String?): String = when {
    id == null -> "Voice"
    id.startsWith("system:") -> "Phone voice"
    else -> vm.voiceManager.catalog.voice(id)?.let { "${it.name} (${it.accent})" } ?: id
}

private fun sleepLabel(n: NowPlaying): String = when {
    n.sleepAtEndOfLocation -> "Sleep: end of section"
    n.sleepEndsAtMs != null -> "Sleep: ${((n.sleepEndsAtMs - System.currentTimeMillis()) / 60_000).coerceAtLeast(1)} min"
    else -> "Sleep timer"
}

@Composable
private fun SpeedDialog(now: NowPlaying, onSpeed: (Float) -> Unit, onPitch: (Float) -> Unit, onDismiss: () -> Unit) {
    var speed by remember { mutableStateOf(now.speed) }
    var pitch by remember { mutableStateOf(now.pitch) }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Reading speed") },
        text = {
            Column {
                Text("%.2f×".format(speed), style = MaterialTheme.typography.headlineMedium)
                Slider(value = speed, onValueChange = { speed = (it * 20).toInt() / 20f }, valueRange = 0.5f..3f)
                Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                    listOf(0.75f, 1f, 1.25f, 1.5f, 2f).forEach { p ->
                        SuggestionChip(onClick = { speed = p }, label = { Text("$p×") })
                    }
                }
                Spacer(Modifier.height(12.dp))
                if (now.pitchSupported) {
                    Text("Pitch %.2f".format(pitch))
                    Slider(value = pitch, onValueChange = { pitch = (it * 20).toInt() / 20f }, valueRange = 0.5f..2f)
                } else {
                    Text(
                        "Pitch can't be changed with this voice. It is available with the phone's built-in voices.",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
        },
        confirmButton = {
            TextButton(onClick = {
                onSpeed(speed)
                if (now.pitchSupported) onPitch(pitch)
                onDismiss()
            }) { Text("Apply") }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Cancel") } },
    )
}

@Composable
private fun SleepDialog(doc: Document, onPick: (Int?) -> Unit, onDismiss: () -> Unit) {
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Sleep timer") },
        text = {
            Column {
                listOf(5, 10, 15, 30, 45, 60).forEach { m -> TextButton(onClick = { onPick(m) }) { Text("$m minutes") } }
                if (doc.locationKind != LocationKind.NONE) {
                    TextButton(onClick = { onPick(-1) }) { Text("End of this ${locationName(doc.locationKind).lowercase()}") }
                }
                TextButton(onClick = { onPick(null) }) { Text("Off") }
            }
        },
        confirmButton = {},
        dismissButton = { TextButton(onClick = onDismiss) { Text("Close") } },
    )
}

@Composable
private fun ContentsDialog(vm: AppViewModel, doc: Document, onPick: (UInt) -> Unit, onDismiss: () -> Unit) {
    var items by remember { mutableStateOf<List<Section>?>(null) }
    LaunchedEffect(doc.id) { items = vm.outline(doc.id) }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Contents") },
        text = {
            val list = items
            when {
                list == null -> CircularProgressIndicator()
                list.isEmpty() -> Text("This document has no headings. Use Search or the position slider to move around.")
                else -> LazyColumn(Modifier.heightIn(max = 420.dp)) {
                    items(list) { s ->
                        Text(
                            s.text,
                            maxLines = 2,
                            overflow = TextOverflow.Ellipsis,
                            style = if (s.level <= 1u) MaterialTheme.typography.titleSmall else MaterialTheme.typography.bodyMedium,
                            modifier = Modifier.fillMaxWidth().clickable { onPick(s.ordinal) }
                                .padding(start = (12 * (s.level.toInt() - 1).coerceAtLeast(0)).dp, top = 10.dp, bottom = 10.dp),
                        )
                    }
                }
            }
        },
        confirmButton = {},
        dismissButton = { TextButton(onClick = onDismiss) { Text("Close") } },
    )
}

@Composable
private fun SearchDialog(vm: AppViewModel, doc: Document, onPick: (UInt) -> Unit, onDismiss: () -> Unit) {
    var q by remember { mutableStateOf("") }
    var results by remember { mutableStateOf<List<Section>>(emptyList()) }
    LaunchedEffect(q) { results = if (q.trim().length >= 2) vm.search(doc.id, q) else emptyList() }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Search in document") },
        text = {
            Column {
                OutlinedTextField(q, { q = it }, singleLine = true, placeholder = { Text("Words to find") })
                if (q.trim().length >= 2) Text("${results.size} matches", style = MaterialTheme.typography.bodySmall, modifier = Modifier.padding(vertical = 6.dp))
                LazyColumn(Modifier.heightIn(max = 360.dp)) {
                    items(results) { s ->
                        Column(Modifier.fillMaxWidth().clickable { onPick(s.ordinal) }.padding(vertical = 8.dp)) {
                            if (s.location > 0u) Text("${locationName(doc.locationKind)} ${s.location}", style = MaterialTheme.typography.labelSmall)
                            Text(s.text, maxLines = 3, overflow = TextOverflow.Ellipsis, style = MaterialTheme.typography.bodyMedium)
                        }
                    }
                }
            }
        },
        confirmButton = {},
        dismissButton = { TextButton(onClick = onDismiss) { Text("Close") } },
    )
}

@Composable
private fun GoToDialog(doc: Document, onGo: (UInt) -> Unit, onDismiss: () -> Unit) {
    var v by remember { mutableStateOf("") }
    val n = v.toIntOrNull()
    val valid = n != null && n in 1..doc.locationCount.toInt()
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Go to ${locationName(doc.locationKind).lowercase()}") },
        text = {
            OutlinedTextField(
                v, { v = it.filter(Char::isDigit).take(5) },
                singleLine = true,
                label = { Text("1 to ${doc.locationCount}") },
                isError = v.isNotEmpty() && !valid,
            )
        },
        confirmButton = { TextButton(enabled = valid, onClick = { onGo(n!!.toUInt()) }) { Text("Go") } },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Cancel") } },
    )
}
