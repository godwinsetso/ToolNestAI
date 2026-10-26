package app.readit.ui

import android.Manifest
import android.content.ComponentName
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.activity.viewModels
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.LibraryBooks
import androidx.compose.material.icons.outlined.GraphicEq
import androidx.compose.material.icons.outlined.Home
import androidx.compose.material.icons.outlined.RecordVoiceOver
import androidx.compose.material.icons.outlined.Settings
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.NavigationRail
import androidx.compose.material3.NavigationRailItem
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.unit.Density
import androidx.compose.ui.unit.dp
import androidx.compose.runtime.CompositionLocalProvider
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.media3.session.MediaController
import androidx.media3.session.SessionToken
import androidx.navigation.NavHostController
import androidx.navigation.NavGraph.Companion.findStartDestination
import androidx.navigation.compose.NavHost
import androidx.navigation.compose.composable
import androidx.navigation.compose.currentBackStackEntryAsState
import androidx.navigation.compose.rememberNavController
import app.readit.playback.PlaybackService
import app.readit.ui.screens.BookmarksScreen
import app.readit.ui.screens.HelpScreen
import app.readit.ui.screens.HomeScreen
import app.readit.ui.screens.LibraryScreen
import app.readit.ui.screens.OnboardingScreen
import app.readit.ui.screens.PronunciationScreen
import app.readit.ui.screens.ProfilesScreen
import app.readit.ui.screens.ReaderScreen
import app.readit.ui.screens.SettingsScreen
import app.readit.ui.screens.StorageScreen
import app.readit.ui.screens.VoicesScreen
import app.readit.ui.theme.ReaditTheme
import com.google.common.util.concurrent.ListenableFuture

private data class Tab(val route: String, val label: String, val icon: ImageVector)

private val tabs = listOf(
    Tab("home", "Home", Icons.Outlined.Home),
    Tab("library", "Library", Icons.AutoMirrored.Outlined.LibraryBooks),
    Tab("listen", "Listen", Icons.Outlined.GraphicEq),
    Tab("voices", "Voices", Icons.Outlined.RecordVoiceOver),
    Tab("settings", "Settings", Icons.Outlined.Settings),
)

class MainActivity : ComponentActivity() {
    private val vm: AppViewModel by viewModels()
    private var controllerFuture: ListenableFuture<MediaController>? = null
    private var pendingNav: ((NavHostController) -> Unit)? = null
    private var nav: NavHostController? = null

