package app.readit.ui.screens

import android.net.Uri
import androidx.activity.compose.ManagedActivityResultLauncher
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import app.readit.core.DocFormat
import app.readit.core.Document
import app.readit.core.LocationKind
import java.text.DateFormat
import java.util.Date

/** Every format READIT can open (legacy Office files get a clear conversion message). */
val IMPORT_MIME_TYPES = arrayOf(
    "application/pdf",
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
    "application/vnd.openxmlformats-officedocument.presentationml.presentation",
    "application/msword",
    "application/vnd.ms-powerpoint",
    "application/epub+zip",
    "application/rtf",
    "text/*",
    "image/*",
)

@Composable
fun rememberImportLauncher(onPicked: (List<Uri>) -> Unit): ManagedActivityResultLauncher<Array<String>, List<Uri>> =
    rememberLauncherForActivityResult(ActivityResultContracts.OpenMultipleDocuments()) { uris ->
        if (uris.isNotEmpty()) onPicked(uris)
    }

@Composable
fun ScreenTitle(title: String, subtitle: String? = null, modifier: Modifier = Modifier) {
    Column(modifier.padding(start = 20.dp, end = 20.dp, top = 20.dp, bottom = 8.dp)) {
        Text(title, style = MaterialTheme.typography.headlineMedium)
        if (subtitle != null) {
            Text(subtitle, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun BackBar(title: String, onBack: () -> Unit) {
    TopAppBar(
        title = { Text(title) },
        navigationIcon = { IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Filled.ArrowBack, "Back") } },
    )
}

fun formatLabel(f: DocFormat): String = when (f) {
    DocFormat.PDF -> "PDF"
    DocFormat.DOCX, DocFormat.DOC -> "WORD"
    DocFormat.PPTX, DocFormat.PPT -> "SLIDES"
    DocFormat.EPUB -> "EPUB"
    DocFormat.TXT -> "TEXT"
    DocFormat.RTF -> "RTF"
    DocFormat.HTML -> "WEB"
    DocFormat.MARKDOWN -> "MD"
    DocFormat.IMAGE -> "IMAGE"
}

fun sizeLabel(bytes: ULong): String {
    val b = bytes.toDouble()
    return when {
        b >= 1e9 -> "%.1f GB".format(b / 1e9)
        b >= 1e6 -> "%.1f MB".format(b / 1e6)
        else -> "%.0f KB".format((b / 1e3).coerceAtLeast(1.0))
    }
}

fun extentLabel(d: Document): String = when (d.locationKind) {
    LocationKind.PAGE -> "${d.locationCount} pages"
    LocationKind.SLIDE -> "${d.locationCount} slides"
    LocationKind.CHAPTER -> "${d.locationCount} chapters"
    LocationKind.NONE -> "${d.sectionCount} paragraphs"
}

fun dateLabel(epochSeconds: Long): String = DateFormat.getDateInstance(DateFormat.MEDIUM).format(Date(epochSeconds * 1000))

@Composable
fun FormatBadge(f: DocFormat) {
    Surface(color = MaterialTheme.colorScheme.primaryContainer, shape = RoundedCornerShape(8.dp)) {
        Box(Modifier.size(width = 52.dp, height = 64.dp), contentAlignment = Alignment.Center) {
            Text(
                formatLabel(f),
                style = MaterialTheme.typography.labelSmall,
                fontWeight = FontWeight.Bold,
                color = MaterialTheme.colorScheme.onPrimaryContainer,
            )
        }
    }
}

@Composable
fun DocumentRow(d: Document, onClick: () -> Unit, trailing: @Composable () -> Unit = {}) {
    Row(
        Modifier.fillMaxWidth().clickable(onClick = onClick).padding(horizontal = 20.dp, vertical = 10.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        FormatBadge(d.format)
        Spacer(Modifier.width(14.dp))
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
            Text(d.title, style = MaterialTheme.typography.titleMedium, maxLines = 2, overflow = TextOverflow.Ellipsis)
            Text(
                "${extentLabel(d)} · ${sizeLabel(d.sizeBytes)}" + (d.lastOpenedAt?.let { " · opened ${dateLabel(it)}" } ?: ""),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
        }
        trailing()
    }
}
