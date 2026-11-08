package it.ckiikc.musiccraft

import android.app.PendingIntent
import android.content.Intent
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import androidx.media3.common.AudioAttributes
import androidx.media3.common.C
import androidx.media3.common.MediaItem
import androidx.media3.common.Player
import androidx.media3.exoplayer.ExoPlayer
import androidx.media3.session.*
import androidx.media3.session.MediaLibraryService.MediaLibrarySession
import com.google.common.collect.ImmutableList
import com.google.common.util.concurrent.Futures
import com.google.common.util.concurrent.ListenableFuture
import com.google.common.util.concurrent.SettableFuture
import java.util.concurrent.Executor

@androidx.annotation.OptIn(androidx.media3.common.util.UnstableApi::class)
class PlaybackService : MediaLibraryService() {
    private lateinit var session: MediaLibrarySession
    private lateinit var player: ExoPlayer
    private lateinit var coordinator: PlaybackCoordinator
    private lateinit var catalog: Catalog
    private val main = Executor { Handler(Looper.getMainLooper()).post(it) }
    override fun onCreate() {
        super.onCreate()
        catalog = Catalog.get(this)
        player = ExoPlayer.Builder(this).build().apply {
            setAudioAttributes(AudioAttributes.Builder().setUsage(C.USAGE_MEDIA).setContentType(C.AUDIO_CONTENT_TYPE_MUSIC).build(), true)
            setHandleAudioBecomingNoisy(true)
            setWakeMode(C.WAKE_MODE_LOCAL)
        }
        player.addListener(object:Player.Listener {
            override fun onEvents(player:Player,events:Player.Events) { SharingGate.localPlayback=player.playWhenReady && player.playbackState!=Player.STATE_ENDED && player.playbackState!=Player.STATE_IDLE }
        })
        session = MediaLibrarySession.Builder(this, player, LibraryCallback())
            .setSessionActivity(PendingIntent.getActivity(this, 0, Intent(this, MainActivity::class.java), PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE))
            .build()
        setMediaNotificationProvider(DefaultMediaNotificationProvider.Builder(this).build().apply { setSmallIcon(R.drawable.ic_notification) })
        coordinator=PlaybackCoordinator(player,catalog,ListeningStore.get(this)){session.setSessionExtras(it)}
        val initial=catalog.refresh();initial.addListener({try{initial.get();coordinator.restore()}catch(_:Exception){}},main)
    }
    override fun onGetSession(controllerInfo: MediaSession.ControllerInfo): MediaLibrarySession = session
    override fun onDestroy() {
        coordinator.close()
        session.release()
        player.release()
        SharingGate.localPlayback=false
        super.onDestroy()
    }
    override fun onTaskRemoved(rootIntent: Intent?) {
        // Playback belongs to the service, not the Activity/task.
        if (!player.playWhenReady || player.mediaItemCount == 0 || player.playbackState == Player.STATE_ENDED) stopSelf()
    }
    private fun <T : Any> result(block: () -> LibraryResult<T>): ListenableFuture<LibraryResult<T>> {
        val output = SettableFuture.create<LibraryResult<T>>()
        val scan = catalog.refresh()
        scan.addListener({
            try { scan.get(); output.set(block()) }
            catch (_: Exception) { output.set(LibraryResult.ofError<T>(SessionError.ERROR_IO)) }
        }, main)
        return output
    }
    private inner class LibraryCallback : MediaLibrarySession.Callback {
        override fun onConnect(session: MediaSession, controller: MediaSession.ControllerInfo): MediaSession.ConnectionResult {
            if (controller.packageName != packageName && !controller.isTrusted && !session.isMediaNotificationController(controller)) return MediaSession.ConnectionResult.reject()
            val base=super.onConnect(session,controller)
            return if(controller.packageName==packageName) MediaSession.ConnectionResult.AcceptedResultBuilder(session).setAvailablePlayerCommands(MediaSession.ConnectionResult.DEFAULT_PLAYER_COMMANDS).setAvailableSessionCommands(MediaSession.ConnectionResult.DEFAULT_SESSION_AND_LIBRARY_COMMANDS.buildUpon().add(SessionCommand(PlaybackCommands.ACTION,Bundle.EMPTY)).build()).build() else base
        }
        override fun onCustomCommand(session:MediaSession,controller:MediaSession.ControllerInfo,customCommand:SessionCommand,args:Bundle):ListenableFuture<SessionResult> {
            if(controller.packageName!=packageName || customCommand.customAction!=PlaybackCommands.ACTION) return Futures.immediateFuture(SessionResult(SessionError.ERROR_PERMISSION_DENIED))
            val output=SettableFuture.create<SessionResult>();val scan=catalog.refresh()
            scan.addListener({try{scan.get();coordinator.command(args);output.set(SessionResult(SessionResult.RESULT_SUCCESS))}catch(e:Exception){output.set(SessionResult(SessionError.ERROR_BAD_VALUE,Bundle().apply{putString("message",e.message ?: "Operazione non riuscita")}))}},main)
            return output
        }
        override fun onGetLibraryRoot(session: MediaLibrarySession, browser: MediaSession.ControllerInfo, params: LibraryParams?): ListenableFuture<LibraryResult<MediaItem>> =
            Futures.immediateFuture(LibraryResult.ofItem(catalog.folder("root", if (catalog.permitted()) "MusicCraft" else "Autorizza la musica sul telefono", androidx.media3.common.MediaMetadata.MEDIA_TYPE_FOLDER_MIXED), params))
        override fun onGetChildren(session: MediaLibrarySession, browser: MediaSession.ControllerInfo, parentId: String, page: Int, pageSize: Int, params: LibraryParams?): ListenableFuture<LibraryResult<ImmutableList<MediaItem>>> = result {
            if (!catalog.permitted()) LibraryResult.ofError(SessionError.ERROR_PERMISSION_DENIED)
            else {
                val children = catalog.children(parentId)
                val range = LibraryRules.pageBounds(children.size, page, pageSize)
                if (range == null) LibraryResult.ofError(SessionError.ERROR_BAD_VALUE)
                else LibraryResult.ofItemList(range.map { children[it] }, params)
            }
        }
        override fun onGetItem(session: MediaLibrarySession, browser: MediaSession.ControllerInfo, mediaId: String): ListenableFuture<LibraryResult<MediaItem>> = result {
            catalog.item(mediaId)?.let { LibraryResult.ofItem(it, null) } ?: LibraryResult.ofError(SessionError.ERROR_BAD_VALUE)
        }
        override fun onSearch(session: MediaLibrarySession, browser: MediaSession.ControllerInfo, query: String, params: LibraryParams?): ListenableFuture<LibraryResult<Void>> = result {
            val count = catalog.songs.count { LibraryRules.matches(it.title, it.artist, it.album, query) }
            session.notifySearchResultChanged(browser, query, count, params)
            LibraryResult.ofVoid()
        }
        override fun onGetSearchResult(session: MediaLibrarySession, browser: MediaSession.ControllerInfo, query: String, page: Int, pageSize: Int, params: LibraryParams?): ListenableFuture<LibraryResult<ImmutableList<MediaItem>>> = result {
            val found = catalog.songs.filter { LibraryRules.matches(it.title, it.artist, it.album, query) }
            val range = LibraryRules.pageBounds(found.size, page, pageSize)
            if (range == null) LibraryResult.ofError(SessionError.ERROR_BAD_VALUE) else LibraryResult.ofItemList(range.map { found[it].item() }, params)
        }
        override fun onSetMediaItems(session: MediaSession, controller: MediaSession.ControllerInfo, mediaItems: List<MediaItem>, startIndex: Int, startPositionMs: Long): ListenableFuture<MediaSession.MediaItemsWithStartPosition> =
            Futures.transform(onAddMediaItems(session, controller, mediaItems), { resolved ->
                if(mediaItems.none{it.mediaId.startsWith("mix:")}) coordinator.userQueue()
                val items = requireNotNull(resolved)
                // A car's single-item play request gets a navigable local queue.
                if (items.size == 1 && mediaItems.none{it.mediaId.startsWith("mix:")} && (session.isAutomotiveController(controller) || session.isAutoCompanionController(controller))) {
                    val queue = catalog.songs.map { it.item() }
                    MediaSession.MediaItemsWithStartPosition(queue, queue.indexOfFirst { it.mediaId == items[0].mediaId }.coerceAtLeast(0), startPositionMs.coerceAtLeast(0))
                } else MediaSession.MediaItemsWithStartPosition(items, startIndex.coerceAtLeast(0), startPositionMs.coerceAtLeast(0))
            }, main)
        override fun onAddMediaItems(session: MediaSession, controller: MediaSession.ControllerInfo, mediaItems: List<MediaItem>): ListenableFuture<List<MediaItem>> {
            // Resolve IDs using our catalog, never arbitrary URIs from another app.
            val output = SettableFuture.create<List<MediaItem>>()
            val scan = catalog.refresh()
            scan.addListener({
                try {
                    scan.get()
                    require(mediaItems.none{it.mediaId.startsWith("mix:")} || mediaItems.size==1)
                    val resolved = mediaItems.flatMap { item ->
                        if(item.mediaId.startsWith("mix:")) return@flatMap coordinator.prepareMix(item.mediaId.removePrefix("mix:"))
                        val query = item.requestMetadata.searchQuery
                        if (query != null) catalog.songs.filter { LibraryRules.matches(it.title,it.artist,it.album,query) }.map { it.item() }.also { require(it.isNotEmpty()) { "Nessun risultato" } }
                        else listOf(catalog.songs.find { it.id == item.mediaId }?.item() ?: throw IllegalArgumentException("Brano non disponibile"))
                    }
                    output.set(resolved)
                } catch (error: Exception) { output.setException(error) }
            }, main)
            return output
        }
    }
}