    private val notificationPermission = registerForActivityResult(ActivityResultContracts.RequestPermission()) {}

    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)
        if (Build.VERSION.SDK_INT >= 33) notificationPermission.launch(Manifest.permission.POST_NOTIFICATIONS)
        handleIntent(intent)
        setContent { Root() }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        handleIntent(intent)
        nav?.let { n -> pendingNav?.invoke(n); pendingNav = null }
    }

    override fun onStart() {
        super.onStart()
        // Binding a controller starts the media service, which keeps reading alive in the background.
        controllerFuture = MediaController.Builder(this, SessionToken(this, ComponentName(this, PlaybackService::class.java))).buildAsync()
    }

    override fun onStop() {
        controllerFuture?.let { MediaController.releaseFuture(it) }
        controllerFuture = null
        super.onStop()
    }

    /** Files from "Open with" and the share menu go straight into the library and open. */
    private fun handleIntent(intent: Intent?) {
        intent ?: return
        if (intent.getBooleanExtra(EXTRA_OPEN_PLAYER, false)) {
            pendingNav = { it.navigate("listen") { launchSingleTop = true } }
            return
        }
        val uris: List<Uri> = when (intent.action) {
            Intent.ACTION_VIEW -> listOfNotNull(intent.data)
            Intent.ACTION_SEND -> listOfNotNull(
                if (Build.VERSION.SDK_INT >= 33) intent.getParcelableExtra(Intent.EXTRA_STREAM, Uri::class.java)
                else @Suppress("DEPRECATION") intent.getParcelableExtra(Intent.EXTRA_STREAM),
            )
            Intent.ACTION_SEND_MULTIPLE -> (
                if (Build.VERSION.SDK_INT >= 33) intent.getParcelableArrayListExtra(Intent.EXTRA_STREAM, Uri::class.java)
                else @Suppress("DEPRECATION") intent.getParcelableArrayListExtra(Intent.EXTRA_STREAM)
                ).orEmpty()
            else -> emptyList()
        }
        if (uris.isEmpty()) {
            // Shared plain text (e.g. from a browser) arrives as text, not a file.
            val text = intent.getStringExtra(Intent.EXTRA_TEXT)
            if (intent.action == Intent.ACTION_SEND && !text.isNullOrBlank()) {
                val f = java.io.File(cacheDir, "shared-text.txt").apply { writeText(text) }
                vm.import(listOf(Uri.fromFile(f))) { openDoc(it.id) }
            }
            return
        }
        vm.import(uris) { openDoc(it.id) }
        intent.action = null
    }

    private fun openDoc(id: String) {
        vm.session.open(id)
        val go: (NavHostController) -> Unit = { it.navigate("listen") { launchSingleTop = true } }
        nav?.let(go) ?: run { pendingNav = go }
    }

    @Composable
    private fun Root() {
        val settings by vm.settings.collectAsStateWithLifecycle()
        ReaditTheme(themeMode = settings.themeMode, highContrast = settings.highContrast) {
            val density = LocalDensity.current
            CompositionLocalProvider(LocalDensity provides Density(density.density, density.fontScale * settings.textScale)) {
                if (!settings.onboardingDone) {
                    OnboardingScreen(vm, onDone = { vm.setSetting("onboarding_done", "1") })
                } else {
                    Shell()
                }
            }
        }
    }

    @Composable
    private fun Shell() {
        val navController = rememberNavController()
        LaunchedEffect(navController) {
            nav = navController
            pendingNav?.invoke(navController)
            pendingNav = null
        }
        val snackbar = remember { SnackbarHostState() }
        LaunchedEffect(Unit) { vm.messages.collect { snackbar.showSnackbar(it) } }
        val now by vm.session.now.collectAsStateWithLifecycle()
        LaunchedEffect(now.message) {
            now.message?.let { snackbar.showSnackbar(it); vm.session.consumeMessage() }
        }
        val wide = LocalConfiguration.current.screenWidthDp >= 600
        val backStack by navController.currentBackStackEntryAsState()
        val route = backStack?.destination?.route
        val go: (String) -> Unit = { r ->
            navController.navigate(r) {
                popUpTo(navController.graph.findStartDestination().id) { saveState = true }
                launchSingleTop = true
                restoreState = true
            }
        }
        Scaffold(
            snackbarHost = { SnackbarHost(snackbar) },
            bottomBar = {
                if (!wide) {
                    NavigationBar {
                        tabs.forEach { t ->
                            NavigationBarItem(selected = route == t.route, onClick = { go(t.route) }, icon = { Icon(t.icon, null) }, label = { Text(t.label) })
                        }
                    }
                }
            },
        ) { pad ->
            Row(Modifier.fillMaxSize().padding(pad)) {
                if (wide) {
                    // Tablets and landscape: a side rail leaves a wide reading area.
                    NavigationRail(containerColor = MaterialTheme.colorScheme.surfaceContainer) {
                        tabs.forEach { t ->
                            NavigationRailItem(selected = route == t.route, onClick = { go(t.route) }, icon = { Icon(t.icon, null) }, label = { Text(t.label) })
                        }
                    }
                }
                Box(Modifier.weight(1f)) {
                    NavHost(navController, startDestination = "home") {
                        composable("home") { HomeScreen(vm, onOpen = ::openDoc, onNavigate = go) }
                        composable("library") { LibraryScreen(vm, onOpen = ::openDoc) }
                        composable("listen") { ReaderScreen(vm, onNavigate = { navController.navigate(it) }) }
                        composable("voices") { VoicesScreen(vm) }
                        composable("settings") { SettingsScreen(vm, onNavigate = { navController.navigate(it) }) }
                        composable("bookmarks") { BookmarksScreen(vm, onBack = { navController.popBackStack() }) }
                        composable("pronunciation") { PronunciationScreen(vm, onBack = { navController.popBackStack() }) }
                        composable("profiles") { ProfilesScreen(vm, onBack = { navController.popBackStack() }) }
                        composable("storage") { StorageScreen(vm, onBack = { navController.popBackStack() }) }
                        composable("help") { HelpScreen(onBack = { navController.popBackStack() }) }
                    }
                }
            }
        }
    }

    companion object {
        const val EXTRA_OPEN_PLAYER = "open_player"
    }
}
