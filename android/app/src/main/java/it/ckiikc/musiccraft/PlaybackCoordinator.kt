package it.ckiikc.musiccraft

import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import androidx.media3.common.*
import androidx.media3.exoplayer.ExoPlayer

object PlaybackCommands {const val ACTION="it.ckiikc.musiccraft.PLAYBACK"}

/** The service owns both the real queue and its dynamic producer. All player access is on main. */
@androidx.annotation.OptIn(androidx.media3.common.util.UnstableApi::class)
class PlaybackCoordinator(private val context:android.content.Context,private val player:ExoPlayer,private val catalog:Catalog,private val store:ListeningStore,private val publish:(Bundle)->Unit) {
    private val handler=Handler(Looper.getMainLooper())
    private var active:SmartRule?=null
    private var cursor=MixCursor()
    private var initialized=false
    private var restoreAllowed=true
    private var closed=false
    private var pending=false
    private var current:String?=null
    private var listened=0L
    private var credited=false
    private var lastTick=SystemClock.elapsedRealtime()
    private var wasPlaying=false
    private var saves=0
    private var observedLibraryRevision=-1L
    private var failures=mutableSetOf<String>()
    private var consecutiveFailures=0
    private var message=""
    private var queueName="Coda manuale"
    private var sourceId:String?=null
    private var generation=0L
    var queueRevision=0L;private set
    private var routeNotice=""
    val profileId:String get()=store.activeProfile
    val profileName:String get()=CarQueues.profileName(profileId,store.cars)
    val name:String get()=queueName
    fun routingNotice(text:String){if(routeNotice!=text){routeNotice=text;notifyState()}}
    private val listener=object:Player.Listener {
        override fun onMediaItemTransition(item:MediaItem?,reason:Int){account();current=item?.mediaId;listened=0;credited=false;lastTick=SystemClock.elapsedRealtime();schedule()}
        override fun onEvents(p:Player,events:Player.Events){if(events.contains(Player.EVENT_TIMELINE_CHANGED))queueRevision++;account();wasPlaying=p.isPlaying;if(p.isPlaying)consecutiveFailures=0;if(events.contains(Player.EVENT_TIMELINE_CHANGED) || events.contains(Player.EVENT_MEDIA_ITEM_TRANSITION)) schedule()}
        override fun onPlayerError(error:PlaybackException){
            consecutiveFailures++
            player.currentMediaItem?.mediaId?.let{failures+=it};message="Brano non disponibile: saltato. Aggiorna la libreria se il file è stato eliminato.";notifyState()
            val failedGeneration=generation
            handler.post {if(closed || failedGeneration!=generation)return@post;if(player.hasNextMediaItem() && consecutiveFailures<=20){player.seekToNextMediaItem();player.prepare();player.play()}else{player.pause();stopMix();save()}}
        }
    }
    private val tick=object:Runnable {override fun run(){if(closed)return;account();wasPlaying=player.isPlaying;if(++saves%5==0)save();if(observedLibraryRevision!=store.libraryRevision){observedLibraryRevision=store.libraryRevision;notifyState()};handler.postDelayed(this,1000)}}
    init {player.addListener(listener);handler.post(tick)}
    private fun account(){
        val now=SystemClock.elapsedRealtime();if(wasPlaying)listened+=(now-lastTick).coerceIn(0,5000);lastTick=now
        val song=catalog.songs.find{it.id==current} ?: return
        val threshold=minOf(30000L,(song.duration/2).coerceAtLeast(1000))
        if(!credited && listened>=threshold){credited=true;store.played(song.statKey);notifyState()}
    }
    private fun notifyState(){
        val title="$queueName · $profileName"
        if(player.playlistMetadata.title?.toString()!=title)player.playlistMetadata=MediaMetadata.Builder().setTitle(title).setMediaType(MediaMetadata.MEDIA_TYPE_PLAYLIST).build()
        publish(Bundle().apply{putString("mix_id",active?.id);putString("mix_name",active?.name);putString("message",message);putLong("revision",store.revision);putString("queue_name",queueName);putString("queue_profile",profileId);putString("queue_profile_name",profileName);putString("car_notice",routeNotice)})
    }
    private fun schedule(){if(pending || closed)return;pending=true;handler.post{pending=false;if(!closed){refill();save();notifyState()}}}
    private fun refill(){
        val rule=active ?: return
        if(player.mediaItemCount==0){stopMix();return}
        val index=player.currentMediaItemIndex.coerceAtLeast(0)
        val remaining=player.mediaItemCount-index-1
        if(remaining>=6)return
        val exclude=(index until player.mediaItemCount).map{player.getMediaItemAt(it).mediaId}.toSet()+failures
        val ids=cursor.take(catalog.songs.filter{it.id !in failures}.map{it.facts()},store.stats,rule,exclude,8-remaining)
        val byId=catalog.songs.associateBy{it.id};if(ids.isNotEmpty())player.addMediaItems(ids.mapNotNull{byId[it]?.item()})
        if(index>20)player.removeMediaItems(0,index-20)
        if(ids.isEmpty() && remaining==0 && SmartRules.select(catalog.songs.map{it.facts()},store.stats,rule).none{it.id !in failures && it.id!=player.currentMediaItem?.mediaId}){stopMix();message="Nessun altro brano corrisponde ai criteri. La coda termina normalmente."}
    }
    fun restore(){
        if(!restoreAllowed || player.mediaItemCount!=0 || !catalog.permitted())return
        val snapshot=store.queue;val songs=catalog.songs.associateBy{it.id};val kept=snapshot.ids.mapIndexedNotNull{i,id->songs[id]?.let{i to it}}
        initialized=true
        queueName=snapshot.name;sourceId=snapshot.sourceId
        player.shuffleModeEnabled=snapshot.shuffle;player.repeatMode=snapshot.repeat
        if(kept.isEmpty()){notifyState();return}
        val selected=kept.indexOfFirst{it.first>=snapshot.index}.let{if(it<0)kept.lastIndex else it}
        active=store.rules.find{it.id==snapshot.ruleId};cursor=MixCursor(snapshot.pending,snapshot.seen)
        player.setMediaItems(kept.map{it.second.item()},selected,if(kept[selected].first==snapshot.index)snapshot.position else 0)
        player.prepare();player.pause();notifyState()
    }
    fun userQueue(name:String="Coda manuale",source:String?=null){generation++;restoreAllowed=false;initialized=true;queueName=name.take(100);sourceId=source;stopMix();failures.clear();consecutiveFailures=0}
    fun switchProfile(id:String){
        require(id==CarQueues.PHONE || store.cars.any{it.id==id}){"Profilo auto non disponibile"}
        if(id==profileId)return
        account();wasPlaying=false;player.pause();save();generation++;store.selectProfile(id)
        initialized=false;restoreAllowed=true;active=null;cursor=MixCursor();failures.clear();consecutiveFailures=0;message="";current=null;listened=0;credited=false
        player.stop();player.clearMediaItems();restore();notifyState()
    }
    fun preparePlaylist(id:String):List<MediaItem>{
        val playlist=store.playlists.find{it.id==id} ?: error("Playlist non disponibile")
        val byId=catalog.songs.associateBy{it.id};val items=playlist.ids.mapNotNull{byId[it]?.item()};require(items.isNotEmpty()){"Nessun file disponibile"}
        userQueue(playlist.name,"playlist:$id");player.shuffleModeEnabled=false;player.repeatMode=Player.REPEAT_MODE_OFF;return items
    }
    fun stopMix(){active=null;cursor=MixCursor();notifyState()}
    fun prepareMix(id:String):List<MediaItem>{
        val rule=store.rules.find{it.id==id} ?: throw IllegalArgumentException("Playlist non disponibile")
        val next=MixCursor();val ids=next.take(catalog.songs.map{it.facts()},store.stats,rule,emptySet(),8)
        require(ids.isNotEmpty()){ "Nessun brano corrisponde ai criteri" }
        generation++;queueName=rule.name;sourceId="mix:${rule.id}";restoreAllowed=false;initialized=true;active=rule;cursor=next;failures.clear();consecutiveFailures=0;message=""
        player.shuffleModeEnabled=false;player.repeatMode=Player.REPEAT_MODE_OFF
        val byId=catalog.songs.associateBy{it.id};return ids.mapNotNull{byId[it]?.item()}
    }
    fun command(args:Bundle){
        val op=args.getString("op");restoreAllowed=false
        when(op){
            "start_mix"->{val items=prepareMix(args.getString("id").orEmpty());player.setMediaItems(items);player.prepare();player.play()}
            "stop_mix"->stopMix()
            "play_ids","play_playlist"->{
                val ids=if(op=="play_playlist")store.playlists.find{it.id==args.getString("id")}?.ids ?: error("Playlist non disponibile") else args.getStringArrayList("ids") ?: emptyList()
                require(ids.size<=20000);val map=catalog.songs.associateBy{it.id};val items=ids.mapNotNull{map[it]?.item()};require(items.isNotEmpty()){ "Nessun file disponibile" }
                when(args.getString("mode","replace")){
                    "next"->{initialized=true;require(player.mediaItemCount+items.size<=20000);player.addMediaItems((player.currentMediaItemIndex+1).coerceIn(0,player.mediaItemCount),items);if(player.playbackState==Player.STATE_IDLE)player.prepare()}
                    "append"->{initialized=true;require(player.mediaItemCount+items.size<=20000);player.addMediaItems(items);if(player.playbackState==Player.STATE_IDLE)player.prepare()}
                    "replace"->{val playlist=if(op=="play_playlist")store.playlists.find{it.id==args.getString("id")} else null;userQueue(playlist?.name ?: args.getString("name","Coda manuale"),playlist?.let{"playlist:${it.id}"});player.shuffleModeEnabled=false;player.repeatMode=Player.REPEAT_MODE_OFF;player.setMediaItems(items);player.prepare();player.play()}
                    else->error("Operazione non valida")
                }
            }
            "jump"->{val i=args.getInt("index",-1);require(i in 0 until player.mediaItemCount);player.seekToDefaultPosition(i);player.prepare();player.play()}
            "remove"->{val i=args.getInt("index",-1);require(i in 0 until player.mediaItemCount);player.removeMediaItem(i);if(player.mediaItemCount==0)stopMix()}
            "move"->{val from=args.getInt("from",-1);val to=args.getInt("to",-1);require(from in 0 until player.mediaItemCount && to in 0 until player.mediaItemCount);player.moveMediaItem(from,to)}
            "clear"->{userQueue();player.stop();player.clearMediaItems()}
            "rate"->{val song=catalog.songs.find{it.id==args.getString("id")} ?: error("Brano non disponibile");store.rate(song.statKey,args.getInt("rating",-1));RatingTags.local(context,song.id.substringAfter(':'),args.getInt("rating",-1))}
            "select_car"->switchProfile(args.getString("id").orEmpty())
            "put_car"->store.putCar(args.getString("name").orEmpty(),args.getString("id") ?: "car:"+java.util.UUID.randomUUID(),args.getString("bluetooth"))
            "remove_car"->store.removeCar(args.getString("id").orEmpty())
            "automatic_cars"->store.setAutomaticCars(args.getBoolean("enabled"))
            "car_settings_changed"->Unit
            "save_queue"->{store.putPlaylist(args.getString("name").orEmpty(),(0 until player.mediaItemCount).map{player.getMediaItemAt(it).mediaId})}
            else->error("Comando non disponibile")
        }
        initialized=true;schedule();save();notifyState()
    }
    private fun save(){if(!initialized || !catalog.permitted())return;store.saveQueue(QueueSnapshot((0 until player.mediaItemCount).map{player.getMediaItemAt(it).mediaId},player.currentMediaItemIndex.coerceAtLeast(0),player.currentPosition.coerceAtLeast(0),active?.id,cursor.pending.toList(),cursor.seen.toSet(),queueName,sourceId,player.shuffleModeEnabled,player.repeatMode))}
    fun close(){account();save();closed=true;handler.removeCallbacksAndMessages(null);player.removeListener(listener)}
}
