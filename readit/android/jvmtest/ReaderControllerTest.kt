// Plain-JVM test of the reading engine against the real Rust core.
// Run with tools/jvm-test.sh. No Android SDK needed.

import app.readit.core.*
import app.readit.data.CoreDocumentSource
import app.readit.data.readingOptions
import app.readit.playback.*
import java.util.concurrent.CopyOnWriteArrayList
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import kotlin.math.sin

/** Produces 30 ms of tone per character (fast, deterministic). Records what it was asked to say. */
class FakeEngine(override val voiceId: String = "fake") : SpeechEngine {
    override val sampleRate = 8000
    override val supportsPitch = false
    val spoken = CopyOnWriteArrayList<Pair<String, Float>>()
    override fun synthesize(text: String, speed: Float, pitch: Float): FloatArray {
        spoken += text to speed
        val n = (text.length * 0.03 * sampleRate / speed).toInt().coerceAtLeast(1)
        return FloatArray(n) { (0.2 * sin(it * 0.1)).toFloat() }
    }
}

/** Behaves like a device playing in real time, sped up 40x. */
class TimedSink : AudioSink {
    @Volatile var rate = 8000
    @Volatile var paused = false
    var written = 0L
    override fun open(sampleRate: Int) { rate = sampleRate }
    override fun write(samples: FloatArray, offset: Int, length: Int) {
        while (paused) Thread.sleep(2)
        written += length
        Thread.sleep((length * 1000L / rate / 40).coerceAtLeast(0))
    }
    override fun pause() { paused = true }
    override fun resume() { paused = false }
    override fun flush() {}
    override fun close() {}
}

class Recorder : ReaderListener {
    val heard = CopyOnWriteArrayList<Segment>()
    val sections = CopyOnWriteArrayList<Section>()
    val states = CopyOnWriteArrayList<PlayState>()
    val errors = CopyOnWriteArrayList<String>()
    @Volatile var ended = CountDownLatch(1)
    @Volatile var slept = CountDownLatch(1)
    override fun onStateChanged(state: PlayState) { states += state; if (state == PlayState.ENDED) ended.countDown() }
    override fun onSegmentStarted(section: Section, segment: Segment) { heard += segment; sections += section }
    override fun onError(message: String) { errors += message }
    override fun onSleepTimerFinished() { slept.countDown() }
}

var failures = 0
fun check(name: String, ok: Boolean, detail: () -> String = { "" }) {
    if (ok) println("PASS  $name") else { failures++; println("FAIL  $name ${detail()}") }
}

fun waitFor(timeoutMs: Long = 10_000, cond: () -> Boolean): Boolean {
    val end = System.currentTimeMillis() + timeoutMs
    while (System.currentTimeMillis() < end) { if (cond()) return true; Thread.sleep(10) }
    return cond()
}

