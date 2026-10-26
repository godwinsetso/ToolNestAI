package app.readit.tts

import android.content.Context
import android.speech.tts.TextToSpeech
import android.speech.tts.UtteranceProgressListener
import android.speech.tts.Voice
import app.readit.playback.SpeechEngine
import java.io.File
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.util.Locale
import java.util.UUID
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

/**
 * The phone's own text-to-speech service, used as READIT's always-available
 * fallback. Audio is rendered to a temporary file and fed through the same
 * player as the neural voices, so highlighting and navigation behave identically.
 *
 * Some system voices need the internet; [SystemVoice.needsNetwork] says which,
 * and READIT labels them so they are never mistaken for offline voices.
 */
class SystemTtsEngine private constructor(
    private val context: Context,
    private val tts: TextToSpeech,
    private val voice: Voice?,
) : SpeechEngine {
    override val voiceId: String = "system:" + (voice?.name ?: "default")
    override val supportsPitch = true
    /** Everything is resampled to one rate so the audio output never needs reopening. */
    override val sampleRate = OUTPUT_RATE

    override fun synthesize(text: String, speed: Float, pitch: Float): FloatArray {
        if (text.isBlank()) return FloatArray(0)
        val file = File.createTempFile("tts", ".wav", context.cacheDir)
        try {
            val id = UUID.randomUUID().toString()
            val done = CountDownLatch(1)
            var failed: String? = null
            tts.setOnUtteranceProgressListener(object : UtteranceProgressListener() {
                override fun onStart(utteranceId: String?) {}
                override fun onDone(utteranceId: String?) { if (utteranceId == id) done.countDown() }
                @Deprecated("Deprecated in Java")
                override fun onError(utteranceId: String?) { if (utteranceId == id) { failed = "error"; done.countDown() } }
                override fun onError(utteranceId: String?, errorCode: Int) {
                    if (utteranceId == id) { failed = "error $errorCode"; done.countDown() }
                }
            })
            synchronized(tts) {
                tts.setSpeechRate(speed)
                tts.setPitch(pitch)
                voice?.let { tts.voice = it }
                val r = tts.synthesizeToFile(text, null, file, id)
                check(r == TextToSpeech.SUCCESS) { "the system voice refused the text" }
            }
            if (!done.await(60, TimeUnit.SECONDS)) error("the system voice timed out")
            failed?.let { error("the system voice failed ($it)") }
            return resample(readWav(file.readBytes()))
        } finally {
            file.delete()
        }
    }

    override fun release() = tts.shutdown()

    private class Pcm(val samples: FloatArray, val rate: Int)

    private fun readWav(bytes: ByteArray): Pcm {
        val b = ByteBuffer.wrap(bytes).order(ByteOrder.LITTLE_ENDIAN)
        require(bytes.size > 44 && String(bytes, 0, 4) == "RIFF") { "the system voice produced no audio" }
        var rate = 22050
        var bits = 16
        var channels = 1
        var pos = 12
        while (pos + 8 <= bytes.size) {
            val id = String(bytes, pos, 4)
            val size = b.getInt(pos + 4)
            if (id == "fmt ") {
                channels = b.getShort(pos + 10).toInt()
                rate = b.getInt(pos + 12)
                bits = b.getShort(pos + 22).toInt()
            } else if (id == "data") {
                val start = pos + 8
                val len = minOf(size.takeIf { it > 0 } ?: (bytes.size - start), bytes.size - start)
                require(bits == 16) { "unsupported system voice audio format" }
                val frames = len / 2 / channels
                val out = FloatArray(frames)
                for (i in 0 until frames) out[i] = b.getShort(start + i * 2 * channels) / 32768f
                return Pcm(out, rate)
            }
            pos += 8 + size + (size and 1)
        }
        error("the system voice produced no audio")
    }

    private fun resample(p: Pcm): FloatArray {
        if (p.rate == OUTPUT_RATE || p.samples.isEmpty()) return p.samples
        val ratio = p.rate.toDouble() / OUTPUT_RATE
        val n = (p.samples.size / ratio).toInt()
        return FloatArray(n) { i ->
            val x = i * ratio
            val j = x.toInt()
            val f = (x - j).toFloat()
            val a = p.samples[j]
            val c = p.samples[minOf(j + 1, p.samples.size - 1)]
            a + (c - a) * f
        }
    }

    data class SystemVoice(val name: String, val locale: Locale, val needsNetwork: Boolean, val quality: Int)

    companion object {
        const val OUTPUT_RATE = 24000

        /** Connects to the system TTS service. Blocking: call off the main thread. */
        fun create(context: Context, voiceName: String?): SystemTtsEngine {
            val ready = CountDownLatch(1)
            var status = TextToSpeech.ERROR
            lateinit var tts: TextToSpeech
            tts = TextToSpeech(context.applicationContext) { s -> status = s; ready.countDown() }
            if (!ready.await(15, TimeUnit.SECONDS) || status != TextToSpeech.SUCCESS) {
                tts.shutdown()
                error("No text-to-speech service is installed on this device.")
            }
            val voice = voiceName?.let { n -> tts.voices?.firstOrNull { it.name == n } }
                ?: tts.voices?.filter { !it.isNetworkConnectionRequired && it.locale.language == "en" }
                    ?.maxByOrNull { it.quality }
            return SystemTtsEngine(context.applicationContext, tts, voice)
        }

        fun voices(context: Context): List<SystemVoice> {
            val e = runCatching { create(context, null) }.getOrNull() ?: return emptyList()
            return try {
                e.tts.voices.orEmpty()
                    .filter { "notInstalled" !in it.features.orEmpty() }
                    .map { SystemVoice(it.name, it.locale, it.isNetworkConnectionRequired, it.quality) }
                    .sortedWith(compareBy({ it.locale.language != "en" }, { it.needsNetwork }, { -it.quality }))
            } finally {
                e.release()
            }
        }
    }
}
