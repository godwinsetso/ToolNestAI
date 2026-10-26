import app.readit.core.*

fun main(args: Array<String>) {
    val dir = java.nio.file.Files.createTempDirectory("readit-jvm").toString()
    val lib = ReaditLibrary(dir, System.getenv("READIT_PDFIUM_DIR"))
    val doc = lib.importFile(args[0], null).document
    println("title=${doc.title} format=${doc.format} sections=${doc.sectionCount}")
    val opts = ReadingOptions(true, true, false, true, false, CodeMode.ANNOUNCE, 350u, 700u, true, false, true)
    val segs = lib.segments(doc.id, 1u, opts)
    segs.forEach { println("  [${it.start}-${it.end}] ${it.spokenText}") }
    try { lib.importFile(args[1], null); println("NO ERROR?") } catch (e: ReaditException) { println("error ok: ${e.javaClass.simpleName}: ${e.message}") }
    println(coreVersion())
}
