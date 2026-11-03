package dev.shrew.app.ui

import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Typography
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.ReadOnlyComposable
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.lerp
import androidx.compose.ui.graphics.luminance
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.Font
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.sp
import dev.shrew.app.R
import uniffi.shrew_ffi.AgentState

val Mono = FontFamily(
    Font(R.font.jetbrains_mono_regular, FontWeight.Normal),
    Font(R.font.jetbrains_mono_bold, FontWeight.Bold),
    Font(R.font.jetbrains_mono_italic, FontWeight.Normal, FontStyle.Italic),
    Font(R.font.jetbrains_mono_bold_italic, FontWeight.Bold, FontStyle.Italic),
)

/** A terminal color scheme. `ansi == null` means "use whatever the server sends". */
@Immutable
data class ThemeSpec(
    val id: String,
    val name: String,
    val background: Long,
    val foreground: Long,
    val cursor: Long,
    val accent: Long,
    val ansi: List<Long>?,
)

val Themes = listOf(
    ThemeSpec(
        "tokyo-night", "Tokyo Night", 0xFF1A1B26, 0xFFC0CAF5, 0xFFC0CAF5, 0xFF7AA2F7,
        listOf(0xFF15161E, 0xFFF7768E, 0xFF9ECE6A, 0xFFE0AF68, 0xFF7AA2F7, 0xFFBB9AF7, 0xFF7DCFFF, 0xFFA9B1D6, 0xFF414868, 0xFFFF899D, 0xFFAFD67A, 0xFFE9C582, 0xFF8DB0FF, 0xFFC7A9FF, 0xFF97D8F8, 0xFFC0CAF5),
    ),
    ThemeSpec(
        "shrew", "Shrew", 0xFF0D1117, 0xFFE6EDF3, 0xFF5EE0A8, 0xFF5EE0A8,
        listOf(0xFF484F58, 0xFFFF7B72, 0xFF3FB950, 0xFFD29922, 0xFF58A6FF, 0xFFBC8CFF, 0xFF39C5CF, 0xFFB1BAC4, 0xFF6E7681, 0xFFFFA198, 0xFF56D364, 0xFFE3B341, 0xFF79C0FF, 0xFFD2A8FF, 0xFF56D4DD, 0xFFFFFFFF),
    ),
    ThemeSpec(
        "midnight", "Midnight", 0xFF0F1626, 0xFFD8DEE9, 0xFF88C0D0, 0xFF8AB4F8,
        listOf(0xFF3B4252, 0xFFE06C75, 0xFFA3BE8C, 0xFFE5C07B, 0xFF81A1C1, 0xFFB48EAD, 0xFF88C0D0, 0xFFE5E9F0, 0xFF4C566A, 0xFFEF8891, 0xFFB5D19E, 0xFFF0D08C, 0xFF8AB4F8, 0xFFC8A2C8, 0xFF8FBCBB, 0xFFECEFF4),
    ),
    ThemeSpec(
        "mocha", "Catppuccin Mocha", 0xFF1E1E2E, 0xFFCDD6F4, 0xFFF5E0DC, 0xFFCBA6F7,
        listOf(0xFF45475A, 0xFFF38BA8, 0xFFA6E3A1, 0xFFF9E2AF, 0xFF89B4FA, 0xFFF5C2E7, 0xFF94E2D5, 0xFFBAC2DE, 0xFF585B70, 0xFFF38BA8, 0xFFA6E3A1, 0xFFF9E2AF, 0xFF89B4FA, 0xFFF5C2E7, 0xFF94E2D5, 0xFFA6ADC8),
    ),
    ThemeSpec(
        "gruvbox", "Gruvbox", 0xFF282828, 0xFFEBDBB2, 0xFFEBDBB2, 0xFFFABD2F,
        listOf(0xFF3C3836, 0xFFCC241D, 0xFF98971A, 0xFFD79921, 0xFF458588, 0xFFB16286, 0xFF689D6A, 0xFFA89984, 0xFF928374, 0xFFFB4934, 0xFFB8BB26, 0xFFFABD2F, 0xFF83A598, 0xFFD3869B, 0xFF8EC07C, 0xFFEBDBB2),
    ),
    ThemeSpec(
        "solarized-light", "Solarized Light", 0xFFFDF6E3, 0xFF586E75, 0xFF586E75, 0xFF268BD2,
        listOf(0xFF073642, 0xFFDC322F, 0xFF859900, 0xFFB58900, 0xFF268BD2, 0xFFD33682, 0xFF2AA198, 0xFFEEE8D5, 0xFF002B36, 0xFFCB4B16, 0xFF586E75, 0xFF657B83, 0xFF839496, 0xFF6C71C4, 0xFF93A1A1, 0xFFFDF6E3),
    ),
    ThemeSpec("server", "Server colors", 0xFF0D1117, 0xFFE6EDF3, 0xFF5EE0A8, 0xFF5EE0A8, null),
)

