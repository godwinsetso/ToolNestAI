package app.readit.playback

import app.readit.core.ReadingOptions
import app.readit.core.Section
import app.readit.core.Segment

/**
 * A speech engine turns one segment of text into PCM audio.
 * Implementations: [app.readit.tts.SherpaEngine] (Kokoro, Piper) and
 * [app.readit.tts.SystemTtsEngine] (Android's installed TTS, the fallback).
 */
interface SpeechEngine {
    val voiceId: String
    val sampleRate: Int
    /** Pitch control is only offered when the engine really supports it. */
    val supportsPitch: Boolean
    val minSpeed: Float get() = 0.5f
    val maxSpeed: Float get() = 3.0f

    /** Blocking. Returns mono float samples in [-1, 1] at [sampleRate]. */
    fun synthesize(text: String, speed: Float, pitch: Float): FloatArray
    fun release() {}
}

/** Where audio goes: an AudioTrack on Android, a recorder in tests. */
interface AudioSink {
    fun open(sampleRate: Int)
    /** Blocking write; returns when the samples are queued for playback. */
    fun write(samples: FloatArray, offset: Int, length: Int)
    fun pause()
    fun resume()
    /** Drop anything queued but not yet heard. */
    fun flush()
    fun close()
}

/** The subset of the core library the reader needs. Lets the controller be tested on a plain JVM. */
interface DocumentSource {
    fun segments(documentId: String, ordinal: UInt, options: ReadingOptions): List<Segment>
    fun nextReadable(documentId: String, fromOrdinal: UInt, forward: Boolean, options: ReadingOptions): UInt?
    fun sectionAtLocation(documentId: String, location: UInt): UInt?
    fun section(documentId: String, ordinal: UInt): Section?
    fun savePosition(documentId: String, sectionOrdinal: UInt, segmentIndex: UInt)
    fun logListening(documentId: String, seconds: UInt)
}

enum class PlayState { IDLE, PREPARING, PLAYING, PAUSED, ENDED }

data class ReaderPosition(val section: UInt, val segment: UInt)

interface ReaderListener {
    fun onStateChanged(state: PlayState) {}
    /** Fired as each segment starts sounding: drives highlighting and progress. */
    fun onSegmentStarted(section: Section, segment: Segment) {}
    fun onError(message: String) {}
    fun onSleepTimerFinished() {}
}
