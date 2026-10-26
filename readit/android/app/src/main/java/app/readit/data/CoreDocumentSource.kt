package app.readit.data

import app.readit.core.ReadingOptions
import app.readit.core.ReaditLibrary
import app.readit.core.Section
import app.readit.core.Segment
import app.readit.playback.DocumentSource

/** Adapts the Rust core library to the reader's [DocumentSource]. */
class CoreDocumentSource(private val lib: ReaditLibrary) : DocumentSource {
    override fun segments(documentId: String, ordinal: UInt, options: ReadingOptions): List<Segment> =
        lib.segments(documentId, ordinal, options)

    override fun nextReadable(documentId: String, fromOrdinal: UInt, forward: Boolean, options: ReadingOptions): UInt? =
        lib.nextReadable(documentId, fromOrdinal, forward, options)

    override fun sectionAtLocation(documentId: String, location: UInt): UInt? =
        lib.sectionAtLocation(documentId, location)

    override fun section(documentId: String, ordinal: UInt): Section? =
        lib.sections(documentId, ordinal, 1u).firstOrNull()?.takeIf { it.ordinal == ordinal }

    override fun savePosition(documentId: String, sectionOrdinal: UInt, segmentIndex: UInt) =
        lib.savePosition(documentId, sectionOrdinal, segmentIndex)

    override fun logListening(documentId: String, seconds: UInt) = lib.logListening(documentId, seconds)
}