fun themeById(id: String?): ThemeSpec = Themes.firstOrNull { it.id == id } ?: Themes.first()  // Tokyo Night by default

/**
 * Every chrome color, derived from a terminal's background and foreground so the
 * UI always belongs to the terminal it sits on — including "Server colors".
 */
@Immutable
data class Chrome(
    val bg: Color,
    val text: Color,
    val accent: Color,
    val light: Boolean,
) {
    val terminal get() = bg
    val surface get() = lerp(bg, text, 0.05f)
    val raised get() = lerp(bg, text, 0.09f)
    val glass get() = lerp(bg, text, 0.11f).copy(alpha = 0.92f)
    val glassStroke get() = text.copy(alpha = if (light) 0.10f else 0.08f)
    val key get() = lerp(bg, text, 0.14f)
    val keyDown get() = lerp(bg, text, 0.24f)
    val hairline get() = text.copy(alpha = 0.08f)
    val secondary get() = lerp(text, bg, 0.38f)
    val tertiary get() = lerp(text, bg, 0.58f)

    companion object {
        fun of(bg: Color, fg: Color, accent: Color) = Chrome(bg, fg, accent, bg.luminance() > 0.5f)
        fun of(t: ThemeSpec) = of(Color(t.background), Color(t.foreground), Color(t.accent))
    }
}

val LocalChrome = staticCompositionLocalOf { Chrome.of(Themes.first()) }

/** The current chrome. Reads like a palette object at call sites: `Ink.text`. */
val Ink: Chrome
    @Composable @ReadOnlyComposable get() = LocalChrome.current

@Immutable
data class StatusColors(
    val online: Color,
    val connecting: Color,
    val offline: Color,
    val working: Color,
    val blocked: Color,
    val done: Color,
    val error: Color,
    val idle: Color,
    val surfaceRaised: Color,
    val hairline: Color,
    val muted: Color,
) {
    fun agent(state: AgentState?): Color = when (state) {
        AgentState.WORKING -> working
        AgentState.BLOCKED -> blocked
        AgentState.DONE -> done
        AgentState.ERROR -> error
        AgentState.IDLE, null -> idle
    }
}

private fun status(c: Chrome) = StatusColors(
    online = Color(0xFF5EE0A8),
    connecting = Color(0xFFE8B54A),
    offline = c.tertiary,
    working = Color(0xFF6CB6FF),
    blocked = Color(0xFFF2A93B),
    done = Color(0xFF5EE0A8),
    error = Color(0xFFF2667A),
    idle = c.tertiary,
    surfaceRaised = c.raised,
    hairline = c.hairline,
    muted = c.secondary,
)

val LocalStatus = staticCompositionLocalOf { status(Chrome.of(Themes.first())) }

private val AppTypography = Typography().run {
    copy(
        titleLarge = titleLarge.copy(fontSize = 20.sp, fontWeight = FontWeight.SemiBold, letterSpacing = (-0.2).sp),
        titleMedium = titleMedium.copy(fontSize = 15.sp, fontWeight = FontWeight.Medium, letterSpacing = 0.sp),
        bodyLarge = bodyLarge.copy(fontSize = 15.sp, letterSpacing = 0.sp),
        bodyMedium = bodyMedium.copy(fontSize = 14.sp, letterSpacing = 0.sp),
        labelMedium = labelMedium.copy(fontSize = 12.sp, fontWeight = FontWeight.Medium, letterSpacing = 0.6.sp),
    )
}

val MonoSmall = TextStyle(fontFamily = Mono, fontSize = 12.sp, lineHeight = 16.sp, letterSpacing = 0.sp)

@Composable
fun ShrewTheme(chrome: Chrome, content: @Composable () -> Unit) {
    val base = if (chrome.light) lightColorScheme() else darkColorScheme()
    val scheme = base.copy(
        primary = chrome.accent,
        onPrimary = chrome.bg,
        primaryContainer = lerp(chrome.bg, chrome.accent, 0.2f),
        onPrimaryContainer = chrome.text,
        secondary = chrome.secondary,
        secondaryContainer = chrome.key,
        onSecondaryContainer = chrome.text,
        background = chrome.bg,
        onBackground = chrome.text,
        surface = chrome.bg,
        onSurface = chrome.text,
        surfaceVariant = chrome.raised,
        onSurfaceVariant = chrome.secondary,
        surfaceContainerLowest = chrome.bg,
        surfaceContainerLow = chrome.surface,
        surfaceContainer = chrome.surface,
        surfaceContainerHigh = chrome.raised,
        surfaceContainerHighest = chrome.key,
        outline = chrome.text.copy(alpha = 0.18f),
        outlineVariant = chrome.hairline,
        error = Color(0xFFF2667A),
    )
    CompositionLocalProvider(LocalChrome provides chrome, LocalStatus provides status(chrome)) {
        MaterialTheme(colorScheme = scheme, typography = AppTypography, content = content)
    }
}
