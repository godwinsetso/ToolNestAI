package app.readit

import android.app.Application
import app.readit.core.ReaditLibrary
import app.readit.playback.ReaderSession
import app.readit.tts.VoiceCatalog
import app.readit.tts.VoiceManager
import java.io.File

/** Owns the long-lived objects: the local library, the voices and the reading session. */
class ReaditApp : Application() {
    lateinit var library: ReaditLibrary
        private set
    lateinit var voices: VoiceManager
        private set
    lateinit var session: ReaderSession
        private set

    override fun onCreate() {
        super.onCreate()
        instance = this
        val dataDir = File(filesDir, "readit").apply { mkdirs() }
        // libpdfium.so ships inside the APK and is extracted to nativeLibraryDir.
        library = ReaditLibrary(dataDir.absolutePath, applicationInfo.nativeLibraryDir)
        voices = VoiceManager(this, library, VoiceCatalog.load(this))
        session = ReaderSession(this, library, voices)
    }

    companion object {
        lateinit var instance: ReaditApp
            private set
    }
}
