package app.readit.ui.theme

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Typography
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.sp

// Calm, paper-like palette: deep green ink on warm off-white; soft charcoal at night.
private val Light = lightColorScheme(
    primary = Color(0xFF1F5C4A),
    onPrimary = Color.White,
    primaryContainer = Color(0xFFCDE8DD),
    onPrimaryContainer = Color(0xFF062019),
    secondary = Color(0xFF5B6360),
    secondaryContainer = Color(0xFFE2E6E3),
    tertiary = Color(0xFFB4632B),
    tertiaryContainer = Color(0xFFFBE2CF),
    background = Color(0xFFF6F4EF),
    onBackground = Color(0xFF1B1C1A),
    surface = Color(0xFFF6F4EF),
    onSurface = Color(0xFF1B1C1A),
    surfaceVariant = Color(0xFFE7E4DC),
    onSurfaceVariant = Color(0xFF4A4A45),
    surfaceContainer = Color(0xFFEDEAE3),
    surfaceContainerHigh = Color(0xFFE6E3DB),
    outline = Color(0xFF7A7A73),
    error = Color(0xFFB3261E),
)

private val Dark = darkColorScheme(
    primary = Color(0xFF8FD2B7),
    onPrimary = Color(0xFF003828),
    primaryContainer = Color(0xFF1F5C4A),
    onPrimaryContainer = Color(0xFFCDE8DD),
    secondary = Color(0xFFBFC8C4),
    secondaryContainer = Color(0xFF3A403E),
    tertiary = Color(0xFFF2B98E),
    tertiaryContainer = Color(0xFF6B3A14),
    background = Color(0xFF121614),
    onBackground = Color(0xFFE2E3DF),
    surface = Color(0xFF121614),
    onSurface = Color(0xFFE2E3DF),
    surfaceVariant = Color(0xFF2A2F2C),
    onSurfaceVariant = Color(0xFFC2C7C3),
    surfaceContainer = Color(0xFF1B201D),
    surfaceContainerHigh = Color(0xFF242A27),
    outline = Color(0xFF8C928E),
    error = Color(0xFFF2B8B5),
)

/** Reading text is a serif face, sized for long stretches; interface text stays sans. */
val ReadingStyle = TextStyle(fontFamily = FontFamily.Serif, fontSize = 19.sp, lineHeight = 30.sp)

private val AppTypography = Typography().let { t ->
    t.copy(
        headlineMedium = t.headlineMedium.copy(fontWeight = FontWeight.SemiBold, letterSpacing = (-0.2).sp),
        titleLarge = t.titleLarge.copy(fontWeight = FontWeight.SemiBold),
        titleMedium = t.titleMedium.copy(fontWeight = FontWeight.SemiBold),
        labelLarge = t.labelLarge.copy(fontWeight = FontWeight.SemiBold),
    )
}

/** themeMode: "system", "light" or "dark". highContrast strengthens text and outlines. */
@Composable
fun ReaditTheme(themeMode: String = "system", highContrast: Boolean = false, content: @Composable () -> Unit) {
    val dark = when (themeMode) {
        "dark" -> true
        "light" -> false
        else -> isSystemInDarkTheme()
    }
    var scheme = if (dark) Dark else Light
    if (highContrast) {
        scheme = if (dark) {
            scheme.copy(onSurface = Color.White, onBackground = Color.White, onSurfaceVariant = Color(0xFFE6E6E6), outline = Color.White, background = Color.Black, surface = Color.Black)
        } else {
            scheme.copy(onSurface = Color.Black, onBackground = Color.Black, onSurfaceVariant = Color(0xFF1A1A1A), outline = Color.Black, background = Color.White, surface = Color.White)
        }
    }
    MaterialTheme(colorScheme = scheme, typography = AppTypography, content = content)
}
