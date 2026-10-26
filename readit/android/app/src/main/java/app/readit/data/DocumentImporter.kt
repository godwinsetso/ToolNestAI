package app.readit.data

import android.content.Context
import android.net.Uri
import android.provider.OpenableColumns
import app.readit.core.Document
import app.readit.core.ReaditException
import app.readit.core.ReaditLibrary
import java.io.File

sealed interface ImportOutcome {
    val name: String
    data class Added(override val name: String, val document: Document) : ImportOutcome
    data class AlreadyThere(override val name: String, val document: Document) : ImportOutcome
    data class Failed(override val name: String, val reason: String) : ImportOutcome
}

/**
 * Brings a file from the picker, the share menu or "Open with" into the library.
 * The file is copied into private storage; READIT never edits the original.
 */
class DocumentImporter(private val context: Context, private val lib: ReaditLibrary) {

    fun displayName(uri: Uri): String =
        runCatching {
            context.contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use { c ->
                if (c.moveToFirst()) c.getString(0) else null
            }
        }.getOrNull() ?: uri.lastPathSegment?.substringAfterLast('/') ?: "document"

    /** Blocking; call off the main thread. [progress] receives bytes copied so far. */
    fun import(uri: Uri, progress: (Long) -> Unit = {}): ImportOutcome {
        val name = displayName(uri)
        val dir = File(context.cacheDir, "import").apply { mkdirs() }
        val safeName = name.replace(Regex("[^A-Za-z0-9._ -]"), "_").take(120).ifBlank { "document" }
        val tmp = File(dir, "${System.nanoTime()}-$safeName")
        return try {
            val input = context.contentResolver.openInputStream(uri)
                ?: return ImportOutcome.Failed(name, "The file could not be opened.")
            input.use { inp ->
                tmp.outputStream().use { out ->
                    val buf = ByteArray(64 * 1024)
                    var total = 0L
                    while (true) {
                        val n = inp.read(buf)
                        if (n < 0) break
                        out.write(buf, 0, n)
                        total += n
                        progress(total)
                    }
                }
            }
            val r = lib.importFile(tmp.absolutePath, name)
            if (r.alreadyInLibrary) ImportOutcome.AlreadyThere(name, r.document) else ImportOutcome.Added(name, r.document)
        } catch (e: ReaditException) {
            ImportOutcome.Failed(name, e.message ?: "The file could not be read.")
        } catch (e: SecurityException) {
            ImportOutcome.Failed(name, "READIT was not given permission to read this file.")
        } catch (e: Exception) {
            ImportOutcome.Failed(name, e.message ?: "The file could not be imported.")
        } finally {
            tmp.delete()
        }
    }
}
