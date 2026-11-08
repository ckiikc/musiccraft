package it.ckiikc.musiccraft

import android.app.Activity
import android.app.Instrumentation
import android.content.ComponentName
import android.content.ContentValues
import android.content.Intent
import android.os.Build
import android.os.Bundle
import android.provider.MediaStore
import androidx.media3.common.*
import androidx.media3.session.*
import java.util.Collections
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicReference

@androidx.annotation.OptIn(androidx.media3.common.util.UnstableApi::class)
class MusicCraftInstrumentation : Instrumentation() {
    private var keepFixtures = false
    private var shareServer = false
    private var restoreOnly=false
    override fun onCreate(arguments: Bundle?) { super.onCreate(arguments);keepFixtures=arguments?.getString("keepFixtures")=="true";shareServer=arguments?.getString("sharingServer")=="true";restoreOnly=arguments?.getString("restoreOnly")=="true";start() }
    private fun <T> main(block:()->T):T {
        val value=AtomicReference<T>();val error=AtomicReference<Throwable>()
        runOnMainSync {try {value.set(block())}catch(e:Throwable){error.set(e)}}
        error.get()?.let {throw it};return value.get()
    }
    private fun await(message:String, timeout:Long=15000, condition:()->Boolean) {
        val end=System.currentTimeMillis()+timeout
        while(System.currentTimeMillis()<end) {if(condition()) return;Thread.sleep(100)}
        error(message)
    }
    private fun verifyRestore(){
        val report=Bundle();var browser:MediaBrowser?=null;var activity:Activity?=null;var code=Activity.RESULT_CANCELED
        try {
            check(Build.HARDWARE.contains("ranchu") || Build.MODEL.contains("sdk"))
            Catalog.get(targetContext).refresh(true).get(20,TimeUnit.SECONDS)
            activity=startActivitySync(Intent(targetContext,MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
            browser=main{MediaBrowser.Builder(targetContext,SessionToken(targetContext,ComponentName(targetContext,PlaybackService::class.java))).buildAsync()}.get(20,TimeUnit.SECONDS)
            val b=browser
            await("Cold restart did not restore the saved queue"){main{b.mediaItemCount==2 && b.currentPosition>=1100}}
            check(main{!b.isPlaying && !b.playWhenReady}){"Restored queue played without a user action"}
            val saved=ListeningStore.get(targetContext)
            check(saved.playlists.any{it.name=="QA Saved"} && saved.rules.any{it.id=="qa-dynamic"} && saved.stats.values.any{it.rating==5})
            check(main{activity?.findViewById<android.widget.LinearLayout>(R.id.player_stage)!=null})
            code=Activity.RESULT_OK;report.putString("stream","PASS: queue/position, saved playlists, dynamic rules and ratings restored after process death; no automatic audio start\n")
        }catch(e:Throwable){report.putString("stream","FAIL: ${e.stackTraceToString()}\n")}
        finally{main{browser?.release();activity?.finish()}}
        finish(code,report)
    }
    override fun onStart() {
        if(restoreOnly){verifyRestore();return}
        val report=Bundle();val inserted=mutableListOf<android.net.Uri>();var browser:MediaBrowser?=null;var activity:Activity?=null;var sharing:SharingChecks?=null;var resultCode=Activity.RESULT_CANCELED
        try {
            check(Build.HARDWARE.contains("ranchu") || Build.MODEL.contains("sdk")) {"These synthetic tests are emulator-only"}
            targetContext.getSharedPreferences("MainActivity",0).edit().clear().commit()
            targetContext.deleteFile("listening-v1.json")
            targetContext.contentResolver.delete(MediaStore.Audio.Media.EXTERNAL_CONTENT_URI,"relative_path = ? AND owner_package_name = ?",arrayOf("Music/MusicCraft QA/",targetContext.packageName))
            for(title in listOf("QA Gamma","QA Alpha","QA Beta")) {
                val values=ContentValues().apply {put(MediaStore.Audio.Media.TITLE,title);put(MediaStore.Audio.Media.DISPLAY_NAME,"$title.flac");put(MediaStore.Audio.Media.ARTIST,"MusicCraft QA");put(MediaStore.Audio.Media.ALBUM,"QA Session");put(MediaStore.Audio.Media.IS_MUSIC,1);put(MediaStore.Audio.Media.MIME_TYPE,"audio/flac");put(MediaStore.Audio.Media.RELATIVE_PATH,"Music/MusicCraft QA");put(MediaStore.Audio.Media.IS_PENDING,1)}
                val uri=targetContext.contentResolver.insert(MediaStore.Audio.Media.EXTERNAL_CONTENT_URI,values)!!;inserted+=uri
                targetContext.contentResolver.openOutputStream(uri)!!.use {output-> context.assets.open("$title.flac").use {it.copyTo(output)}}
                targetContext.contentResolver.update(uri,ContentValues().apply {put(MediaStore.Audio.Media.IS_PENDING,0)},null,null)
            }
            await("MediaStore did not index fixture tags",15000) {
                Catalog.get(targetContext).refresh(true).get(20,TimeUnit.SECONDS).count { it.artist=="MusicCraft QA" }==3
            }
            if(shareServer) {
                val identity=SharingIdentity(targetContext)
                SharingServer(targetContext,identity,java.net.InetAddress.getByName("127.0.0.1")).use {server ->
                    val file=java.io.File(targetContext.filesDir,"qa-pairing-link")
                    val pinFile=java.io.File(targetContext.filesDir,"qa-pairing-code")
                    try {
                        file.writeText(identity.pairing("127.0.0.1",server.port));pinFile.writeText(PairingWindow.open())
                        val end=System.currentTimeMillis()+180000
                        while(System.currentTimeMillis()<end){if(PairingWindow.pending()) PairingWindow.approve();Thread.sleep(100)}
                    } finally {file.delete();pinFile.delete();PairingWindow.close()}
                }
                report.putString("stream","PASS: synthetic sharing server finished\n")
            } else {
            activity=startActivitySync(Intent(targetContext,MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
            browser=main {MediaBrowser.Builder(targetContext,SessionToken(targetContext,ComponentName(targetContext,PlaybackService::class.java))).buildAsync()}.get(20,TimeUnit.SECONDS)
            await("Phone UI did not show the indexed tracks") {main{activity?.findViewById<android.widget.ListView>(R.id.library_list)?.adapter?.count==3}}
            val b=browser
            val root=main {b.getLibraryRoot(null)}.get(10,TimeUnit.SECONDS);check(root.resultCode==LibraryResult.RESULT_SUCCESS && root.value?.mediaId=="root"){"Root failed: code=${root.resultCode}, id=${root.value?.mediaId}"}
            val folders=main {b.getChildren("root",0,100,null)}.get(10,TimeUnit.SECONDS).value!!;check(folders.map{it.mediaId}==listOf("songs","albums","artists","dynamic"))
            val legacyConnected=java.util.concurrent.CountDownLatch(1)
            val legacyChildren=java.util.concurrent.CountDownLatch(1)
            val legacyIds=AtomicReference<List<String>>()
            val legacy=main { android.media.browse.MediaBrowser(targetContext,ComponentName(targetContext,PlaybackService::class.java),object:android.media.browse.MediaBrowser.ConnectionCallback(){override fun onConnected(){legacyConnected.countDown()}},null) }
            main{legacy.connect()};check(legacyConnected.await(10,TimeUnit.SECONDS)){"Legacy Android Auto browser did not connect"}
            check(main{legacy.root}=="root")
            main{legacy.subscribe("root",object:android.media.browse.MediaBrowser.SubscriptionCallback(){override fun onChildrenLoaded(parentId:String,children:MutableList<android.media.browse.MediaBrowser.MediaItem>){legacyIds.set(children.map{it.mediaId.orEmpty()});legacyChildren.countDown()}})}
            check(legacyChildren.await(10,TimeUnit.SECONDS)){"Legacy Android Auto browse did not return children"}
            check(legacyIds.get()==listOf("songs","albums","artists","dynamic"))
            main{legacy.disconnect()}
            val songs=main {b.getChildren("songs",0,100,null)}.get(10,TimeUnit.SECONDS).value!!.filter {it.mediaMetadata.artist=="MusicCraft QA"};check(songs.size==3) { "Expected three tagged fixtures, got ${songs.map { it.mediaMetadata.title to it.mediaMetadata.artist }}" }
            check(main {b.getChildren("songs",1,2,null)}.get(10,TimeUnit.SECONDS).value!!.size==1)
            check(main {b.getChildren("albums",0,100,null)}.get(10,TimeUnit.SECONDS).value!!.isNotEmpty())
            check(main {b.getChildren("artists",0,100,null)}.get(10,TimeUnit.SECONDS).value!!.isNotEmpty())
            main {b.search("QA Beta",null)}.get(10,TimeUnit.SECONDS)
            check(main {b.getSearchResult("QA Beta",0,100,null)}.get(10,TimeUnit.SECONDS).value!!.single().mediaMetadata.title=="QA Beta")
            sharing=SharingChecks(targetContext);sharing.idle()
            val queue=listOf("QA Gamma","QA Alpha","QA Beta").map {name->songs.single {it.mediaMetadata.title==name}}
            val transitions=Collections.synchronizedList(mutableListOf<String>())
            main {b.addListener(object:Player.Listener {override fun onMediaItemTransition(mediaItem:MediaItem?,reason:Int){mediaItem?.let{val name=it.mediaMetadata.title.toString(); if(transitions.lastOrNull()!=name || reason==Player.MEDIA_ITEM_TRANSITION_REASON_AUTO || reason==Player.MEDIA_ITEM_TRANSITION_REASON_REPEAT) transitions.add(name)}}});b.setMediaItems(queue);b.prepare();b.play()}
            await("Playback did not start") {main {b.isPlaying}}
            sharing.suspended()
            main {b.seekToNextMediaItem()};await("Next failed"){main{b.currentMediaItemIndex==1}}
            main {b.seekToPreviousMediaItem()};await("Previous failed"){main{b.currentMediaItemIndex==0}}
            main {b.seekTo(1200)};await("Seek failed"){main{b.currentPosition>=1100}}
            main {b.pause()};val paused=main {b.currentPosition};Thread.sleep(450);check(kotlin.math.abs(main{b.currentPosition}-paused)<100)
            transitions.clear()
            main {b.stop();b.clearMediaItems();b.setMediaItems(queue);b.prepare();b.play()}
            await("Second playback did not start"){main{b.isPlaying}}
            Thread.sleep(500);main {activity?.finish()};activity=null
            val before=main{b.currentMediaItemIndex to b.currentPosition};Thread.sleep(600);check(main{b.isPlaying && (b.currentMediaItemIndex>before.first || b.currentMediaItemIndex==before.first && b.currentPosition>before.second)}){"Background state: ${main{listOf(b.playbackState,b.isPlaying,b.playWhenReady,b.currentMediaItemIndex,b.currentPosition,b.mediaItemCount,b.playerError?.message)}} transitions=$transitions"}
            try {await("Automatic queue did not finish",20000){main{b.playbackState==Player.STATE_ENDED}}} catch(e:Exception) {error("${e.message}: ${main{listOf(b.playbackState,b.isPlaying,b.playWhenReady,b.currentMediaItemIndex,b.currentPosition,b.mediaItemCount,b.repeatMode,b.playerError?.message)}} transitions=$transitions") }
            check(transitions.toList()==listOf("QA Gamma","QA Alpha","QA Beta")){"Wrong automatic order: $transitions"}
            main {b.stop()};Thread.sleep(300);sharing.resumed()
            activity=startActivitySync(Intent(targetContext,MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
            activity=DynamicChecks(this).run(b,activity!!)
            report.putString("stream","PASS: independent search/queue; standalone portrait/landscape player; rotation while playing; queue editing; ratings; persistent playlists/position; dynamic replenishment/no adjacent repeats/stop adding; HTTPS sharing authentication/identity/GET/HEAD/ranges/read-only/suspension/resumption;  Media3 and legacy Android Auto browser; media browser root/categories/pagination/search; previous/next/seek/pause; playback after Activity finish; automatic three-track sequence and stop\n")
            }
            resultCode=Activity.RESULT_OK
        } catch(e:Throwable) {
            report.putString("stream","FAIL: ${e.stackTraceToString()}\n")
        } finally {
            sharing?.close()
            try {main {browser?.pause();browser?.release();activity?.finish()}}catch(_:Throwable){}
            for(uri in if(keepFixtures) emptyList() else inserted) try {targetContext.contentResolver.delete(uri,null,null)}catch(_:Exception){}
        }
        finish(resultCode,report)
    }
}
