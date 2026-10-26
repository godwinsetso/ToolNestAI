package app.readit.ui

import android.app.Application
import android.net.Uri
import androidx.compose.runtime.mutableStateMapOf
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import app.readit.ReaditApp
import app.readit.core.Bookmark
import app.readit.core.Collection
import app.readit.core.Document
import app.readit.core.InstalledVoice
import app.readit.core.PronunciationRule
import app.readit.core.ReadingProfile
import app.readit.core.Section
import app.readit.core.StorageReport
import app.readit.data.DocumentImporter
import app.readit.data.ImportOutcome
import app.readit.data.readingOptions
import app.readit.tts.SystemTtsEngine
import app.readit.tts.VoiceDownloadWorker
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

data class ImportProgress(val name: String, val index: Int, val total: Int, val bytes: Long)

data class AppSettings(
    val themeMode: String = "system",
    val highContrast: Boolean = false,
    val textScale: Float = 1f,
    val largeControls: Boolean = false,
    val skipSeconds: Int = 15,
    val onboardingDone: Boolean = false,
    val activeProfile: String = "My Reader",
)

class AppViewModel(app: Application) : AndroidViewModel(app) {
    private val readit = app as ReaditApp
    val lib = readit.library
    val session = readit.session
    val voiceManager = readit.voices
    private val importer = DocumentImporter(app, lib)

    private val _documents = MutableStateFlow<List<Document>>(emptyList())
    val documents: StateFlow<List<Document>> = _documents
    private val _recent = MutableStateFlow<List<Document>>(emptyList())
    val recent: StateFlow<List<Document>> = _recent
    private val _collections = MutableStateFlow<List<Collection>>(emptyList())
    val collections: StateFlow<List<Collection>> = _collections
    private val _importing = MutableStateFlow<ImportProgress?>(null)
    val importing: StateFlow<ImportProgress?> = _importing
    private val _messages = MutableSharedFlow<String>(extraBufferCapacity = 8)
    val messages: SharedFlow<String> = _messages
    private val _settings = MutableStateFlow(AppSettings())
    val settings: StateFlow<AppSettings> = _settings
    private val _installedVoices = MutableStateFlow<List<InstalledVoice>>(emptyList())
    val installedVoices: StateFlow<List<InstalledVoice>> = _installedVoices
    private val _systemVoices = MutableStateFlow<List<SystemTtsEngine.SystemVoice>?>(null)
    val systemVoices: StateFlow<List<SystemTtsEngine.SystemVoice>?> = _systemVoices

    var libraryFilter = "all"
        private set
    var libraryQuery = ""
        private set

    init {
        refreshAll()
    }

    private fun io(block: suspend () -> Unit) = viewModelScope.launch(Dispatchers.IO) {
        try { block() } catch (e: Exception) { _messages.tryEmit(e.message ?: "Something went wrong.") }
    }

    fun refreshAll() = io {
        _settings.value = AppSettings(
            themeMode = lib.setting("theme") ?: "system",
            highContrast = lib.setting("high_contrast") == "1",
            textScale = lib.setting("text_scale")?.toFloatOrNull() ?: 1f,
            largeControls = lib.setting("large_controls") == "1",
            skipSeconds = lib.setting("skip_seconds")?.toIntOrNull() ?: 15,
            onboardingDone = lib.setting("onboarding_done") == "1",
            activeProfile = lib.setting("active_profile") ?: "My Reader",
        )
        refreshLibrarySync()
        _installedVoices.value = lib.installedVoices()
    }

    private fun refreshLibrarySync() {
        _documents.value = lib.documents(libraryFilter, libraryQuery.ifBlank { null })
        _recent.value = lib.documents("recent", null).take(8)
        _collections.value = lib.collections()
    }

    fun refreshLibrary() = io { refreshLibrarySync() }

    fun setLibraryFilter(filter: String, query: String) {
        libraryFilter = filter
        libraryQuery = query
        refreshLibrary()
    }

