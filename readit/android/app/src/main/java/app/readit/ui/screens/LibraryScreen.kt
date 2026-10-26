package app.readit.ui.screens

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.Favorite
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material.icons.outlined.FavoriteBorder
import androidx.compose.material.icons.outlined.Search
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExtendedFloatingActionButton
import androidx.compose.material3.FilterChip
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.readit.core.Document
import app.readit.ui.AppViewModel

@Composable
fun LibraryScreen(vm: AppViewModel, onOpen: (String) -> Unit) {
    val docs by vm.documents.collectAsStateWithLifecycle()
    val collections by vm.collections.collectAsStateWithLifecycle()
    var query by remember { mutableStateOf(vm.libraryQuery) }
    var filter by remember { mutableStateOf(vm.libraryFilter) }
    var renaming by remember { mutableStateOf<Document?>(null) }
    var removing by remember { mutableStateOf<Document?>(null) }
    var collectionFor by remember { mutableStateOf<Document?>(null) }
    val picker = rememberImportLauncher { vm.import(it) }

    Box(Modifier.fillMaxSize()) {
        LazyColumn(Modifier.fillMaxSize()) {
            item { ScreenTitle("Library", "${docs.size} document${if (docs.size == 1) "" else "s"}") }
            item {
                OutlinedTextField(
                    value = query,
                    onValueChange = { query = it; vm.setLibraryFilter(filter, it) },
                    leadingIcon = { Icon(Icons.Outlined.Search, null) },
                    placeholder = { Text("Search titles") },
                    singleLine = true,
                    modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp),
                )
            }
            item {
                Row(Modifier.padding(horizontal = 16.dp, vertical = 8.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    listOf("all" to "All", "recent" to "Recent", "favourites" to "Favourites").forEach { (k, label) ->
                        FilterChip(selected = filter == k, onClick = { filter = k; vm.setLibraryFilter(k, query) }, label = { Text(label) })
                    }
                }
            }
            if (collections.isNotEmpty()) {
                item {
                    Text(
                        "Collections: " + collections.joinToString { "${it.name} (${it.documentCount})" },
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        modifier = Modifier.padding(horizontal = 20.dp, vertical = 4.dp),
                    )
                }
            }
            items(docs, key = { it.id }) { d ->
                DocumentRow(d, onClick = { onOpen(d.id) }) {
                    IconButton(onClick = { vm.toggleFavourite(d) }) {
                        Icon(
                            if (d.favourite) Icons.Filled.Favorite else Icons.Outlined.FavoriteBorder,
                            if (d.favourite) "Remove from favourites" else "Add to favourites",
                            tint = if (d.favourite) MaterialTheme.colorScheme.tertiary else MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                    var menu by remember { mutableStateOf(false) }
                    Box {
                        IconButton(onClick = { menu = true }) { Icon(Icons.Filled.MoreVert, "More actions for ${d.title}") }
                        DropdownMenu(expanded = menu, onDismissRequest = { menu = false }) {
                            DropdownMenuItem(text = { Text("Rename") }, onClick = { menu = false; renaming = d })
                            DropdownMenuItem(text = { Text("Add to collection") }, onClick = { menu = false; collectionFor = d })
                            DropdownMenuItem(text = { Text("Remove from READIT") }, onClick = { menu = false; removing = d })
                        }
                    }
                }
                HorizontalDivider(Modifier.padding(start = 86.dp), color = MaterialTheme.colorScheme.surfaceVariant)
            }
            item { Box(Modifier.padding(48.dp)) }
        }
        ExtendedFloatingActionButton(
            onClick = { picker.launch(IMPORT_MIME_TYPES) },
            icon = { Icon(Icons.Filled.Add, null) },
            text = { Text("Import") },
            modifier = Modifier.align(Alignment.BottomEnd).padding(16.dp),
        )
    }

    renaming?.let { d ->
        var title by remember(d.id) { mutableStateOf(d.title) }
        AlertDialog(
            onDismissRequest = { renaming = null },
            title = { Text("Rename") },
            text = { OutlinedTextField(title, { title = it }, singleLine = true) },
            confirmButton = { TextButton(onClick = { vm.rename(d, title); renaming = null }) { Text("Save") } },
            dismissButton = { TextButton(onClick = { renaming = null }) { Text("Cancel") } },
        )
    }

    removing?.let { d ->
        AlertDialog(
            onDismissRequest = { removing = null },
            title = { Text("Remove \"${d.title}\"?") },
            text = {
                Text(
                    "READIT will delete its own copy, your bookmarks and notes for it, and any generated audio. " +
                        "The original file on your phone is not touched.",
                )
            },
            confirmButton = { TextButton(onClick = { vm.remove(d); removing = null }) { Text("Remove") } },
            dismissButton = { TextButton(onClick = { removing = null }) { Text("Cancel") } },
        )
    }

    collectionFor?.let { d ->
        var newName by remember { mutableStateOf("") }
        AlertDialog(
            onDismissRequest = { collectionFor = null },
            title = { Text("Add to collection") },
            text = {
                Column {
                    collections.forEach { c ->
                        TextButton(onClick = { vm.addToCollection(c.id, d); collectionFor = null }) { Text(c.name) }
                    }
                    OutlinedTextField(newName, { newName = it }, label = { Text("New collection") }, singleLine = true)
                }
            },
            confirmButton = {
                TextButton(onClick = {
                    if (newName.isNotBlank()) vm.createCollection(newName.trim(), d)
                    collectionFor = null
                }) { Text("Create") }
            },
            dismissButton = { TextButton(onClick = { collectionFor = null }) { Text("Close") } },
        )
    }
}
