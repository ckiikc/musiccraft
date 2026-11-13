package it.ckiikc.musiccraft

import android.app.Activity
import android.app.Instrumentation
import android.content.pm.ActivityInfo
import android.os.Bundle
import android.widget.*
import androidx.media3.common.*
import androidx.media3.session.*
import java.util.Collections
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicReference
import org.json.JSONObject

@androidx.annotation.OptIn(androidx.media3.common.util.UnstableApi::class)
class DynamicChecks(private val test:Instrumentation) {
    private fun <T> main(block:()->T):T {val value=AtomicReference<T>();val error=AtomicReference<Throwable>();test.runOnMainSync{try{value.set(block())}catch(e:Throwable){error.set(e)}};error.get()?.let{throw it};return value.get()}
    private fun await(message:String,timeout:Long=15000,check:()->Boolean){val end=System.currentTimeMillis()+timeout;while(System.currentTimeMillis()<end){if(check())return;Thread.sleep(80)};error(message)}
    private fun send(b:MediaBrowser,op:String,extra:Bundle.()->Unit={}):SessionResult=main{b.sendCustomCommand(SessionCommand(PlaybackCommands.ACTION,Bundle.EMPTY),Bundle().apply{putString("op",op);extra()})}.get(15,TimeUnit.SECONDS)
    private fun ids(b:MediaBrowser)=main{(0 until b.mediaItemCount).map{b.getMediaItemAt(it).mediaId}}
    fun run(b:MediaBrowser,initial:Activity):Activity {
        var activity=initial
        val catalog=Catalog.get(test.targetContext);val songs=catalog.songs
        val queue=listOf("QA Gamma","QA Alpha","QA Beta").map{name->songs.single{it.title==name}.id}
        check(send(b,"play_ids"){putStringArrayList("ids",ArrayList(queue));putString("mode","replace")}.resultCode==SessionResult.RESULT_SUCCESS)
        await("Manual queue did not play"){main{b.isPlaying}}
        main{activity.findViewById<android.view.View>(R.id.nav_search).performClick();activity.findViewById<EditText>(R.id.library_search).setText("QA Beta")}
        await("Search did not filter"){main{activity.findViewById<ListView>(R.id.library_list).adapter.count==1}}
        check(ids(b)==queue){"Searching replaced the listening queue"}
        main{activity.findViewById<android.view.View>(R.id.nav_playlists).performClick()};check(ids(b)==queue)
        main{activity.findViewById<android.view.View>(R.id.nav_player).performClick()}
        check(main{activity.findViewById<LinearLayout>(R.id.player_stage).orientation==LinearLayout.VERTICAL})
        fun rotate(orientation:Int):Activity {
            val monitor=test.addMonitor(MainActivity::class.java.name,null,false)
            main{activity.requestedOrientation=orientation}
            val recreated=test.waitForMonitorWithTimeout(monitor,10000)
            test.removeMonitor(monitor)
            return recreated ?: error("Player Activity did not recreate after orientation change")
        }
        activity=rotate(ActivityInfo.SCREEN_ORIENTATION_LANDSCAPE)
        check(main{activity.findViewById<LinearLayout>(R.id.player_stage).orientation==LinearLayout.HORIZONTAL})
        check(ids(b)==queue && main{b.isPlaying}){"Landscape rotation interrupted or replaced playback"}
        activity=rotate(ActivityInfo.SCREEN_ORIENTATION_PORTRAIT)
        check(main{activity.findViewById<LinearLayout>(R.id.player_stage).orientation==LinearLayout.VERTICAL})
        check(ids(b)==queue)
        main{b.pause()};val song=main{b.currentMediaItem!!.mediaId};val position=main{b.currentPosition}
        check(send(b,"move"){putInt("from",2);putInt("to",0)}.resultCode==SessionResult.RESULT_SUCCESS)
        check(main{b.currentMediaItem!!.mediaId==song && kotlin.math.abs(b.currentPosition-position)<200}){"Moving queue items lost the current track/position"}
        val before=ids(b)
        check(send(b,"play_ids"){putStringArrayList("ids",arrayListOf("song:missing"))}.resultCode!=SessionResult.RESULT_SUCCESS)
        check(ids(b)==before){"Invalid replacement destroyed the existing queue"}
        check(send(b,"save_queue"){putString("name","QA Saved")}.resultCode==SessionResult.RESULT_SUCCESS)
        val store=ListeningStore.get(test.targetContext);check(store.playlists.single{it.name=="QA Saved"}.ids==before)
        check(send(b,"rate"){putString("id",songs.first().id);putInt("rating",5)}.resultCode==SessionResult.RESULT_SUCCESS)
        check(store.stats[songs.first().statKey]?.rating==5)
        val rule=SmartRule("qa-dynamic","QA Dynamic",query="QA")
        store.put(rule)
        val transitions=Collections.synchronizedList(mutableListOf<String>())
        val observer=object:Player.Listener{override fun onMediaItemTransition(item:MediaItem?,reason:Int){if(reason==Player.MEDIA_ITEM_TRANSITION_REASON_AUTO)transitions.add(item!!.mediaId)}}
        main{b.addListener(observer)}
        check(send(b,"start_mix"){putString("id",rule.id)}.resultCode==SessionResult.RESULT_SUCCESS)
        await("Dynamic mix did not become active"){main{b.sessionExtras.getString("mix_id")==rule.id && b.isPlaying}}
        await("Dynamic mix did not replenish its queue",30000){transitions.size>=4}
        val played=transitions.toList();check(played.zipWithNext().none{it.first==it.second}){"Dynamic mix repeated adjacent songs"}
        check(main{b.mediaItemCount<=30}){"Dynamic queue grew without bound"}
        main{b.pause()};val countBefore=store.stats.values.sumOf{it.plays};Thread.sleep(1600);check(store.stats.values.sumOf{it.plays}==countBefore){"Pause was counted as listening"}
        val retained=ids(b);check(send(b,"stop_mix").resultCode==SessionResult.RESULT_SUCCESS);check(ids(b)==retained){"Stop adding cleared the queue"}
        check(main{b.sessionExtras.getString("mix_id").isNullOrEmpty()})
        main{b.seekTo(b.mediaItemCount-1,1000000);b.play()}
        await("Stopped mix kept adding music instead of finishing",8000){main{b.playbackState==Player.STATE_ENDED}}
        main{b.removeListener(observer)}
        check(store.stats.values.any{it.plays>0}){"Actual listening was not recorded"}
        check(send(b,"play_ids"){putStringArrayList("ids",ArrayList(queue.take(2)))}.resultCode==SessionResult.RESULT_SUCCESS)
        await("Final persistence queue did not play"){main{b.isPlaying}}
        main{b.pause();b.seekTo(1200)}
        await("Queue/position did not reach atomic storage"){
            try{val json=JSONObject(java.io.File(test.targetContext.filesDir,"listening-v1.json").readText());json.getJSONObject("queue").getLong("position")>=1100 && json.getJSONObject("queue").getJSONArray("ids").length()==2}catch(_:Exception){false}
        }
        return activity
    }
}