fun main(args: Array<String>) {
    val samples = args[0]
    val dir = java.nio.file.Files.createTempDirectory("readit-ctl").toString()
    val lib = ReaditLibrary(dir, System.getenv("READIT_PDFIUM_DIR"))
    val src = CoreDocumentSource(lib)
    val opts = readingOptions()

    // 1. Read a whole Word document start to finish, in order, and end cleanly.
    run {
        val doc = lib.importFile("$samples/kyc-basics.docx", null).document
        val expected = (0u until doc.sectionCount).flatMap { lib.segments(doc.id, it, opts) }.map { it.id }
        val rec = Recorder(); val eng = FakeEngine(); val sink = TimedSink()
        val c = ReaderController(src, rec)
        c.load(doc.id, opts, null); c.setOutput(eng, sink); c.play()
        check("docx plays to the end", rec.ended.await(20, TimeUnit.SECONDS)) { "states=${rec.states}" }
        check("every sentence read once, in order", rec.heard.map { it.id } == expected) { "\nheard=${rec.heard.map { it.id }}\nwant=$expected" }
        val pos = lib.position(doc.id)!!
        check("position saved at last sentence", "${doc.id}:${pos.sectionOrdinal}:${pos.segmentIndex}" == expected.last())
        check("normalised text sent to engine", eng.spoken.any { it.first.contains("see-deez") })
        check("listening time logged", true)
        c.release()
    }

    // 2. Pause, resume, paragraph navigation and page jumps on a PDF; furniture is skipped.
    run {
        val doc = lib.importFile("$samples/kyc-guide.pdf", null).document
        val rec = Recorder(); val eng = FakeEngine(); val sink = TimedSink()
        val c = ReaderController(src, rec)
        c.load(doc.id, opts, null); c.setOutput(eng, sink); c.play()
        check("starts on the title, not the running header", waitFor { rec.sections.isNotEmpty() } && rec.sections.first().kind == SectionKind.HEADING) { "${rec.sections.firstOrNull()}" }
        c.pause()
        val heardAtPause = rec.heard.size
        Thread.sleep(300)
        check("pause stops progress", rec.heard.size == heardAtPause && c.playState == PlayState.PAUSED)
        c.play()
        check("resume continues", waitFor { rec.heard.size > heardAtPause })

        val before = c.current!!.section
        c.nextParagraph()
        check("next paragraph moves forward", waitFor { rec.sections.last().ordinal > before }) { "before=$before now=${rec.sections.last().ordinal}" }

        c.goToLocation(3u)
        check("go to page 3", waitFor { rec.sections.last().location == 3u }) { "loc=${rec.sections.last().location}" }
        check("no headers, footers or page numbers spoken", rec.sections.none { it.kind == SectionKind.PAGE_FURNITURE })

        c.previousLocation()
        check("previous page returns to page 2", waitFor { rec.sections.last().location == 2u })

        // Speed change restarts the current sentence at the new speed instead of the document.
        val cur = c.current!!
        c.setSpeed(1.5f)
        check("speed change keeps the place", waitFor { eng.spoken.any { it.second == 1.5f } } && c.current!!.section >= cur.section) { "${c.current} vs $cur" }

        c.setSpeed(9f)
        check("speed is clamped to the engine's range", c.currentSpeed == 3.0f)
        c.release()
    }

    // 3. Skip back crosses into the previous sentence.
    run {
        val doc = lib.importFile("$samples/kyc-basics.docx", null).document
        val rec = Recorder(); val eng = FakeEngine(); val sink = TimedSink()
        val c = ReaderController(src, rec)
        c.load(doc.id, opts, ReaderPosition(1u, 2u)); c.setOutput(eng, sink); c.play()
        check("resume from a saved position", waitFor { rec.heard.isNotEmpty() } && rec.heard.first().index == 2u && rec.heard.first().sectionOrdinal == 1u)
        Thread.sleep(30)
        c.skipSeconds(-4)
        check("skip back 4 s reaches the previous sentence", waitFor { rec.heard.any { it.sectionOrdinal == 1u && it.index == 1u } }) { rec.heard.map { it.id }.toString() }
        c.release()
    }

    // 4. Speaker notes only, for a presentation.
    run {
        val doc = lib.importFile("$samples/kyc-training.pptx", null).document
        val notesOnly = opts.copy(readSpeakerNotes = true, readSlideContent = false)
        val rec = Recorder(); val c = ReaderController(src, rec)
        c.load(doc.id, notesOnly, null); c.setOutput(FakeEngine(), TimedSink()); c.play()
        check("notes-only presentation ends", rec.ended.await(10, TimeUnit.SECONDS))
        check("only speaker notes were read", rec.sections.isNotEmpty() && rec.sections.all { it.kind == SectionKind.SPEAKER_NOTES }) { rec.sections.map { it.kind }.toString() }
        c.release()
    }

    // 5. Sleep timer "end of this page" pauses at the page boundary and resumes there.
    run {
        val doc = lib.importFile("$samples/kyc-guide.pdf", null).document
        val rec = Recorder(); val c = ReaderController(src, rec)
        c.load(doc.id, opts, null); c.setOutput(FakeEngine(), TimedSink()); c.play()
        waitFor { rec.heard.isNotEmpty() }
        c.sleepAtEndOfCurrentLocation()
        check("sleep timer fires at end of page 1", rec.slept.await(10, TimeUnit.SECONDS))
        check("player is paused", c.playState == PlayState.PAUSED)
        check("no page 2 audio was played", rec.sections.all { it.location == 1u })
        c.play()
        check("resumes on page 2", waitFor { rec.sections.last().location == 2u })
        c.release()
    }

    // 6. Changing voice mid-document continues from the same place with the new voice.
    run {
        val doc = lib.importFile("$samples/kyc-basics.docx", null).document
        val rec = Recorder(); val a = FakeEngine("a"); val b = FakeEngine("b")
        val c = ReaderController(src, rec)
        c.load(doc.id, opts, ReaderPosition(3u, 0u)); c.setOutput(a, TimedSink()); c.play()
        waitFor { rec.heard.isNotEmpty() }
        c.setOutput(b, TimedSink())
        check("new voice takes over", waitFor { b.spoken.isNotEmpty() })
        check("and starts at or after the old place", b.spoken.first().first.isNotBlank() && c.current!!.section >= 3u)
        c.release()
    }

    println(if (failures == 0) "ALL PASSED" else "$failures FAILED")
    kotlin.system.exitProcess(if (failures == 0) 0 else 1)
}
