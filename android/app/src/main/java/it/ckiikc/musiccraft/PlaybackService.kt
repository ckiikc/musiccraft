package it.ckiikc.musiccraft

import android.app.PendingIntent
import android.content.Intent
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import androidx.media3.common.AudioAttributes
import androidx.media3.common.C
import androidx.media3.common.MediaMetadata
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
    private var destroyed=false
    private lateinit var session: MediaLibrarySession
    private lateinit var player: ExoPlayer
    private lateinit var coordinator: PlaybackCoordinator
    private lateinit var catalog: Catalog
    private lateinit var carMonitor:CarRouteMonitor
    private val main = Executor { Handler(Looper.getMainLooper()).post(it) }
    override fun onCreate() {
        super.onCreate()
        catalog = Catalog.get(this)
        ListeningStore.get(this).selectProfile(CarQueues.PHONE)
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
        coordinator=PlaybackCoordinator(this,player,catalog,ListeningStore.get(this)){
            session.setSessionExtras(it)
            session.notifyChildrenChanged("root",4,null)
            for(limit in 1..3)session.notifyChildrenChanged("root:$limit",limit,null)
            session.notifyChildrenChanged("playlist-menu",3,null)
            session.notifyChildrenChanged("now-playing",player.mediaItemCount,null)
            session.notifyChildrenChanged("cars",ListeningStore.get(this).cars.size+1,null)
            session.notifyChildrenChanged("playlists",ListeningStore.get(this).playlists.size,null)
            session.notifyChildrenChanged("dynamic",ListeningStore.get(this).rules.size,null)
        }
        carMonitor=CarRouteMonitor(this,ListeningStore.get(this),coordinator)
        val initial=catalog.refresh();initial.addListener({if(!destroyed)try{initial.get();coordinator.restore();carMonitor.start()}catch(_:Exception){}},main)
    }
    override fun onGetSession(controllerInfo: MediaSession.ControllerInfo): MediaLibrarySession = session
    override fun onDestroy() {
        destroyed=true
        carMonitor.close()
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
    private fun browseChildren(parent:String):List<MediaItem> {
        val store=ListeningStore.get(this)
        return when(parent){
            "root","root:1","root:2","root:3","browse"->{
                val now=catalog.folder("now-playing","In ascolto",MediaMetadata.MEDIA_TYPE_FOLDER_PLAYLISTS).let{it.buildUpon().setMediaMetadata(it.mediaMetadata.buildUpon().setSubtitle("${coordinator.name} · ${coordinator.profileName}").build()).build()}
                val lists=catalog.folder("playlist-menu","Playlist",MediaMetadata.MEDIA_TYPE_FOLDER_PLAYLISTS)
                val mix=catalog.folder("dynamic","Mix",MediaMetadata.MEDIA_TYPE_FOLDER_PLAYLISTS)
                val library=catalog.folder("library","Libreria",MediaMetadata.MEDIA_TYPE_FOLDER_MIXED)
                val cars=catalog.folder("cars","Auto e code",MediaMetadata.MEDIA_TYPE_FOLDER_MIXED)
                when(parent){"root:1"->listOf(catalog.folder("browse","Musica",MediaMetadata.MEDIA_TYPE_FOLDER_MIXED));"root:2"->listOf(lists,library);"root:3"->listOf(now,lists,library);else->listOf(now,mix,catalog.folder("playlists","Playlist",MediaMetadata.MEDIA_TYPE_FOLDER_PLAYLISTS),library)}
            }
            "playlist-menu"->listOf(browseChildren("root").first())+catalog.children("root").filter{it.mediaId in listOf("dynamic","playlists")}
            "library"->catalog.children("root").filter{it.mediaId in listOf("songs","albums","artists")}+catalog.folder("cars","Auto e code",MediaMetadata.MEDIA_TYPE_FOLDER_MIXED)
            "now-playing"->(0 until player.mediaItemCount).map{i->
                val song=player.getMediaItemAt(i);val meta=song.mediaMetadata
                song.buildUpon().setMediaId(CarQueues.queueEntry(coordinator.profileId,coordinator.queueRevision,i)).setMediaMetadata(meta.buildUpon().setDisplayTitle((if(i==player.currentMediaItemIndex)"▶ " else "")+"${i+1}. ${meta.title}").setSubtitle("${coordinator.name} · ${coordinator.profileName}").build()).build()
            }
            "cars"->(listOf(CarProfile(CarQueues.PHONE,"Telefono"))+store.cars).map{c->
                val q=if(c.id==coordinator.profileId)QueueSnapshot((0 until player.mediaItemCount).map{player.getMediaItemAt(it).mediaId},name=coordinator.name) else store.queues[c.id] ?: QueueSnapshot()
                MediaItem.Builder().setMediaId("profile:${c.id}").setMediaMetadata(MediaMetadata.Builder().setTitle((if(c.id==coordinator.profileId)"✓ " else "")+c.name).setSubtitle("${q.name} · ${q.ids.size} brani").setIsBrowsable(false).setIsPlayable(true).setMediaType(MediaMetadata.MEDIA_TYPE_PLAYLIST).build()).build()
            }
            else->catalog.children(parent)
        }
    }
    private fun browseItem(id:String):MediaItem? = when {
        id=="cars"->catalog.folder("cars","Auto e code",MediaMetadata.MEDIA_TYPE_FOLDER_MIXED)
        id in listOf("now-playing","playlist-menu","library")->browseChildren("root").find{it.mediaId==id}
        id.startsWith("root:")->catalog.folder(id,"MusicCraft",MediaMetadata.MEDIA_TYPE_FOLDER_MIXED)
        id=="browse"->browseChildren("root:1").single()
        id.startsWith("queue-entry:")->browseChildren("now-playing").find{it.mediaId==id}
        id.startsWith("profile:")->browseChildren("cars").find{it.mediaId==id}
        else->catalog.item(id)
    }
    private inner class LibraryCallback : MediaLibrarySession.Callback {
        override fun onConnect(session: MediaSession, controller: MediaSession.ControllerInfo): MediaSession.ConnectionResult {
            if (controller.packageName != packageName && !controller.isTrusted && !session.isMediaNotificationController(controller)) return MediaSession.ConnectionResult.reject()
            if(controller.packageName!=packageName)try{grantUriPermission(controller.packageName,android.net.Uri.parse("content://${ArtworkLibrary.AUTHORITY}/cover"),Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_PREFIX_URI_PERMISSION)}catch(_:SecurityException){}
            val base=super.onConnect(session,controller)
            return if(controller.packageName==packageName) MediaSession.ConnectionResult.AcceptedResultBuilder(session).setAvailablePlayerCommands(MediaSession.ConnectionResult.DEFAULT_PLAYER_COMMANDS).setAvailableSessionCommands(MediaSession.ConnectionResult.DEFAULT_SESSION_AND_LIBRARY_COMMANDS.buildUpon().add(SessionCommand(PlaybackCommands.ACTION,Bundle.EMPTY)).build()).build() else base
        }
        override fun onCustomCommand(session:MediaSession,controller:MediaSession.ControllerInfo,customCommand:SessionCommand,args:Bundle):ListenableFuture<SessionResult> {
            if(controller.packageName!=packageName || customCommand.customAction!=PlaybackCommands.ACTION) return Futures.immediateFuture(SessionResult(SessionError.ERROR_PERMISSION_DENIED))
            val output=SettableFuture.create<SessionResult>();val scan=catalog.refresh()
            scan.addListener({try{scan.get();coordinator.command(args);if(args.getString("op")=="select_car")carMonitor.manualSelection();if(args.getString("op")=="automatic_cars")carMonitor.automaticChanged() else carMonitor.refresh();output.set(SessionResult(SessionResult.RESULT_SUCCESS))}catch(e:Exception){output.set(SessionResult(SessionError.ERROR_BAD_VALUE,Bundle().apply{putString("message",e.message ?: "Operazione non riuscita")}))}},main)
            return output
        }
        override fun onGetLibraryRoot(session: MediaLibrarySession, browser: MediaSession.ControllerInfo, params: LibraryParams?): ListenableFuture<LibraryResult<MediaItem>> {
            val limit=params?.extras?.getInt(androidx.media3.session.MediaConstants.EXTRAS_KEY_ROOT_CHILDREN_LIMIT,4)?.coerceIn(1,4) ?: 4
            val id=if(limit==4)"root" else "root:$limit"
            return Futures.immediateFuture(LibraryResult.ofItem(catalog.folder(id,if(catalog.permitted())"MusicCraft" else "Autorizza la musica sul telefono",MediaMetadata.MEDIA_TYPE_FOLDER_MIXED),params))
        }
        override fun onGetChildren(session: MediaLibrarySession, browser: MediaSession.ControllerInfo, parentId: String, page: Int, pageSize: Int, params: LibraryParams?): ListenableFuture<LibraryResult<ImmutableList<MediaItem>>> = result {
            if (!catalog.permitted()) LibraryResult.ofError(SessionError.ERROR_PERMISSION_DENIED)
            else {
                val children = browseChildren(parentId)
                val range = LibraryRules.pageBounds(children.size, page, pageSize)
                if (range == null) LibraryResult.ofError(SessionError.ERROR_BAD_VALUE)
                else LibraryResult.ofItemList(range.map { children[it] }, params)
            }
        }
        override fun onGetItem(session: MediaLibrarySession, browser: MediaSession.ControllerInfo, mediaId: String): ListenableFuture<LibraryResult<MediaItem>> = result {
            browseItem(mediaId)?.let { LibraryResult.ofItem(it, null) } ?: LibraryResult.ofError(SessionError.ERROR_BAD_VALUE)
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
        override fun onSubscribe(session:MediaLibrarySession,browser:MediaSession.ControllerInfo,parentId:String,params:LibraryParams?):ListenableFuture<LibraryResult<Void>> = Futures.immediateFuture(LibraryResult.ofVoid())
        override fun onSetMediaItems(session: MediaSession, controller: MediaSession.ControllerInfo, mediaItems: List<MediaItem>, startIndex: Int, startPositionMs: Long): ListenableFuture<MediaSession.MediaItemsWithStartPosition> {
            val single=mediaItems.singleOrNull()?.mediaId
            if(single?.startsWith("profile:")==true || single?.startsWith("queue-entry:")==true){
                val out=SettableFuture.create<MediaSession.MediaItemsWithStartPosition>();val scan=catalog.refresh()
                scan.addListener({try{
                    scan.get()
                    val index:Int;val position:Long
                    if(single.startsWith("profile:")){
                        coordinator.switchProfile(single.removePrefix("profile:"));carMonitor.manualSelection()
                        index=player.currentMediaItemIndex.coerceAtLeast(0);position=player.currentPosition.coerceAtLeast(0)
                    }else{index=CarQueues.queueIndex(single,coordinator.profileId,coordinator.queueRevision,player.mediaItemCount) ?: error("La coda è cambiata: riaprila");position=0}
                    val items=(0 until player.mediaItemCount).map{player.getMediaItemAt(it)}
                    out.set(MediaSession.MediaItemsWithStartPosition(items,index,position))
                }catch(e:Exception){out.setException(e)}},main);return out
            }
            return Futures.transform(onAddMediaItems(session, controller, mediaItems), { resolved ->
                val curated=mediaItems.any{it.mediaId.startsWith("mix:") || it.mediaId.startsWith("playlist:")}
                val items = requireNotNull(resolved)
                val automotive=session.isAutomotiveController(controller) || session.isAutoCompanionController(controller)
                if(!curated)coordinator.userQueue(if(automotive && items.size==1)"Tutti i brani" else if(mediaItems.any{it.requestMetadata.searchQuery!=null})"Risultati della ricerca" else "Coda manuale")
                if(items.size==1 && !curated && automotive){
                    val queue=catalog.songs.map{it.item()}
                    MediaSession.MediaItemsWithStartPosition(queue,queue.indexOfFirst{it.mediaId==items[0].mediaId}.coerceAtLeast(0),startPositionMs.coerceAtLeast(0))
                }else MediaSession.MediaItemsWithStartPosition(items,startIndex.coerceAtLeast(0),startPositionMs.coerceAtLeast(0))
            },main)
        }
        override fun onAddMediaItems(session: MediaSession, controller: MediaSession.ControllerInfo, mediaItems: List<MediaItem>): ListenableFuture<List<MediaItem>> {
            // Resolve IDs using our catalog, never arbitrary URIs from another app.
            val output = SettableFuture.create<List<MediaItem>>()
            val scan = catalog.refresh()
            scan.addListener({
                try {
                    scan.get()
                    require(mediaItems.none{it.mediaId.startsWith("mix:") || it.mediaId.startsWith("playlist:")} || mediaItems.size==1)
                    val resolved = mediaItems.flatMap { item ->
                        if(item.mediaId.startsWith("mix:")) return@flatMap coordinator.prepareMix(item.mediaId.removePrefix("mix:"))
                        if(item.mediaId.startsWith("playlist:")) return@flatMap coordinator.preparePlaylist(item.mediaId.removePrefix("playlist:"))
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