    // --------------------------------------------------------------- import

    /** Import one or more files; returns the first new or existing document, for opening. */
    fun import(uris: List<Uri>, openFirst: (Document) -> Unit = {}) = io {
        var first: Document? = null
        val failures = mutableListOf<String>()
        uris.forEachIndexed { i, uri ->
            val name = importer.displayName(uri)
            _importing.value = ImportProgress(name, i + 1, uris.size, 0)
            when (val r = importer.import(uri) { b -> _importing.value = ImportProgress(name, i + 1, uris.size, b) }) {
                is ImportOutcome.Added -> {
                    first = first ?: r.document
                    if (r.document.warnings.isNotEmpty()) _messages.tryEmit("${r.name}: ${r.document.warnings.first()}")
                }
                is ImportOutcome.AlreadyThere -> {
                    first = first ?: r.document
                    _messages.tryEmit("${r.name} is already in your library.")
                }
                is ImportOutcome.Failed -> failures += "${r.name}: ${r.reason}"
            }
        }
        _importing.value = null
        refreshLibrarySync()
        failures.forEach { _messages.tryEmit(it) }
        if (uris.size > 1) _messages.tryEmit("Imported ${uris.size - failures.size} of ${uris.size} files.")
        first?.let { withContext(Dispatchers.Main) { openFirst(it) } }
    }

    fun toggleFavourite(doc: Document) = io { lib.setFavourite(doc.id, !doc.favourite); refreshLibrarySync() }

    fun rename(doc: Document, title: String) = io { if (title.isNotBlank()) lib.renameDocument(doc.id, title); refreshLibrarySync() }

    /** Removes the library entry and READIT's private copy. The user's original file is untouched. */
    fun remove(doc: Document) = io {
        if (session.now.value.document?.id == doc.id) session.stop()
        lib.removeDocument(doc.id)
        refreshLibrarySync()
        _messages.tryEmit("Removed \"${doc.title}\" from READIT. Your original file was not changed.")
    }

    fun createCollection(name: String, add: Document? = null) = io {
        if (name.isNotBlank()) {
            val id = lib.createCollection(name)
            add?.let { lib.addToCollection(id, it.id) }
        }
        refreshLibrarySync()
    }
    fun addToCollection(collectionId: Long, doc: Document) = io { lib.addToCollection(collectionId, doc.id); refreshLibrarySync() }

    // --------------------------------------------------------- document view

    /** Sections are loaded in chunks so books with tens of thousands of paragraphs stay responsive. */
    val sectionChunks = mutableStateMapOf<Pair<String, Int>, List<Section>>()
    private val loadingChunks = mutableSetOf<Pair<String, Int>>()

    fun section(docId: String, ordinal: Int): Section? {
        val key = docId to ordinal / CHUNK
        val chunk = sectionChunks[key]
        if (chunk == null) {
            synchronized(loadingChunks) { if (!loadingChunks.add(key)) return null }
            io {
                val list = lib.sections(docId, (key.second * CHUNK).toUInt(), CHUNK.toUInt())
                withContext(Dispatchers.Main) { sectionChunks[key] = list }
                synchronized(loadingChunks) { loadingChunks.remove(key) }
            }
            return null
        }
        return chunk.getOrNull(ordinal % CHUNK)
    }

    suspend fun outline(docId: String): List<Section> = withContext(Dispatchers.IO) { lib.outline(docId) }
    suspend fun search(docId: String, q: String): List<Section> = withContext(Dispatchers.IO) { lib.search(docId, q) }
    suspend fun bookmarks(docId: String): List<Bookmark> = withContext(Dispatchers.IO) { lib.bookmarks(docId) }
    fun deleteBookmark(id: Long) = io { lib.deleteBookmark(id) }
    fun updateBookmarkNote(id: Long, note: String?) = io { lib.updateBookmarkNote(id, note) }

    // --------------------------------------------------------------- voices

    fun refreshVoices() = io { _installedVoices.value = lib.installedVoices() }

