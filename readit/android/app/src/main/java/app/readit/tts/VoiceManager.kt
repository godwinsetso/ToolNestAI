package app.readit.tts

import android.content.Context
import android.os.StatFs
import app.readit.core.InstalledVoice
import app.readit.core.ReaditLibrary
import app.readit.playback.SpeechEngine
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import org.apache.commons.compress.archivers.tar.TarArchiveInputStream
import org.apache.commons.compress.compressors.bzip2.BZip2CompressorInputStream
import java.io.BufferedInputStream
import java.io.File
import java.io.FileInputStream
import java.io.FileOutputStream
import java.io.IOException
import java.io.RandomAccessFile
import java.net.HttpURLConnection
import java.net.URL
import java.security.MessageDigest

sealed interface DownloadState {
    data object NotInstalled : DownloadState
    data class Downloading(val bytes: Long, val total: Long) : DownloadState
    data class Paused(val bytes: Long, val total: Long) : DownloadState
    data object Verifying : DownloadState
    data object Installing : DownloadState
    data object Installed : DownloadState
    data class Failed(val message: String, val bytes: Long) : DownloadState
}

/**
 * Downloads, verifies, installs and removes voice packs, and creates speech
 * engines. Downloads resume where they stopped, are checked against the
 * SHA-256 in the catalogue before use, and are only ever started by the user.
 */
class VoiceManager(private val context: Context, private val lib: ReaditLibrary, val catalog: VoiceCatalog) {
    private val root = File(context.filesDir, "readit/voices")
    private val partial = File(context.cacheDir, "voice-downloads").apply { mkdirs() }
    private val states = mutableMapOf<String, MutableStateFlow<DownloadState>>()
    @Volatile private var cancelRequested = mutableSetOf<String>()

    fun packDir(pack: VoicePack) = File(root, pack.archive)

    fun isInstalled(pack: VoicePack) = File(packDir(pack), ".installed").exists()

    fun state(packId: String): StateFlow<DownloadState> = synchronized(states) {
        states.getOrPut(packId) {
            val pack = catalog.pack(packId)
            val part = pack?.let { File(partial, it.archive + ".part") }
            MutableStateFlow(
                when {
                    pack == null -> DownloadState.NotInstalled
                    isInstalled(pack) -> DownloadState.Installed
                    part != null && part.exists() -> DownloadState.Paused(part.length(), pack.downloadBytes)
                    else -> DownloadState.NotInstalled
                },
            )
        }
    }

    private fun set(packId: String, s: DownloadState) {
        (state(packId) as MutableStateFlow).value = s
    }

    fun freeBytes(): Long = StatFs(context.filesDir.absolutePath).availableBytes

    /** Space needed to finish: the rest of the download plus the unpacked files. */
    fun bytesNeeded(pack: VoicePack): Long {
        val have = File(partial, pack.archive + ".part").takeIf { it.exists() }?.length() ?: 0
        return (pack.downloadBytes - have) + pack.installedBytes
    }

    fun requestPause(packId: String) = synchronized(cancelRequested) { cancelRequested += packId }

    /**
     * Blocking: download (resuming if possible), verify, extract and register.
     * Run from a background worker. Throws on failure after recording the state.
     */
    fun install(packId: String) {
        val pack = catalog.pack(packId) ?: error("Unknown voice pack $packId")
        synchronized(cancelRequested) { cancelRequested -= packId }
        if (isInstalled(pack)) {
            register(pack)
            set(packId, DownloadState.Installed)
            return
        }
        val part = File(partial, pack.archive + ".part")
        try {
            if (freeBytes() < bytesNeeded(pack) + 50_000_000) {
                throw IOException(
                    "Not enough free storage. This voice needs about ${mb(bytesNeeded(pack))} MB; " +
                        "${mb(freeBytes())} MB is free.",
                )
            }
            download(pack, part)
            if (synchronized(cancelRequested) { packId in cancelRequested }) {
                set(packId, DownloadState.Paused(part.length(), pack.downloadBytes))
                return
            }
            set(packId, DownloadState.Verifying)
            val hash = sha256(part)
            if (!hash.equals(pack.sha256, ignoreCase = true)) {
                part.delete()
                throw IOException("The download was damaged (checksum mismatch) and has been deleted. Please try again.")
            }
            set(packId, DownloadState.Installing)
            extract(part, root, pack.archive)
            File(packDir(pack), ".installed").writeText(pack.sha256)
            part.delete()
            register(pack)
            set(packId, DownloadState.Installed)
        } catch (e: Exception) {
            set(packId, DownloadState.Failed(e.message ?: "Download failed", part.takeIf { it.exists() }?.length() ?: 0))
            throw e
        }
    }

