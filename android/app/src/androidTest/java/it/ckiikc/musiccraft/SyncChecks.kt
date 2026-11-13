package it.ckiikc.musiccraft

import android.content.Context
import android.os.Build
import org.json.JSONObject
import java.io.ByteArrayInputStream
import java.util.concurrent.atomic.AtomicBoolean

/** Real SMB transport and ExternalStorageProvider, restricted to an emulator QA directory. */
class SyncChecks(private val app:Context,private val test:Context) {
    private fun same(a:ByteArray,b:ByteArray)=check(a.contentEquals(b)){"Transferred bytes changed"}
    fun serve(instrumentation:android.app.Instrumentation) {
        check(Build.HARDWARE.contains("ranchu") || Build.MODEL.contains("sdk"));check(FolderSync.folder(app)?.endsWith("MusicCraftSyncQA")==true)
        FolderSync.computer(app,true)
        app.getSharedPreferences("MainActivity",0).edit().putInt("scene",5).commit()
        val link=java.io.File(app.filesDir,"qa-sync-pairing-link");val stop=java.io.File(app.filesDir,"qa-sync-stop");stop.delete()
        val identity=SharingIdentity(app)
        SharingServer(app,identity,java.net.InetAddress.getByName("127.0.0.1")).use{server->
            link.writeText(identity.pairing("127.0.0.1",server.port))
            val activity=instrumentation.startActivitySync(android.content.Intent(app,MainActivity::class.java).addFlags(android.content.Intent.FLAG_ACTIVITY_NEW_TASK))
            try{val end=System.currentTimeMillis()+360000;while(System.currentTimeMillis()<end && !stop.exists())Thread.sleep(100)}finally{instrumentation.runOnMainSync{activity.finish()};link.delete();stop.delete()}
        }
    }
    fun run() {
        check(Build.HARDWARE.contains("ranchu") || Build.MODEL.contains("sdk")){"Emulator only"}
        check(FolderSync.folder(app)?.endsWith("MusicCraftSyncQA")==true){"Grant the dedicated QA folder first"}
        val cancel=AtomicBoolean(false);val bytes=test.assets.open("ratings/test.flac").use{it.readBytes()}
        SmbSyncStorage("127.0.0.1","MUSIC","","fixture","fixture-only".toCharArray(),port=1445).use {a->FolderSync.tree(app).use {b->
            fun create(s:SyncStorage,path:String,value:ByteArray){s.create(path).use{it.write(value)}}
            create(a,"alpha.flac",bytes);create(b,"beta.flac",bytes)
            create(a,"conflict.flac",bytes);create(b,"conflict.flac",bytes+byteArrayOf(1))
            val catalog=Catalog.get(app)
            fun conflictSong():Song {val end=System.currentTimeMillis()+10000;while(System.currentTimeMillis()<end){catalog.refresh(true).get(20,java.util.concurrent.TimeUnit.SECONDS).find{it.displayName=="conflict.flac"}?.let{return it};Thread.sleep(50)};error("Fixture not indexed")}
            val oldSong=conflictSong();val stats=ListeningStore.get(app);stats.rate(oldSong.statKey,5);stats.played(oldSong.statKey)
            val oldStats=stats.stats[oldSong.statKey];val oldQueue=stats.queue;stats.putPlaylist("QA Sync identity",listOf(oldSong.id))
            val first=FolderSync.preview(SyncFiles.snapshot(a,SyncFiles.root(a),cancel),SyncFiles.snapshot(b,SyncFiles.root(b),cancel),"Both",null)
            val ops=first.getJSONArray("operations");check(ops.length()==3)
            val conflict=(0 until ops.length()).map{ops.getJSONObject(it)}.single{it.optString("action")=="Conflict"}
            conflict.put("action",JSONObject().put("Copy","B"))
            val baseline=FolderSync.execute(app,first,a,b){_,_->}
            val newSong=conflictSong();check(newSong.id==oldSong.id && newSong.added==oldSong.added){"Replacement changed a playlist/stat identity"};check(newSong.syncVersion>oldSong.syncVersion){"Remote cache version did not advance"};check(stats.stats[newSong.statKey]==oldStats);check(stats.queue==oldQueue);check(stats.playlists.first{it.name=="QA Sync identity"}.ids==listOf(newSong.id))
            same(a.read("alpha.flac").use{it.readBytes()},b.read("alpha.flac").use{it.readBytes()})
            same(a.read("beta.flac").use{it.readBytes()},b.read("beta.flac").use{it.readBytes()})
            same(a.read("conflict.flac").use{it.readBytes()},b.read("conflict.flac").use{it.readBytes()})
            check(FolderSync.preview(SyncFiles.snapshot(a,SyncFiles.root(a),cancel),SyncFiles.snapshot(b,SyncFiles.root(b),cancel),"Both",baseline).getJSONArray("operations").length()==0)
            a.move("alpha.flac",".musiccraft-sync/simulated-user-delete.flac")
            create(b,"gamma.flac",bytes)
            val next=FolderSync.preview(SyncFiles.snapshot(a,SyncFiles.root(a),cancel),SyncFiles.snapshot(b,SyncFiles.root(b),cancel),"Both",baseline)
            check((0 until next.getJSONArray("operations").length()).any{next.getJSONArray("operations").getJSONObject(it).optJSONObject("action")?.optString("Delete")=="B"})
            val after=FolderSync.execute(app,next,a,b){_,_->}
            check(!SyncFiles.exists(b,"alpha.flac"));same(bytes,b.read("gamma.flac").use{it.readBytes()})
            val recovery=b.children(".musiccraft-sync/recovery");check(recovery.any{SyncFiles.exists(b,".musiccraft-sync/recovery/${it.name}/alpha.flac")})
            create(a,"late.flac",bytes)
            val stale=FolderSync.preview(SyncFiles.snapshot(a,SyncFiles.root(a),cancel),SyncFiles.snapshot(b,SyncFiles.root(b),cancel),"Both",after)
            create(b,"changed-after-preview.flac",bytes)
            check(runCatching{FolderSync.execute(app,stale,a,b){_,_->}}.isFailure);check(!SyncFiles.exists(b,"late.flac"))
            val bad=JSONObject().put("root","bad").put("files",JSONObject().put("../escape.flac",SyncFiles.stamp(ByteArrayInputStream(bytes))))
            check(runCatching{FolderSync.preview(bad,baseline.getJSONObject("b"),"Both",null)}.isFailure)
        }}
    }
}
