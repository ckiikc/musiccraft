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
class PlaybackCoordinator(private val player:ExoPlayer,private val catalog:Catalog,private val store:ListeningStore,private val publish:(Bundle)->Unit) {
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
    private var failures=mutableSetOf<String>()
    private var consecutiveFailures=0
    private var message=""
    private val listener=object:Player.Listener {
        override fun onMediaItemTransition(item:MediaItem?,reason:Int){account();current=item?.mediaId;listened=0;credited=false;lastTick=SystemClock.elapsedRealtime();schedule()}
        override fun onEvents(p:Player,events:Player.Events){account();wasPlaying=p.isPlaying;if(p.isPlaying)consecutiveFailures=0;if(events.contains(Player.EVENT_TIMELINE_CHANGED) || events.contains(Player.EVENT_MEDIA_ITEM_TRANSITION)) schedule()}
        override fun onPlayerError(error:PlaybackException){
            consecutiveFailures++
            player.currentMediaItem?.mediaId?.let{failures+=it};message="Brano non disponibile: saltato. Aggiorna la libreria se il file è stato eliminato.";notifyState()
            handler.post {if(closed)return@post;if(player.hasNextMediaItem() && consecutiveFailures<=20){player.seekToNextMediaItem();player.prepare();player.play()}else{player.pause();stopMix();save()}}
        }
    }
    private val tick=object:Runnable {override fun run(){if(closed)return;account();wasPlaying=player.isPlaying;if(++saves%5==0)save();handler.postDelayed(this,1000)}}
    init {player.addListener(listener);handler.post(tick)}
    private fun account(){
        val now=SystemClock.elapsedRealtime();if(wasPlaying)listened+=(now-lastTick).coerceIn(0,5000);lastTick=now
        val song=catalog.songs.find{it.id==current} ?: return
        val threshold=minOf(30000L,(song.duration/2).coerceAtLeast(1000))
        if(!credited && listened>=threshold){credited=true;store.played(song.statKey);notifyState()}
    }
    private fun notifyState(){publish(Bundle().apply{putString("mix_id",active?.id);putString("mix_name",active?.name);putString("message",message);putLong("revision",store.revision)})}
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
        if(kept.isEmpty()){notifyState();return}
        val selected=kept.indexOfFirst{it.first>=snapshot.index}.let{if(it<0)kept.lastIndex else it}
        active=store.rules.find{it.id==snapshot.ruleId};cursor=MixCursor(snapshot.pending,snapshot.seen)
        player.setMediaItems(kept.map{it.second.item()},selected,if(kept[selected].first==snapshot.index)snapshot.position else 0)
        player.prepare();player.pause();notifyState()
    }
    fun userQueue(){restoreAllowed=false;initialized=true;stopMix();failures.clear();consecutiveFailures=0}
    fun stopMix(){active=null;cursor=MixCursor();notifyState()}
    fun prepareMix(id:String):List<MediaItem>{
        val rule=store.rules.find{it.id==id} ?: throw IllegalArgumentException("Playlist non disponibile")
        val next=MixCursor();val ids=next.take(catalog.songs.map{it.facts()},store.stats,rule,emptySet(),8)
        require(ids.isNotEmpty()){ "Nessun brano corrisponde ai criteri" }
        restoreAllowed=false;initialized=true;active=rule;cursor=next;failures.clear();consecutiveFailures=0;message=""
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
                    "replace"->{userQueue();player.shuffleModeEnabled=false;player.repeatMode=Player.REPEAT_MODE_OFF;player.setMediaItems(items);player.prepare();player.play()}
                    else->error("Operazione non valida")
                }
            }
            "jump"->{val i=args.getInt("index",-1);require(i in 0 until player.mediaItemCount);player.seekToDefaultPosition(i);player.prepare();player.play()}
            "remove"->{val i=args.getInt("index",-1);require(i in 0 until player.mediaItemCount);player.removeMediaItem(i);if(player.mediaItemCount==0)stopMix()}
            "move"->{val from=args.getInt("from",-1);val to=args.getInt("to",-1);require(from in 0 until player.mediaItemCount && to in 0 until player.mediaItemCount);player.moveMediaItem(from,to)}
            "clear"->{userQueue();player.stop();player.clearMediaItems()}
            "rate"->{val song=catalog.songs.find{it.id==args.getString("id")} ?: error("Brano non disponibile");store.rate(song.statKey,args.getInt("rating",-1))}
            "save_queue"->{store.putPlaylist(args.getString("name").orEmpty(),(0 until player.mediaItemCount).map{player.getMediaItemAt(it).mediaId})}
            else->error("Comando non disponibile")
        }
        initialized=true;schedule();save();notifyState()
    }
    private fun save(){if(!initialized || !catalog.permitted())return;store.saveQueue(QueueSnapshot((0 until player.mediaItemCount).map{player.getMediaItemAt(it).mediaId},player.currentMediaItemIndex.coerceAtLeast(0),player.currentPosition.coerceAtLeast(0),active?.id,cursor.pending.toList(),cursor.seen.toSet()))}
    fun close(){account();save();closed=true;handler.removeCallbacksAndMessages(null);player.removeListener(listener)}
}
