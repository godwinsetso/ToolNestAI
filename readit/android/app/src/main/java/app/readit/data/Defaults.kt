package app.readit.data

import app.readit.core.CodeMode
import app.readit.core.ReadingOptions
import app.readit.core.ReadingProfile

/** Reading options derived from a saved profile, with READIT's defaults for the rest. */
fun readingOptions(profile: ReadingProfile? = null, spellMode: Boolean = false) = ReadingOptions(
    skipPageFurniture = profile?.skipHeadersFooters ?: true,
    readSlideContent = profile?.readSlideContent ?: true,
    readSpeakerNotes = profile?.readSpeakerNotes ?: false,
    readTables = true,
    readFootnotes = false,
    codeMode = CodeMode.ANNOUNCE,
    paragraphPauseMs = profile?.paragraphPauseMs ?: 350u,
    headingPauseMs = profile?.headingPauseMs ?: 700u,
    spellAcronyms = true,
    spellMode = spellMode,
    dayFirstDates = true,
)
