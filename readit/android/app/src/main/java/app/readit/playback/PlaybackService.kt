package app.readit.playback

import android.app.PendingIntent
import android.content.Intent
import android.os.Looper
import androidx.media3.common.MediaItem
import androidx.media3.common.MediaMetadata
import androidx.media3.common.Player
import androidx.media3.common.SimpleBasePlayer
import androidx.annotation.OptIn
import androidx.media3.common.util.UnstableApi
import androidx.media3.session.MediaSession
import androidx.media3.session.MediaSessionService
import app.readit.ReaditApp
import app.readit.ui.MainActivity
import com.google.common.util.concurrent.Futures
import com.google.common.util.concurrent.ListenableFuture

/**
 * Keeps reading while the screen is off or another app is open, and provides
 * the media notification, lock-screen controls and Bluetooth/headset buttons.
 * It mirrors [ReaderSession]; it has no playback logic of its own.
 */
class PlaybackService : MediaSessionService() {
    private var mediaSession: MediaSession? = null

    @OptIn(UnstableApi::class)
    override fun onCreate() {
        super.onCreate()
        val session = ReaditApp.instance.session
        val player = SessionPlayer(session)
        val open = PendingIntent.getActivity(
            this, 0,
            Intent(this, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP).putExtra(MainActivity.EXTRA_OPEN_PLAYER, true),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
        mediaSession = MediaSession.Builder(this, player).setSessionActivity(open).build()
    }

    override fun onGetSession(controllerInfo: MediaSession.ControllerInfo): MediaSession? = mediaSession

    override fun onTaskRemoved(rootIntent: Intent?) {
        // Swiping the app away while paused ends the session; while playing, reading continues.
        if (!ReaditApp.instance.session.now.value.isPlaying) stopSelf()
    }

    override fun onDestroy() {
        mediaSession?.run {
            player.release()
            release()
        }
        mediaSession = null
        super.onDestroy()
    }
}

/** Presents the reading session to Media3 as a player with one item: the open document. */
@OptIn(UnstableApi::class)
private class SessionPlayer(private val session: ReaderSession) : SimpleBasePlayer(Looper.getMainLooper()) {
    private val onChange: () -> Unit = { invalidateState() }

    init {
        session.addChangeListener(onChange)
    }

    override fun getState(): State {
        val n = session.now.value
        val commands = Player.Commands.Builder()
            .addAll(
                Player.COMMAND_PLAY_PAUSE, Player.COMMAND_STOP, Player.COMMAND_SEEK_TO_NEXT,
                Player.COMMAND_SEEK_TO_PREVIOUS, Player.COMMAND_SEEK_BACK, Player.COMMAND_SEEK_FORWARD,
                Player.COMMAND_GET_CURRENT_MEDIA_ITEM, Player.COMMAND_GET_METADATA, Player.COMMAND_RELEASE,
            )
            .build()
        val builder = State.Builder()
            .setAvailableCommands(commands)
            .setSeekBackIncrementMs(session.skipIntervalSeconds() * 1000L)
            .setSeekForwardIncrementMs(session.skipIntervalSeconds() * 1000L)
            .setPlayWhenReady(n.isPlaying, Player.PLAY_WHEN_READY_CHANGE_REASON_USER_REQUEST)
        val doc = n.document ?: return builder.setPlaybackState(Player.STATE_IDLE).build()
        val metadata = MediaMetadata.Builder()
            .setTitle(doc.title)
            .setArtist(n.locationLabel.ifEmpty { "READIT" })
            .setSubtitle(n.segment?.displayText ?: n.section?.text?.take(120))
            .build()
        val item = MediaItemData.Builder(doc.id)
            .setMediaItem(MediaItem.Builder().setMediaId(doc.id).setMediaMetadata(metadata).build())
            .setMediaMetadata(metadata)
            .build()
        return builder
            .setPlaylist(listOf(item))
            .setCurrentMediaItemIndex(0)
            .setPlaybackState(
                when (n.state) {
                    PlayState.PREPARING -> Player.STATE_BUFFERING
                    PlayState.ENDED -> Player.STATE_ENDED
                    else -> Player.STATE_READY
                },
            )
            .build()
    }

    override fun handleSetPlayWhenReady(playWhenReady: Boolean): ListenableFuture<*> {
        if (playWhenReady) session.play() else session.pause()
        return Futures.immediateVoidFuture()
    }

    override fun handleStop(): ListenableFuture<*> {
        session.stop()
        return Futures.immediateVoidFuture()
    }

    override fun handleSeek(mediaItemIndex: Int, positionMs: Long, seekCommand: Int): ListenableFuture<*> {
        when (seekCommand) {
            Player.COMMAND_SEEK_TO_NEXT, Player.COMMAND_SEEK_TO_NEXT_MEDIA_ITEM -> session.nextParagraph()
            Player.COMMAND_SEEK_TO_PREVIOUS, Player.COMMAND_SEEK_TO_PREVIOUS_MEDIA_ITEM -> session.previousParagraph()
            Player.COMMAND_SEEK_BACK -> session.skipSeconds(-session.skipIntervalSeconds())
            Player.COMMAND_SEEK_FORWARD -> session.skipSeconds(session.skipIntervalSeconds())
        }
        return Futures.immediateVoidFuture()
    }

    override fun handleRelease(): ListenableFuture<*> {
        session.removeChangeListener(onChange)
        return Futures.immediateVoidFuture()
    }
}
