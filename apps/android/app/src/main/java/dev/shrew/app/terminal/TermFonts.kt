package dev.shrew.app.terminal

import android.content.Context
import android.graphics.Typeface
import androidx.core.content.res.ResourcesCompat
import dev.shrew.app.R

/** A terminal font: regular, bold, italic and bold italic faces (same ids as iOS). */
class TermFont(val id: String, val name: String, private val faces: IntArray?) {
    /** For showing the font's name in itself in Settings. */
    val family: androidx.compose.ui.text.font.FontFamily =
        faces?.let { androidx.compose.ui.text.font.FontFamily(androidx.compose.ui.text.font.Font(it[0]), androidx.compose.ui.text.font.Font(it[1], androidx.compose.ui.text.font.FontWeight.Bold)) }
            ?: androidx.compose.ui.text.font.FontFamily.Monospace

    /** [regular, bold, italic, boldItalic]. */
    fun typefaces(context: Context): Array<Typeface> {
        val f = faces ?: return arrayOf(
            Typeface.MONOSPACE,
            Typeface.create(Typeface.MONOSPACE, Typeface.BOLD),
            Typeface.create(Typeface.MONOSPACE, Typeface.ITALIC),
            Typeface.create(Typeface.MONOSPACE, Typeface.BOLD_ITALIC),
        )
        return Array(4) { ResourcesCompat.getFont(context, f[it]) ?: Typeface.MONOSPACE }
    }
}

object TermFonts {
    val all = listOf(
        TermFont("jetbrains-mono", "JetBrains Mono", intArrayOf(R.font.jetbrains_mono_regular, R.font.jetbrains_mono_bold, R.font.jetbrains_mono_italic, R.font.jetbrains_mono_bold_italic)),
        TermFont("iosevka", "Iosevka", intArrayOf(R.font.iosevka_term_regular, R.font.iosevka_term_bold, R.font.iosevka_term_italic, R.font.iosevka_term_bold_italic)),
        TermFont("fira-code", "Fira Code", intArrayOf(R.font.fira_code_regular, R.font.fira_code_bold, R.font.fira_code_regular, R.font.fira_code_bold)),
        TermFont("cascadia-code", "Cascadia Code", intArrayOf(R.font.cascadia_code_regular, R.font.cascadia_code_bold, R.font.cascadia_code_italic, R.font.cascadia_code_bold_italic)),
        TermFont("ibm-plex-mono", "IBM Plex Mono", intArrayOf(R.font.ibm_plex_mono_regular, R.font.ibm_plex_mono_bold, R.font.ibm_plex_mono_italic, R.font.ibm_plex_mono_bold_italic)),
        TermFont("source-code-pro", "Source Code Pro", intArrayOf(R.font.source_code_pro_regular, R.font.source_code_pro_bold, R.font.source_code_pro_italic, R.font.source_code_pro_bold_italic)),
        TermFont("system", "System Mono", null),
    )

    fun byId(id: String?): TermFont = all.firstOrNull { it.id == id } ?: all.first()
}
