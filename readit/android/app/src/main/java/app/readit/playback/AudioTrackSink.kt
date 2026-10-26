package app.readit.playback

import android.media.AudioAttributes
import android.media.AudioFormat
import android.media.AudioTrack

/** Streams float PCM to the device speaker, headphones or Bluetooth through AudioTrack. */
class AudioTrackSink : AudioSink {
    @Volatile private var track: AudioTrack? = null
    @Volatile private var volume = 1f

    @Synchronized
    override fun open(sampleRate: Int) {
        track?.release()
        val min = AudioTrack.getMinBufferSize(sampleRate, AudioFormat.CHANNEL_OUT_MONO, AudioFormat.ENCODING_PCM_FLOAT)
        track = AudioTrack.Builder()
            .setAudioAttributes(
                AudioAttributes.Builder()
                    .setUsage(AudioAttributes.USAGE_MEDIA)
                    .setContentType(AudioAttributes.CONTENT_TYPE_SPEECH)
                    .build(),
            )
            .setAudioFormat(
                AudioFormat.Builder()
                    .setEncoding(AudioFormat.ENCODING_PCM_FLOAT)
                    .setSampleRate(sampleRate)
                    .setChannelMask(AudioFormat.CHANNEL_OUT_MONO)
                    .build(),
            )
            // About a quarter of a second: short enough that pause and skip feel immediate.
            .setBufferSizeInBytes(maxOf(min, sampleRate / 4 * 4))
            .setTransferMode(AudioTrack.MODE_STREAM)
            .build()
            .also {
                it.setVolume(volume)
                it.play()
            }
    }

    override fun write(samples: FloatArray, offset: Int, length: Int) {
        val t = track ?: return
        var done = 0
        while (done < length) {
            val n = t.write(samples, offset + done, length - done, AudioTrack.WRITE_BLOCKING)
            if (n <= 0) {
                // Track was paused or flushed under us; the controller decides what happens next.
                if (t.playState != AudioTrack.PLAYSTATE_PLAYING) Thread.sleep(20) else return
            } else {
                done += n
            }
        }
    }

    override fun pause() { runCatching { track?.pause() } }
    override fun resume() { runCatching { track?.play() } }
    override fun flush() {
        val t = track ?: return
        runCatching {
            val playing = t.playState == AudioTrack.PLAYSTATE_PLAYING
            t.pause()
            t.flush()
            if (playing) t.play()
        }
    }

    /** Used for ducking under navigation prompts and notifications. */
    fun setVolume(v: Float) {
        volume = v
        runCatching { track?.setVolume(v) }
    }

    @Synchronized
    override fun close() {
        track?.let { runCatching { it.stop() }; it.release() }
        track = null
    }
}
