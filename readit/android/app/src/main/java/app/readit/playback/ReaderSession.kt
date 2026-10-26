package app.readit.playback

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.media.AudioAttributes
import android.media.AudioFocusRequest
import android.media.AudioManager
import android.os.Handler
import android.os.Looper
import androidx.core.content.ContextCompat
import app.readit.core.Document
import app.readit.core.LocationKind
import app.readit.core.ReaditLibrary
import app.readit.core.Section
import app.readit.core.Segment
import app.readit.data.CoreDocumentSource
import app.readit.data.readingOptions
import app.readit.tts.VoiceManager
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.update
import java.util.concurrent.Executors

data class NowPlaying(
    val document: Document? = null,
    val state: PlayState = PlayState.IDLE,
    val section: Section? = null,
    val segment: Segment? = null,
    val voiceId: String? = null,
    val voiceLoading: Boolean = false,
    val speed: Float = 1.0f,
    val pitch: Float = 1.0f,
    val pitchSupported: Boolean = false,
    val sleepEndsAtMs: Long? = null,
    val sleepAtEndOfLocation: Boolean = false,
    val message: String? = null,
) {
    val isPlaying get() = state == PlayState.PLAYING || state == PlayState.PREPARING
    val locationLabel: String
        get() {
            val d = document ?: return ""
            val loc = section?.location ?: return ""
            return when (d.locationKind) {
                LocationKind.PAGE -> "Page $loc of ${d.locationCount}"
                LocationKind.SLIDE -> "Slide $loc of ${d.locationCount}"
                LocationKind.CHAPTER -> "Chapter $loc of ${d.locationCount}"
                LocationKind.NONE -> if (d.sectionCount > 0u) "${((section!!.ordinal.toLong() + 1) * 100 / d.sectionCount.toLong())}%" else ""
            }
        }
}

/**
 * The one reading session in the app. The UI, the notification and the lock
 * screen all drive this object, so they always agree on what is playing.
 * Also handles audio focus (calls, other apps) and headphone unplugging.
 */