    private fun download(pack: VoicePack, part: File) {
        var url = URL(pack.url)
        var redirects = 0
        while (true) {
            val have = if (part.exists()) part.length() else 0L
            if (have >= pack.downloadBytes) return
            val conn = (url.openConnection() as HttpURLConnection).apply {
                instanceFollowRedirects = false
                connectTimeout = 20_000
                readTimeout = 30_000
                if (have > 0) setRequestProperty("Range", "bytes=$have-")
            }
            val code = conn.responseCode
            if (code in 300..399) {
                val loc = conn.getHeaderField("Location") ?: throw IOException("Bad redirect")
                conn.disconnect()
                // Only follow HTTPS redirects (GitHub release assets go to a CDN).
                url = URL(url, loc).also { require(it.protocol == "https") { "Refusing insecure redirect" } }
                if (++redirects > 5) throw IOException("Too many redirects")
                continue
            }
            if (code != 200 && code != 206) throw IOException("The server replied $code. Check your connection and try again.")
            val append = code == 206
            RandomAccessFile(part, "rw").use { out ->
                if (append) out.seek(have) else out.setLength(0)
                conn.inputStream.use { input ->
                    val buf = ByteArray(64 * 1024)
                    var done = if (append) have else 0L
                    var lastReport = 0L
                    while (true) {
                        if (synchronized(cancelRequested) { pack.id in cancelRequested }) return
                        val n = input.read(buf)
                        if (n < 0) break
                        out.write(buf, 0, n)
                        done += n
                        if (done - lastReport > 512 * 1024) {
                            set(pack.id, DownloadState.Downloading(done, pack.downloadBytes))
                            lastReport = done
                        }
                    }
                }
            }
            conn.disconnect()
            return
        }
    }

    private fun sha256(f: File): String {
        val md = MessageDigest.getInstance("SHA-256")
        FileInputStream(f).use { s ->
            val buf = ByteArray(1 shl 16)
            while (true) {
                val n = s.read(buf)
                if (n < 0) break
                md.update(buf, 0, n)
            }
        }
        return md.digest().joinToString("") { "%02x".format(it) }
    }

    /** Extract a .tar.bz2, refusing any entry that would land outside the pack's folder. */
    private fun extract(archive: File, into: File, expectedTop: String) {
        val staging = File(into, ".staging-$expectedTop").apply { deleteRecursively(); mkdirs() }
        val canonicalRoot = staging.canonicalPath + File.separator
        TarArchiveInputStream(BZip2CompressorInputStream(BufferedInputStream(FileInputStream(archive)))).use { tar ->
            while (true) {
                val e = tar.nextEntry ?: break
                val out = File(staging, e.name)
                if (!out.canonicalPath.startsWith(canonicalRoot)) throw IOException("Unsafe path in voice archive: ${e.name}")
                if (e.isSymbolicLink || e.isLink) continue
                if (e.isDirectory) { out.mkdirs(); continue }
                out.parentFile?.mkdirs()
                FileOutputStream(out).use { tar.copyTo(it) }
            }
        }
        val extracted = File(staging, expectedTop)
        if (!extracted.isDirectory) throw IOException("The voice archive has an unexpected layout.")
        val target = File(into, expectedTop)
        target.deleteRecursively()
        if (!extracted.renameTo(target)) throw IOException("Could not install the voice files.")
        staging.deleteRecursively()
    }

    private fun register(pack: VoicePack) {
        val dir = packDir(pack)
        val size = dir.walkTopDown().filter { it.isFile }.sumOf { it.length() }
        val now = System.currentTimeMillis() / 1000
        for (v in pack.voices) {
            lib.registerVoice(
                InstalledVoice(
                    id = v.id, engine = pack.engine, name = v.name, language = v.language, accent = v.accent,
                    gender = v.gender, quality = pack.quality, licence = pack.licence,
                    sizeBytes = size.toULong(), offline = true, path = dir.absolutePath, installedAt = now,
                ),
            )
        }
    }

    /** Delete a pack's files and its voices. Documents are never touched. */
    fun remove(packId: String) {
        val pack = catalog.pack(packId) ?: return
        pack.voices.forEach { runCatching { lib.removeVoice(it.id) } }
        packDir(pack).deleteRecursively()
        File(partial, pack.archive + ".part").delete()
        set(packId, DownloadState.NotInstalled)
    }

    /** Create an engine for a voice id: "kokoro:…", "piper:…" or "system:…". Blocking. */
    fun createEngine(voiceId: String): SpeechEngine {
        if (voiceId.startsWith("system:")) {
            return SystemTtsEngine.create(context, voiceId.removePrefix("system:").takeIf { it != "default" })
        }
        val voice = catalog.voice(voiceId) ?: error("Unknown voice")
        val pack = catalog.pack(voice.packId)!!
        if (!isInstalled(pack)) error("${voice.name} is not downloaded yet.")
        val threads = Runtime.getRuntime().availableProcessors().coerceIn(1, 4)
        return SherpaEngine(packDir(pack), pack, voice, threads)
    }

    /**
     * Measures how fast a voice speaks on this device. A real-time factor below
     * 1.0 means it renders faster than it plays. Stored so the voice screen can
     * say honestly whether a voice will keep up on this phone.
     */
    fun benchmark(voiceId: String): Double {
        val engine = createEngine(voiceId)
        try {
            val text = "Financial institutions must verify the identity of every customer before opening an account."
            engine.synthesize("Warming up.", 1f, 1f)
            val t = System.nanoTime()
            val audio = engine.synthesize(text, 1f, 1f)
            val secs = (System.nanoTime() - t) / 1e9
            val rtf = secs / (audio.size.toDouble() / engine.sampleRate)
            lib.setSetting("rtf:$voiceId", "%.3f".format(rtf))
            return rtf
        } finally {
            engine.release()
        }
    }

    fun measuredRtf(voiceId: String): Double? = lib.setting("rtf:$voiceId")?.toDoubleOrNull()

    private fun mb(b: Long) = (b / 1_000_000).coerceAtLeast(1)
}
