package app.readit.playback

import app.readit.core.ReadingOptions
import app.readit.core.Section
import app.readit.core.Segment
import java.util.concurrent.ArrayBlockingQueue
import java.util.concurrent.TimeUnit
import java.util.concurrent.locks.ReentrantLock
import kotlin.concurrent.withLock

/**
 * Reads a document aloud, one segment at a time.
 *
 * Two worker threads cooperate:
 *  - the synthesis thread renders upcoming segments ahead of time (look-ahead), so
 *    playback stays continuous whenever the engine is faster than real time;
 *  - the playback thread writes audio to the sink and reports which segment is
 *    sounding, which drives highlighting and position saving.
 *
 * Every jump (seek, skip, speed or voice change) bumps a generation number, so
 * audio rendered for the old position is discarded rather than played.
 * Platform-free: Android supplies the engine and sink; tests supply fakes.
 */
class ReaderController(
    private val source: DocumentSource,
    private val listener: ReaderListener,
    lookAhead: Int = 3,
) {
    private class Item(
        val generation: Long,
        val section: Section?,
        val segment: Segment?,
        val samples: FloatArray,
        val pauseSamples: Int,
        val startOffset: Int,
        val end: Boolean = false,
    )

    private class Played(val pos: ReaderPosition, val samples: Int, val sampleRate: Int)

    private val lock = ReentrantLock()
    private val wake = lock.newCondition()
    private val queue = ArrayBlockingQueue<Item>(lookAhead.coerceAtLeast(1))

    private var documentId: String? = null
    private var engine: SpeechEngine? = null
    private var sink: AudioSink? = null
    private var options: ReadingOptions? = null
    private var speed = 1.0f
    private var pitch = 1.0f

    @Volatile private var generation = 0L
    @Volatile private var state = PlayState.IDLE
    @Volatile private var released = false

    // Where synthesis continues from, and where playback currently is.
    private var synthCursor: ReaderPosition? = null
    private var pendingStartOffset = 0
    @Volatile var current: ReaderPosition? = null
        private set
    private val history = ArrayDeque<Played>()
    private val sectionCache = LinkedHashMap<UInt, Pair<Section, List<Segment>>>()

    // Interval progress inside the sounding segment, for skip-by-seconds.
    @Volatile private var playedInSegment = 0
    @Volatile private var currentSegmentSamples = 0
    @Volatile private var skipRequest: Int? = null

    private var sleepDeadlineMs: Long? = null
    private var sleepAtEndOfSection: UInt? = null
    private var unloggedSamples = 0L
    private var openedSampleRate = 0

    private val synthThread = Thread(::synthLoop, "readit-synth").apply { isDaemon = true; start() }
    private val playThread = Thread(::playLoop, "readit-play").apply { isDaemon = true; start() }

    val playState: PlayState get() = state
    val currentSpeed: Float get() = speed

    // ---------------------------------------------------------------- setup

    fun load(documentId: String, options: ReadingOptions, start: ReaderPosition?) = lock.withLock {
        flushLog()
        this.documentId = documentId
        this.options = options
        sectionCache.clear()
        history.clear()
        val first = start ?: firstReadable() ?: ReaderPosition(0u, 0u)
        current = first
        restartLocked(first, 0, keepPlaying = false)
        setState(PlayState.IDLE)
    }

    fun setOutput(engine: SpeechEngine, sink: AudioSink) = lock.withLock {
        val changed = this.engine?.voiceId != engine.voiceId
        this.engine = engine
        if (this.sink !== sink) {
            this.sink?.close()
            this.sink = sink
            openedSampleRate = 0
        }
        if (changed) current?.let { restartLocked(it, 0, keepPlaying = state == PlayState.PLAYING) }
    }

    fun setOptions(options: ReadingOptions) = lock.withLock {
        this.options = options
        sectionCache.clear()
        current?.let { restartLocked(it, 0, keepPlaying = state == PlayState.PLAYING) }
    }

    /** Speed changes resume from the start of the current sentence, not the document. */
    fun setSpeed(value: Float) = lock.withLock {
        val e = engine
        val v = if (e != null) value.coerceIn(e.minSpeed, e.maxSpeed) else value.coerceIn(0.5f, 3f)
        if (v == speed) return@withLock
        speed = v
        current?.let { restartLocked(it, 0, keepPlaying = state == PlayState.PLAYING) }
    }

    fun setPitch(value: Float) = lock.withLock {
        if (engine?.supportsPitch != true) return@withLock
        pitch = value.coerceIn(0.5f, 2f)
        current?.let { restartLocked(it, 0, keepPlaying = state == PlayState.PLAYING) }
    }

    // ------------------------------------------------------------- transport

    fun play() = lock.withLock {
        if (documentId == null || engine == null || sink == null) {
            listener.onError("Choose a document and a voice first.")
            return@withLock
        }
        if (state == PlayState.ENDED) {
            firstReadable()?.let { restartLocked(it, 0, keepPlaying = false) }
        }
        if (state == PlayState.PAUSED) sink?.resume()
        setState(if (queue.isEmpty() && playedInSegment == 0) PlayState.PREPARING else PlayState.PLAYING)
        wake.signalAll()
    }

    fun pause() = lock.withLock {
        if (state == PlayState.PLAYING || state == PlayState.PREPARING) {
            sink?.pause()
            setState(PlayState.PAUSED)
            flushLog()
            current?.let { save(it) }
        }
    }

    fun togglePlay() = if (state == PlayState.PLAYING || state == PlayState.PREPARING) pause() else play()

    fun stop() = lock.withLock {
        current?.let { save(it) }
        flushLog()
        generation++
        queue.clear()
        sink?.flush()
        sink?.pause()
        setState(PlayState.IDLE)
    }

    fun seekTo(pos: ReaderPosition) = lock.withLock { restartLocked(pos, 0, keepPlaying = isActive()) }

    /** Restart the sentence that is currently being read. */
    fun restartSegment() = lock.withLock { current?.let { restartLocked(it, 0, keepPlaying = isActive()) } }

    fun nextParagraph() = lock.withLock {
        val doc = documentId ?: return@withLock
        val cur = current ?: return@withLock
        val next = source.nextReadable(doc, cur.section, true, options!!) ?: return@withLock
        restartLocked(ReaderPosition(next, 0u), 0, keepPlaying = isActive())
    }

    /** Back to the start of this paragraph, or the previous one if we are at its start. */
    fun previousParagraph() = lock.withLock {
        val doc = documentId ?: return@withLock
        val cur = current ?: return@withLock
        val target = if (cur.segment > 0u || playedSeconds() > 2.0) {
            cur.section
        } else {
            source.nextReadable(doc, cur.section, false, options!!) ?: cur.section
        }
        restartLocked(ReaderPosition(target, 0u), 0, keepPlaying = isActive())
    }

    /** Jump to a page, slide or chapter number. */
    fun goToLocation(location: UInt) = lock.withLock {
        val doc = documentId ?: return@withLock
        val ord = source.sectionAtLocation(doc, location) ?: return@withLock
        val readable = if (isReadable(ord)) ord else source.nextReadable(doc, ord, true, options!!) ?: return@withLock
        restartLocked(ReaderPosition(readable, 0u), 0, keepPlaying = isActive())
    }

    fun nextLocation() = lock.withLock {
        val loc = current?.let { sectionOf(it.section)?.location } ?: return@withLock
        lock.unlock()
        try { goToLocation(loc + 1u) } finally { lock.lock() }
    }

    fun previousLocation() = lock.withLock {
        val loc = current?.let { sectionOf(it.section)?.location } ?: return@withLock
        if (loc > 1u) {
            lock.unlock()
            try { goToLocation(loc - 1u) } finally { lock.lock() }
        }
    }

    /** Skip forward or back by a number of seconds, crossing sentence boundaries when needed. */
    fun skipSeconds(seconds: Int) = lock.withLock {
        val rate = engine?.sampleRate ?: return@withLock
        val delta = seconds * rate
        if (seconds > 0) {
            val target = playedInSegment + delta
            if (target < currentSegmentSamples) {
                skipRequest = target
            } else {
                // Move to the following sentence(s); a few seconds rarely spans more than one.
                val next = nextPosition(current ?: return@withLock) ?: return@withLock
                restartLocked(next, 0, keepPlaying = isActive())
            }
        } else {
            var remaining = -delta - playedInSegment
            if (remaining <= 0) {
                skipRequest = playedInSegment + delta
                return@withLock
            }
            // Walk back through what was already played.
            val past = history.toList().asReversed().drop(1) // drop the current segment
            for (p in past) {
                val n = p.samples * rate / p.sampleRate
                if (remaining <= n) {
                    restartLocked(p.pos, n - remaining, keepPlaying = isActive())
                    return@withLock
                }
                remaining -= n
            }
            // Earlier than anything heard this session: step back through the text,
            // estimating each sentence's length from its spoken text.
            var pos = past.lastOrNull()?.pos ?: current ?: return@withLock
            while (true) {
                val prev = previousPosition(pos) ?: break
                val text = sectionData(prev.section)?.second?.getOrNull(prev.segment.toInt())?.spokenText.orEmpty()
                val n = (text.length * 0.065 / speed * rate).toInt().coerceAtLeast(rate / 2)
                pos = prev
                if (remaining <= n) {
                    restartLocked(prev, 0, keepPlaying = isActive())
                    return@withLock
                }
                remaining -= n
            }
            restartLocked(pos, 0, keepPlaying = isActive())
        }
    }

    // ----------------------------------------------------------- sleep timer

    fun setSleepTimer(minutes: Int?) = lock.withLock {
        sleepAtEndOfSection = null
        sleepDeadlineMs = minutes?.let { System.currentTimeMillis() + it * 60_000L }
    }

    fun sleepAtEndOfCurrentLocation() = lock.withLock {
        sleepDeadlineMs = null
        sleepAtEndOfSection = current?.let { sectionOf(it.section)?.location }
    }

    val sleepTimerRemainingMs: Long?
        get() = sleepDeadlineMs?.let { (it - System.currentTimeMillis()).coerceAtLeast(0) }

    fun release() {
        lock.withLock {
            current?.let { save(it) }
            flushLog()
            released = true
            generation++
            queue.clear()
            wake.signalAll()
        }
        synthThread.interrupt()
        playThread.interrupt()
        sink?.close()
        engine?.release()
    }

    // ---------------------------------------------------------------- internals

    private fun isActive() = state == PlayState.PLAYING || state == PlayState.PREPARING

    private fun setState(s: PlayState) {
        if (state != s) {
            state = s
            listener.onStateChanged(s)
        }
    }

    private fun restartLocked(pos: ReaderPosition, offsetSamples: Int, keepPlaying: Boolean) {
        generation++
        queue.clear()
        sink?.flush()
        synthCursor = pos
        pendingStartOffset = offsetSamples.coerceAtLeast(0)
        current = pos
        playedInSegment = 0
        skipRequest = null
        if (keepPlaying) {
            setState(PlayState.PREPARING)
        } else if (state == PlayState.ENDED) {
            setState(PlayState.IDLE)
        }
        save(pos)
        wake.signalAll()
    }

    private fun save(pos: ReaderPosition) {
        val doc = documentId ?: return
        runCatching { source.savePosition(doc, pos.section, pos.segment) }
    }

    private fun flushLog() {
        val doc = documentId ?: return
        val rate = openedSampleRate.takeIf { it > 0 } ?: return
        val secs = (unloggedSamples / rate).toInt()
        if (secs > 0) {
            runCatching { source.logListening(doc, secs.toUInt()) }
            unloggedSamples -= secs.toLong() * rate
        }
    }

    private fun playedSeconds(): Double = engine?.let { playedInSegment.toDouble() / it.sampleRate } ?: 0.0

    private fun sectionData(ordinal: UInt): Pair<Section, List<Segment>>? {
        sectionCache[ordinal]?.let { return it }
        val doc = documentId ?: return null
        val section = source.section(doc, ordinal) ?: return null
        val segs = source.segments(doc, ordinal, options!!)
        if (sectionCache.size > 64) sectionCache.remove(sectionCache.keys.first())
        return (section to segs).also { sectionCache[ordinal] = it }
    }

    private fun sectionOf(ordinal: UInt): Section? = sectionData(ordinal)?.first

    private fun isReadable(ordinal: UInt) = sectionData(ordinal)?.second?.isNotEmpty() == true

    private fun firstReadable(): ReaderPosition? {
        if (isReadable(0u)) return ReaderPosition(0u, 0u)
        val doc = documentId ?: return null
        return source.nextReadable(doc, 0u, true, options!!)?.let { ReaderPosition(it, 0u) }
    }

    private fun nextPosition(pos: ReaderPosition): ReaderPosition? {
        val segs = sectionData(pos.section)?.second.orEmpty()
        if (pos.segment + 1u < segs.size.toUInt()) return ReaderPosition(pos.section, pos.segment + 1u)
        val doc = documentId ?: return null
        val next = source.nextReadable(doc, pos.section, true, options!!) ?: return null
        return ReaderPosition(next, 0u)
    }

    private fun previousPosition(pos: ReaderPosition): ReaderPosition? {
        if (pos.segment > 0u) return ReaderPosition(pos.section, pos.segment - 1u)
        val doc = documentId ?: return null
        val prev = source.nextReadable(doc, pos.section, false, options!!) ?: return null
        val n = sectionData(prev)?.second?.size ?: return null
        return ReaderPosition(prev, (n - 1).coerceAtLeast(0).toUInt())
    }

    private fun synthLoop() {
        while (!released) {
            var gen: Long
            var pos: ReaderPosition
            var eng: SpeechEngine
            var spd: Float
            var ptch: Float
            var offset: Int
            lock.lock()
            try {
                while (!released && (!isActive() || synthCursor == null || engine == null)) {
                    wake.await(500, TimeUnit.MILLISECONDS)
                }
                if (released) return
                gen = generation
                pos = synthCursor!!
                eng = engine!!
                spd = speed
                ptch = pitch
                offset = pendingStartOffset
                pendingStartOffset = 0
            } catch (_: InterruptedException) {
                return
            } finally {
                lock.unlock()
            }

            // Resolve the position to a real sentence; skipped sections are passed over.
            val resolved: Pair<Pair<Section, List<Segment>>, Segment>? = lock.withLock {
                var p: ReaderPosition? = pos
                var found: Pair<Pair<Section, List<Segment>>, Segment>? = null
                while (p != null && found == null) {
                    val data = sectionData(p.section)
                    val seg = data?.second?.getOrNull(p.segment.toInt())
                    if (data != null && seg != null) found = data to seg
                    else p = if (data == null) null else nextPosition(ReaderPosition(p.section, data.second.size.toUInt()))
                }
                if (gen == generation && p != null) synthCursor = p
                found
            }

            val item = if (resolved == null) {
                Item(gen, null, null, FloatArray(0), 0, 0, end = true)
            } else {
                val (data, seg) = resolved
                try {
                    val samples = eng.synthesize(seg.spokenText, spd, ptch)
                    val pause = (seg.pauseAfterMs.toLong() * eng.sampleRate / 1000).toInt()
                    Item(gen, data.first, seg, samples, pause, offset)
                } catch (e: Exception) {
                    listener.onError("The voice could not read this passage: ${e.message ?: e.javaClass.simpleName}")
                    lock.withLock { if (gen == generation) { sink?.pause(); setState(PlayState.PAUSED) } }
                    continue
                }
            }

            val isLast = lock.withLock {
                if (gen != generation) return@withLock false
                val next = if (item.end) null else nextPosition(ReaderPosition(item.segment!!.sectionOrdinal, item.segment.index))
                synthCursor = next
                next == null
            }
            try {
                if (!offerWhileCurrent(item, gen)) continue
                if (isLast && !item.end) offerWhileCurrent(Item(gen, null, null, FloatArray(0), 0, 0, end = true), gen)
                if (isLast || item.end) lock.withLock { waitUntilRestart(gen) }
            } catch (_: InterruptedException) {
                return
            }
        }
    }

    private fun offerWhileCurrent(item: Item, gen: Long): Boolean {
        while (!released && gen == generation) {
            if (queue.offer(item, 200, TimeUnit.MILLISECONDS)) return true
        }
        return false
    }

    /** Synthesis has reached the end; idle until the user jumps somewhere. */
    private fun waitUntilRestart(gen: Long) {
        while (!released && gen == generation) {
            try { wake.await(500, TimeUnit.MILLISECONDS) } catch (_: InterruptedException) { return }
        }
    }

    private fun playLoop() {
        val chunk = 2048
        while (!released) {
            val item = try { queue.poll(200, TimeUnit.MILLISECONDS) } catch (_: InterruptedException) { return } ?: continue
            if (item.generation != generation) continue
            if (item.end) {
                lock.withLock {
                    if (item.generation == generation) {
                        flushLog()
                        setState(PlayState.ENDED)
                    }
                }
                continue
            }
            val section = item.section!!
            val seg = item.segment!!
            // Do not start a new sentence while paused: hold it until play resumes.
            try {
                while (!released && item.generation == generation && (state == PlayState.PAUSED || state == PlayState.IDLE)) {
                    lock.withLock { if (state == PlayState.PAUSED || state == PlayState.IDLE) wake.await(200, TimeUnit.MILLISECONDS) }
                }
            } catch (_: InterruptedException) {
                return
            }
            if (item.generation != generation || released) continue
            if (sleepAtEndOfSection != null && section.location != sleepAtEndOfSection) {
                sleepDue(section, seg)
                continue
            }
            val sk = sink ?: continue
            val rate = engine?.sampleRate ?: continue
            val started = lock.withLock {
                // Re-check under the lock so a pause between the wait above and here is honoured.
                while (!released && item.generation == generation && (state == PlayState.PAUSED || state == PlayState.IDLE)) {
                    try { wake.await(200, TimeUnit.MILLISECONDS) } catch (_: InterruptedException) { return }
                }
                if (item.generation != generation || released) return@withLock false
                if (openedSampleRate != rate) {
                    sk.open(rate)
                    openedSampleRate = rate
                }
                val pos = ReaderPosition(seg.sectionOrdinal, seg.index)
                current = pos
                currentSegmentSamples = item.samples.size
                playedInSegment = item.startOffset.coerceIn(0, item.samples.size)
                history.addLast(Played(pos, item.samples.size, rate))
                while (history.size > 50) history.removeFirst()
                if (state == PlayState.PREPARING) setState(PlayState.PLAYING)
                save(pos)
                // Listeners must not call back into the controller synchronously (post to a UI thread instead).
                listener.onSegmentStarted(section, seg)
                true
            }
            if (!started) continue

            var i = playedInSegment
            val total = item.samples.size + item.pauseSamples
            val silence = FloatArray(chunk)
            while (i < total && item.generation == generation && !released) {
                if (state == PlayState.PAUSED || state == PlayState.IDLE) {
                    try {
                        lock.withLock { if (state == PlayState.PAUSED || state == PlayState.IDLE) wake.await(200, TimeUnit.MILLISECONDS) }
                    } catch (_: InterruptedException) {
                        return
                    }
                    continue
                }
                skipRequest?.let { req ->
                    skipRequest = null
                    sk.flush()
                    i = req.coerceIn(0, item.samples.size)
                }
                if (sleepDue(section, seg)) break
                val n = minOf(chunk, total - i)
                try {
                    if (i < item.samples.size) {
                        val m = minOf(n, item.samples.size - i)
                        sk.write(item.samples, i, m)
                        i += m
                    } else {
                        sk.write(silence, 0, n)
                        i += n
                    }
                } catch (_: InterruptedException) {
                    return
                }
                playedInSegment = minOf(i, item.samples.size)
                unloggedSamples += n
                if (unloggedSamples > rate * 60L) lock.withLock { flushLog() }
            }
        }
    }

    private fun sleepDue(section: Section, seg: Segment): Boolean {
        val deadline = sleepDeadlineMs
        val endLoc = sleepAtEndOfSection
        val due = (deadline != null && System.currentTimeMillis() >= deadline) ||
            (endLoc != null && section.location != endLoc)
        // (End-of-page timers are checked before a new sentence starts; see playLoop.)
        if (!due) return false
        lock.withLock {
            sleepDeadlineMs = null
            sleepAtEndOfSection = null
            flushLog()
            // Resume exactly here next time: re-queue this sentence from its start.
            restartLocked(ReaderPosition(seg.sectionOrdinal, seg.index), 0, keepPlaying = false)
            sink?.pause()
            setState(PlayState.PAUSED)
        }
        listener.onSleepTimerFinished()
        return true
    }
}