class ReaderSession(
    private val context: Context,
    private val lib: ReaditLibrary,
    private val voices: VoiceManager,
) {
    private val main = Handler(Looper.getMainLooper())
    private val worker = Executors.newSingleThreadExecutor { r -> Thread(r, "readit-session").apply { isDaemon = true } }
    private val sink = AudioTrackSink()
    private val _now = MutableStateFlow(NowPlaying())
    val now: StateFlow<NowPlaying> = _now
    private val changeListeners = mutableListOf<() -> Unit>()

    private val listener = object : ReaderListener {
        override fun onStateChanged(state: PlayState) {
            post { _now.update { it.copy(state = state) } }
        }
        override fun onSegmentStarted(section: Section, segment: Segment) {
            post { _now.update { it.copy(section = section, segment = segment) } }
        }
        override fun onError(message: String) {
            post { _now.update { it.copy(message = message) } }
        }
        override fun onSleepTimerFinished() {
            post { _now.update { it.copy(sleepEndsAtMs = null, sleepAtEndOfLocation = false, message = "Sleep timer: reading paused.") } }
        }
    }
    private val controller = ReaderController(CoreDocumentSource(lib), listener)

    private fun post(block: () -> Unit) {
        main.post {
            block()
            changeListeners.toList().forEach { it() }
        }
    }

    /** The media service registers here to refresh the notification. */
    fun addChangeListener(l: () -> Unit) { changeListeners += l }
    fun removeChangeListener(l: () -> Unit) { changeListeners -= l }

    fun consumeMessage() = _now.update { it.copy(message = null) }

    // ------------------------------------------------------------ documents

    /** Open a document at its saved position. */
    fun open(documentId: String, autoplay: Boolean = false) {
        worker.execute {
            try {
                val doc = lib.document(documentId)
                val pos = lib.openDocument(documentId)
                val profile = activeProfile()
                controller.load(documentId, readingOptions(profile), pos?.let { ReaderPosition(it.sectionOrdinal, it.segmentIndex) })
                val first = controller.current?.let { lib.sections(documentId, it.section, 1u).firstOrNull() }
                post { _now.update { it.copy(document = doc, section = first, segment = null, state = PlayState.IDLE) } }
                ensureVoice()
                if (autoplay) play()
            } catch (e: Exception) {
                post { _now.update { it.copy(message = e.message ?: "Could not open the document.") } }
            }
        }
    }

    /** Re-read the profile and reading options (after settings change). */
    fun reloadOptions(): Unit = worker.execute { controller.setOptions(readingOptions(activeProfile())) }

    private fun activeProfile() = runCatching {
        val name = lib.setting("active_profile") ?: "My Reader"
        lib.readingProfiles().firstOrNull { it.name == name }
    }.getOrNull()

    // --------------------------------------------------------------- voices

    /** The voice to use: the saved choice, else the best installed neural voice, else the system voice. */
    fun preferredVoiceId(): String {
        lib.setting("voice")?.let { return it }
        val installed = lib.installedVoices().map { it.id }
        return listOf("kokoro:bf_emma", "piper:alba", "piper:cori").firstOrNull { it in installed }
            ?: installed.firstOrNull() ?: "system:default"
    }

    private fun ensureVoice() {
        if (_now.value.voiceId == null) selectVoiceBlocking(preferredVoiceId())
    }

    fun selectVoice(voiceId: String): Unit = worker.execute { selectVoiceBlocking(voiceId) }

    private fun selectVoiceBlocking(voiceId: String) {
        post { _now.update { it.copy(voiceLoading = true) } }
        try {
            val engine = voices.createEngine(voiceId)
            controller.setOutput(engine, sink)
            lib.setSetting("voice", voiceId)
            val speed = lib.setting("speed")?.toFloatOrNull() ?: activeProfile()?.speed ?: 1f
            controller.setSpeed(speed)
            post {
                _now.update {
                    it.copy(voiceId = voiceId, voiceLoading = false, speed = controller.currentSpeed, pitchSupported = engine.supportsPitch)
                }
            }
        } catch (e: Exception) {
            // Fall back to the system voice rather than leaving the user with silence.
            if (!voiceId.startsWith("system:")) {
                post { _now.update { it.copy(message = "${e.message} Using the phone's built-in voice instead.") } }
                selectVoiceBlocking("system:default")
            } else {
                post { _now.update { it.copy(voiceLoading = false, message = e.message ?: "No voice is available.") } }
            }
        }
    }

    // ------------------------------------------------------------ transport

    private val audioManager = context.getSystemService(AudioManager::class.java)
    private var resumeOnFocusGain = false
    private val focusRequest = AudioFocusRequest.Builder(AudioManager.AUDIOFOCUS_GAIN)
        .setAudioAttributes(
            AudioAttributes.Builder().setUsage(AudioAttributes.USAGE_MEDIA).setContentType(AudioAttributes.CONTENT_TYPE_SPEECH).build(),
        )
        .setWillPauseWhenDucked(false)
        .setOnAudioFocusChangeListener({ change ->
            when (change) {
                AudioManager.AUDIOFOCUS_LOSS -> { resumeOnFocusGain = false; pause() }
                AudioManager.AUDIOFOCUS_LOSS_TRANSIENT -> {
                    // Phone call or voice assistant: pause, and pick up again afterwards.
                    resumeOnFocusGain = _now.value.isPlaying
                    pause()
                }
                AudioManager.AUDIOFOCUS_LOSS_TRANSIENT_CAN_DUCK -> sink.setVolume(0.25f)
                AudioManager.AUDIOFOCUS_GAIN -> {
                    sink.setVolume(1f)
                    if (resumeOnFocusGain) { resumeOnFocusGain = false; play() }
                }
            }
        }, main)
        .build()

    private val noisy = object : BroadcastReceiver() {
        override fun onReceive(c: Context?, i: Intent?) {
            if (i?.action == AudioManager.ACTION_AUDIO_BECOMING_NOISY) pause() // headphones unplugged
        }
    }
    private var noisyRegistered = false

    fun play() {
        main.post { playOnMain() }
    }

    private fun playOnMain() {
        if (_now.value.document == null) return
        if (audioManager.requestAudioFocus(focusRequest) != AudioManager.AUDIOFOCUS_REQUEST_GRANTED) {
            _now.update { it.copy(message = "Another app is using audio. Try again in a moment.") }
            return
        }
        if (!noisyRegistered) {
            ContextCompat.registerReceiver(context, noisy, IntentFilter(AudioManager.ACTION_AUDIO_BECOMING_NOISY), ContextCompat.RECEIVER_NOT_EXPORTED)
            noisyRegistered = true
        }
        worker.execute { controller.play() }
    }

    fun pause(): Unit = worker.execute { controller.pause() }
    fun toggle() {
        if (_now.value.isPlaying) pause() else play()
    }

    fun stop() {
        worker.execute { controller.stop() }
        audioManager.abandonAudioFocusRequest(focusRequest)
        if (noisyRegistered) {
            runCatching { context.unregisterReceiver(noisy) }
            noisyRegistered = false
        }
    }

    fun nextParagraph(): Unit = worker.execute { controller.nextParagraph() }
    fun previousParagraph(): Unit = worker.execute { controller.previousParagraph() }
    fun nextLocation(): Unit = worker.execute { controller.nextLocation() }
    fun previousLocation(): Unit = worker.execute { controller.previousLocation() }
    fun goToLocation(loc: UInt): Unit = worker.execute { controller.goToLocation(loc) }
    fun seekToSection(ordinal: UInt, segment: UInt = 0u): Unit = worker.execute { controller.seekTo(ReaderPosition(ordinal, segment)) }
    fun restartSentence(): Unit = worker.execute { controller.restartSegment() }

    fun skipSeconds(seconds: Int): Unit = worker.execute { controller.skipSeconds(seconds) }
    fun skipIntervalSeconds(): Int = lib.setting("skip_seconds")?.toIntOrNull() ?: 15

    fun setSpeed(speed: Float): Unit = worker.execute {
        controller.setSpeed(speed)
        lib.setSetting("speed", controller.currentSpeed.toString())
        post { _now.update { it.copy(speed = controller.currentSpeed) } }
    }

    fun setPitch(pitch: Float): Unit = worker.execute {
        controller.setPitch(pitch)
        post { _now.update { it.copy(pitch = pitch) } }
    }

    fun setSleepTimer(minutes: Int?): Unit = worker.execute {
        controller.setSleepTimer(minutes)
        post { _now.update { it.copy(sleepEndsAtMs = minutes?.let { m -> System.currentTimeMillis() + m * 60_000L }, sleepAtEndOfLocation = false) } }
    }

    fun sleepAtEndOfLocation(): Unit = worker.execute {
        controller.sleepAtEndOfCurrentLocation()
        post { _now.update { it.copy(sleepEndsAtMs = null, sleepAtEndOfLocation = true) } }
    }

    /** Add a bookmark at the sentence being read. */
    fun bookmarkHere(note: String? = null): Boolean {
        val n = _now.value
        val doc = n.document ?: return false
        val sec = n.section ?: return false
        val label = n.segment?.displayText?.take(80) ?: sec.text.take(80)
        worker.execute {
            lib.addBookmark(doc.id, sec.ordinal, n.segment?.index ?: 0u, label, note, n.segment?.displayText)
            post { _now.update { it.copy(message = "Bookmark added") } }
        }
        return true
    }
}
