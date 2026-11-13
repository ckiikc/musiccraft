package it.ckiikc.musiccraft

import android.app.Instrumentation
import android.content.ComponentName
import android.os.Bundle
import androidx.media3.common.*
import androidx.media3.session.*
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicReference

@androidx.annotation.OptIn(androidx.media3.common.util.UnstableApi::class)
class CarChecks(private val test:Instrumentation) {
    private fun <T> main(block:()->T):T {val value=AtomicReference<T>();val error=AtomicReference<Throwable>();test.runOnMainSync{try{value.set(block())}catch(e:Throwable){error.set(e)}};error.get()?.let{throw it};return value.get()}
    private fun await(message:String,check:()->Boolean){val end=System.currentTimeMillis()+15000;while(System.currentTimeMillis()<end){if(check())return;Thread.sleep(80)};error(message)}
    private fun send(b:MediaBrowser,op:String,extra:Bundle.()->Unit={}):SessionResult=main{b.sendCustomCommand(SessionCommand(PlaybackCommands.ACTION,Bundle.EMPTY),Bundle().apply{putString("op",op);extra()})}.get(15,TimeUnit.SECONDS)
    private fun ids(b:MediaBrowser)=main{(0 until b.mediaItemCount).map{b.getMediaItemAt(it).mediaId}}
    fun run(b:MediaBrowser){
        val store=ListeningStore.get(test.targetContext);val songs=Catalog.get(test.targetContext).songs;val phone=songs.take(2).map{it.id}
        for((id,name) in listOf("car:qa-a" to "QA Auto A","car:qa-b" to "QA Auto B"))check(send(b,"put_car"){putString("id",id);putString("name",name)}.resultCode==SessionResult.RESULT_SUCCESS)
        check(send(b,"play_ids"){putStringArrayList("ids",ArrayList(phone))}.resultCode==SessionResult.RESULT_SUCCESS)
        await("Phone queue did not start"){main{b.isPlaying}}
        main{b.pause();b.seekTo(1200)}
        check(send(b,"select_car"){putString("id","car:qa-a")}.resultCode==SessionResult.RESULT_SUCCESS)
        await("Auto A did not get a separate empty queue"){main{b.mediaItemCount==0 && !b.playWhenReady}}
        check(store.queues[CarQueues.PHONE]!!.ids==phone && store.queues[CarQueues.PHONE]!!.position>=1100)
        val a=songs.reversed().take(2).map{it.id};store.putPlaylist("QA Car Playlist",a,"qa-car-playlist")
        main{b.setMediaItem(MediaItem.Builder().setMediaId("playlist:qa-car-playlist").build());b.prepare();b.play()}
        await("Saved playlist did not play from the car browser"){main{b.isPlaying && b.playlistMetadata.title.toString()=="QA Car Playlist · QA Auto A"}}
        check(ids(b)==a)
        main{b.pause();b.seekTo(800);b.shuffleModeEnabled=true;b.repeatMode=Player.REPEAT_MODE_ALL}
        check(send(b,"select_car"){putString("id","car:qa-b")}.resultCode==SessionResult.RESULT_SUCCESS)
        check(store.queues["car:qa-a"]!!.name=="QA Car Playlist")
        val connected=java.util.concurrent.CountDownLatch(1)
        val legacy=main{android.media.browse.MediaBrowser(test.targetContext,ComponentName(test.targetContext,PlaybackService::class.java),object:android.media.browse.MediaBrowser.ConnectionCallback(){override fun onConnected(){connected.countDown()}},null)}
        try {
            main{legacy.connect()};check(connected.await(10,TimeUnit.SECONDS))
            val controller=main{android.media.session.MediaController(test.targetContext,legacy.sessionToken)}
            // Same legacy playFromMediaId path used by Android Auto, with no phone Activity interaction.
            main{controller.transportControls.playFromMediaId("mix:qa-dynamic",null)}
            await("Dynamic playlist did not start from the legacy Android Auto controller"){main{b.isPlaying && b.sessionExtras.getString("mix_id")=="qa-dynamic"}}
            main{b.pause()}
            await("Legacy queue title did not identify playlist and car"){main{controller.queueTitle.toString()=="QA Dynamic · QA Auto B" && !controller.queue.isNullOrEmpty()}}
            val root=main{b.getChildren("root",0,100,null)}.get(10,TimeUnit.SECONDS).value!!
            check(root.first{it.mediaId=="now-playing"}.mediaMetadata.subtitle.toString().contains("QA Dynamic"))
            val playlistMenu=main{b.getChildren("playlist-menu",0,100,null)}.get(10,TimeUnit.SECONDS).value!!
            check(playlistMenu.map{it.mediaId}==listOf("now-playing","dynamic","playlists"))
            check(main{b.getChildren("dynamic",0,100,null)}.get(10,TimeUnit.SECONDS).value!!.any{it.mediaId=="mix:qa-dynamic"})
            val limited=main{b.getLibraryRoot(MediaLibraryService.LibraryParams.Builder().setExtras(Bundle().apply{putInt(androidx.media3.session.MediaConstants.EXTRAS_KEY_ROOT_CHILDREN_LIMIT,2)}).build())}.get(10,TimeUnit.SECONDS).value!!
            check(main{b.getChildren(limited.mediaId,0,100,null)}.get(10,TimeUnit.SECONDS).value!!.size==2)
            val queue=main{b.getChildren("now-playing",0,100,null)}.get(10,TimeUnit.SECONDS).value!!;check(queue.isNotEmpty())
            check(queue.any{it.mediaMetadata.displayTitle.toString().startsWith("▶")})
            val item=queue.last();val before=ids(b)
            main{b.setMediaItem(item);b.prepare();b.play()}
            await("Selecting a queue item replaced the dynamic playlist"){main{b.currentMediaItem?.mediaId==before.last() && b.sessionExtras.getString("mix_id")=="qa-dynamic"}}
            main{b.pause()}
            val bIds=ids(b)
            // Profile selection is itself exposed through the car's media browser.
            main{b.setMediaItem(MediaItem.Builder().setMediaId("profile:car:qa-a").build());b.prepare()}
            await("Auto A was not restored through its browse entry"){main{b.sessionExtras.getString("queue_profile")=="car:qa-a" && b.mediaItemCount==a.size}}
            check(ids(b)==a && main{b.currentPosition>=700 && b.shuffleModeEnabled && b.repeatMode==Player.REPEAT_MODE_ALL && !b.playWhenReady})
            check(send(b,"select_car"){putString("id","car:qa-b")}.resultCode==SessionResult.RESULT_SUCCESS)
            check(ids(b)==bIds && main{b.sessionExtras.getString("mix_id")=="qa-dynamic" && !b.playWhenReady})
            check(send(b,"remove_car"){putString("id","car:qa-b")}.resultCode!=SessionResult.RESULT_SUCCESS)
            check(send(b,"select_car"){putString("id",CarQueues.PHONE)}.resultCode==SessionResult.RESULT_SUCCESS)
            check(ids(b)==phone && main{b.currentPosition>=1100 && !b.playWhenReady})
            // Keep the previous cold-restart check's phone position/queue and saved fixtures intact.
            await("Car queues were not persisted"){try{val j=org.json.JSONObject(java.io.File(test.targetContext.filesDir,"listening-v1.json").readText());j.getJSONObject("queues").getJSONObject("car:qa-a").getString("name")=="QA Car Playlist"}catch(_:Exception){false}}
        }finally{main{legacy.disconnect()}}
    }
}