    fun loadSystemVoices() = io { _systemVoices.value = SystemTtsEngine.voices(getApplication()) }

    fun downloadPack(packId: String) {
        val pack = voiceManager.catalog.pack(packId) ?: return
        if (voiceManager.freeBytes() < voiceManager.bytesNeeded(pack) + 50_000_000) {
            _messages.tryEmit("Not enough free storage for ${pack.title}. It needs about ${voiceManager.bytesNeeded(pack) / 1_000_000} MB.")
            return
        }
        VoiceDownloadWorker.start(getApplication(), packId)
    }

    fun pauseDownload(packId: String) = VoiceDownloadWorker.pause(getApplication(), packId)

    fun removePack(packId: String) = io {
        val current = session.now.value.voiceId
        voiceManager.remove(packId)
        if (current != null && voiceManager.catalog.packOf(current)?.id == packId) {
            lib.setSetting("voice", "system:default")
            session.selectVoice("system:default")
        }
        _installedVoices.value = lib.installedVoices()
    }

    /** Measure how quickly a voice renders on this phone (see VoiceManager.benchmark). */
    fun benchmark(voiceId: String, done: (Double) -> Unit) = io {
        val rtf = voiceManager.benchmark(voiceId)
        withContext(Dispatchers.Main) { done(rtf) }
    }

    /** Speak a short sample with a voice, without touching the open document. */
    fun preview(voiceId: String, text: String = "Hello. This is how I will sound reading your documents.") = io {
        session.pause() // never talk over the document
        val engine = voiceManager.createEngine(voiceId)
        try {
            val spoken = lib.previewSpoken(text, null, readingOptions())
            val audio = engine.synthesize(spoken, session.now.value.speed, session.now.value.pitch)
            val sink = app.readit.playback.AudioTrackSink()
            sink.open(engine.sampleRate)
            sink.write(audio, 0, audio.size)
            Thread.sleep((audio.size * 1000L / engine.sampleRate) + 300)
            sink.close()
        } finally {
            engine.release()
        }
    }

    // ------------------------------------------------- settings and profiles

    fun setSetting(key: String, value: String) = io {
        lib.setSetting(key, value)
        refreshAll()
    }

    suspend fun profiles(): List<ReadingProfile> = withContext(Dispatchers.IO) { lib.readingProfiles() }

    fun saveProfile(p: ReadingProfile) = io {
        lib.saveReadingProfile(p)
        session.reloadOptions()
    }

    fun activateProfile(p: ReadingProfile) = io {
        lib.setSetting("active_profile", p.name)
        lib.setSetting("speed", p.speed.toString())
        p.voiceId?.let { session.selectVoice(it) }
        session.setSpeed(p.speed)
        session.reloadOptions()
        refreshAll()
    }

    suspend fun pronunciations(): List<PronunciationRule> = withContext(Dispatchers.IO) { lib.pronunciations(null) }
    fun addPronunciation(term: String, say: String, caseSensitive: Boolean) = io {
        lib.addPronunciation(term, say, caseSensitive, null)
        session.reloadOptions()
    }
    fun deletePronunciation(id: Long) = io { lib.deletePronunciation(id); session.reloadOptions() }
    suspend fun previewSpoken(text: String): String = withContext(Dispatchers.IO) { lib.previewSpoken(text, null, readingOptions()) }

    suspend fun storage(): StorageReport = withContext(Dispatchers.IO) { lib.storageReport() }
    fun clearAudioCache() = io {
        val freed = lib.clearAudioCache()
        _messages.tryEmit("Cleared ${freed / 1_000_000} MB of generated audio. Documents and voices were kept.")
    }

    fun eraseEverything() = io {
        session.stop()
        voiceManager.catalog.packs.forEach { voiceManager.remove(it.id) }
        lib.eraseAllData()
        refreshAll()
        _messages.tryEmit("All READIT data on this device has been erased.")
    }

    companion object {
        const val CHUNK = 200
    }
}
