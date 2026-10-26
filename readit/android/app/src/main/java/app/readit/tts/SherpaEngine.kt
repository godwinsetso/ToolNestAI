package app.readit.tts

import app.readit.playback.SpeechEngine
import com.k2fsa.sherpa.onnx.OfflineTts
import com.k2fsa.sherpa.onnx.OfflineTtsConfig
import com.k2fsa.sherpa.onnx.OfflineTtsKokoroModelConfig
import com.k2fsa.sherpa.onnx.OfflineTtsModelConfig
import com.k2fsa.sherpa.onnx.OfflineTtsVitsModelConfig
import java.io.File

/**
 * Offline neural speech through sherpa-onnx: Kokoro (natural, heavier) or
 * Piper (clear, fast). Everything runs on the device; nothing is sent anywhere.
 * Kokoro and Piper do not support pitch changes, so pitch is not offered for them.
 */
class SherpaEngine(packDir: File, private val pack: VoicePack, private val voice: VoiceInfo, threads: Int) : SpeechEngine {
    override val voiceId: String = voice.id
    override val supportsPitch = false
    private val tts: OfflineTts

    init {
        val dataDir = File(packDir, "espeak-ng-data").absolutePath
        val tokens = File(packDir, "tokens.txt").absolutePath
        val model = when (pack.engine) {
            "kokoro" -> OfflineTtsModelConfig(
                kokoro = OfflineTtsKokoroModelConfig(
                    model = File(packDir, "model.int8.onnx").absolutePath,
                    voices = File(packDir, "voices.bin").absolutePath,
                    tokens = tokens,
                    dataDir = dataDir,
                    lexicon = voice.lexicon?.let { File(packDir, it).absolutePath } ?: "",
                ),
                numThreads = threads,
                debug = false,
            )
            else -> OfflineTtsModelConfig(
                vits = OfflineTtsVitsModelConfig(
                    model = File(packDir, voice.model ?: error("Piper voice without a model file")).absolutePath,
                    tokens = tokens,
                    dataDir = dataDir,
                ),
                numThreads = threads,
                debug = false,
            )
        }
        tts = OfflineTts(assetManager = null, config = OfflineTtsConfig(model = model))
    }

    override val sampleRate: Int = tts.sampleRate()

    override fun synthesize(text: String, speed: Float, pitch: Float): FloatArray {
        if (text.isBlank()) return FloatArray(0)
        return tts.generate(text = text, sid = voice.speaker, speed = speed).samples
    }

    override fun release() = tts.release()
}
