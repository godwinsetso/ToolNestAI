package app.readit.tts

import android.content.Context
import org.json.JSONObject

/** One downloadable voice pack (a model file that may contain several speakers). */
data class VoicePack(
    val id: String,
    val engine: String, // "kokoro" or "piper"
    val title: String,
    val archive: String,
    val url: String,
    val sha256: String,
    val downloadBytes: Long,
    val installedBytes: Long,
    val licence: String,
    val licenceUrl: String,
    val quality: String,
    val requirements: String,
    val voices: List<VoiceInfo>,
)

data class VoiceInfo(
    val id: String,
    val packId: String,
    val speaker: Int,
    val name: String,
    val language: String,
    val accent: String,
    val gender: String,
    /** Kokoro: which pronunciation dictionary to load. */
    val lexicon: String? = null,
    /** Piper: model file inside the pack. */
    val model: String? = null,
)

/** The voices READIT knows how to download, from assets/voices.json. */
class VoiceCatalog(val packs: List<VoicePack>) {
    fun pack(id: String) = packs.firstOrNull { it.id == id }
    fun voice(id: String) = packs.asSequence().flatMap { it.voices }.firstOrNull { it.id == id }
    fun packOf(voiceId: String) = packs.firstOrNull { p -> p.voices.any { it.id == voiceId } }

    companion object {
        fun load(context: Context): VoiceCatalog =
            parse(context.assets.open("voices.json").bufferedReader().use { it.readText() })

        fun parse(json: String): VoiceCatalog {
            val root = JSONObject(json)
            val arr = root.getJSONArray("packs")
            val packs = (0 until arr.length()).map { i ->
                val p = arr.getJSONObject(i)
                val packId = p.getString("id")
                val vs = p.getJSONArray("voices")
                VoicePack(
                    id = packId,
                    engine = p.getString("engine"),
                    title = p.getString("title"),
                    archive = p.getString("archive"),
                    url = p.getString("url"),
                    sha256 = p.getString("sha256"),
                    downloadBytes = p.getLong("downloadBytes"),
                    installedBytes = p.getLong("installedBytes"),
                    licence = p.getString("licence"),
                    licenceUrl = p.optString("licenceUrl"),
                    quality = p.getString("quality"),
                    requirements = p.getString("requirements"),
                    voices = (0 until vs.length()).map { j ->
                        val v = vs.getJSONObject(j)
                        VoiceInfo(
                            id = v.getString("id"),
                            packId = packId,
                            speaker = v.getInt("speaker"),
                            name = v.getString("name"),
                            language = v.getString("language"),
                            accent = v.getString("accent"),
                            gender = v.getString("gender"),
                            lexicon = v.optString("lexicon").ifEmpty { null },
                            model = v.optString("model").ifEmpty { null },
                        )
                    },
                )
            }
            return VoiceCatalog(packs)
        }
    }
}
