package it.ckiikc.musiccraft
import android.content.ContentValues
import android.content.Context
import android.provider.MediaStore
import android.net.Uri
import org.json.JSONObject
import java.security.MessageDigest

class RatingChecks(private val app:Context,private val test:Context){
    private fun data(uri:Uri)=app.contentResolver.openInputStream(uri)!!.use{it.readBytes()}
    private fun hash(bytes:ByteArray)=MessageDigest.getInstance("SHA-256").digest(bytes).toList()
    private fun audioOffset(b:ByteArray):Int {
        if(String(b,0,3)=="ID3")return 10+(6..9).fold(0){n,i->n*128+(b[i].toInt() and 127)}
        check(String(b,0,4)=="fLaC");var p=4
        while(true){val last=b[p].toInt() and 128!=0;val n=((b[p+1].toInt() and 255) shl 16)+((b[p+2].toInt() and 255) shl 8)+(b[p+3].toInt() and 255);p+=4+n;if(last)return p}
    }
    fun serve(instrumentation:android.app.Instrumentation){
        check(android.os.Build.HARDWARE.contains("ranchu") || android.os.Build.MODEL.contains("sdk"))
        val uri=app.contentResolver.insert(MediaStore.Audio.Media.EXTERNAL_CONTENT_URI,ContentValues().apply{put(MediaStore.Audio.Media.DISPLAY_NAME,"musiccraft-rating-live.flac");put(MediaStore.Audio.Media.MIME_TYPE,"audio/flac");put(MediaStore.Audio.Media.RELATIVE_PATH,"Music/MusicCraftRatingQA");put(MediaStore.Audio.Media.IS_PENDING,1)})!!
        val link=java.io.File(app.filesDir,"qa-rating-pairing-link")
        try{
            app.contentResolver.openOutputStream(uri,"w")!!.use{out->test.assets.open("ratings/test.flac").use{it.copyTo(out)}}
            app.contentResolver.update(uri,ContentValues().apply{put(MediaStore.Audio.Media.IS_PENDING,0)},null,null)
            Catalog.get(app).refresh(true).get(20,java.util.concurrent.TimeUnit.SECONDS)
            val identity=SharingIdentity(app)
            SharingServer(app,identity,java.net.InetAddress.getByName("127.0.0.1")).use{server->
                link.writeText(identity.pairing("127.0.0.1",server.port))
                val activity=instrumentation.startActivitySync(android.content.Intent(app,MainActivity::class.java).addFlags(android.content.Intent.FLAG_ACTIVITY_NEW_TASK))
                try{val end=System.currentTimeMillis()+180000;while(System.currentTimeMillis()<end && !java.io.File(app.filesDir,"qa-rating-stop").exists())Thread.sleep(100)}finally{instrumentation.runOnMainSync{activity.finish()}}
            }
        }finally{link.delete();java.io.File(app.filesDir,"qa-rating-stop").delete();app.contentResolver.delete(uri,null,null)}
    }
    fun run(){
        check(android.os.Build.HARDWARE.contains("ranchu") || android.os.Build.MODEL.contains("sdk"))
        val rule=SmartRule("rating-check","Votes and counts",minimumRating=4,minimumPlays=1,maximumPlays=2)
        val encoded=ListeningStore.encodeRule(rule);check(ListeningStore.decodeRule(encoded)==rule)
        encoded.remove("minPlays");encoded.remove("maxPlays");check(ListeningStore.decodeRule(encoded)?.minimumPlays==0L && ListeningStore.decodeRule(encoded)?.maximumPlays==null)
        val uris=mutableListOf<Uri>()
        try{for(ext in listOf("flac","mp3")){
            val uri=app.contentResolver.insert(MediaStore.Audio.Media.EXTERNAL_CONTENT_URI,ContentValues().apply{put(MediaStore.Audio.Media.DISPLAY_NAME,"musiccraft-rating-fixture.$ext");put(MediaStore.Audio.Media.MIME_TYPE,if(ext=="flac")"audio/flac" else "audio/mpeg");put(MediaStore.Audio.Media.RELATIVE_PATH,"Music/MusicCraftRatingQA");put(MediaStore.Audio.Media.IS_PENDING,1)})!!;uris+=uri
            app.contentResolver.openOutputStream(uri,"w")!!.use{out->test.assets.open("ratings/test.$ext").use{it.copyTo(out)}}
            app.contentResolver.update(uri,ContentValues().apply{put(MediaStore.Audio.Media.IS_PENDING,0)},null,null)
            val before=data(uri);val originalHash=hash(before);val summary=RatingTags.inspect(app,uri)
            check(RatingTags.resolve(summary,5)==4){"$ext $summary"};check(RatingTags.resolve(summary,0)==null);check(RatingTags.resolve(summary,100)==null)
            check(hash(data(uri))==originalHash){"Import modified original bytes"}
            val songs=Catalog.get(app).refresh(true).get(20,java.util.concurrent.TimeUnit.SECONDS);val song=songs.first{it.uri==uri}
            SharingChecks(app).use{it.ratingFailures(song.id.substringAfter(':'))}
            val key=java.util.UUID.randomUUID().toString().replace("-","")
            RatingTags.enqueue(app,song.id.substringAfter(':'),2,5,key);RatingTags.apply(app,key)
            val end=System.currentTimeMillis()+10000;while(RatingTags.status(app,key)!!.getString("state")=="waiting" && System.currentTimeMillis()<end)Thread.sleep(50)
            check(RatingTags.status(app,key)!!.getString("state")=="done"){RatingTags.status(app,key).toString()}
            val after=data(uri);check(after.size==before.size);val offset=audioOffset(before)
            check(after.copyOfRange(offset,after.size).contentEquals(before.copyOfRange(offset,before.size))){"Audio payload changed"}
            check(RatingTags.resolve(RatingTags.inspect(app,uri),5)==2)
            check(after.toList().windowed("Preserve this custom metadata".length).any{it=="Preserve this custom metadata".toByteArray().toList()})
            val store=ListeningStore.get(app);store.rate(song.statKey,0);store.importRating(song.statKey,4);check(store.stats[song.statKey]!!.rating==0)
            // Crash recovery: durable original prefix + a deliberately partial old/new write.
            val old=before.copyOfRange(0,offset);val next=after.copyOfRange(0,offset)
            val journal=android.util.AtomicFile(java.io.File(app.filesDir,"rating-tag-journal.json"));val out=journal.startWrite()
            out.write(JSONObject().put("request",key).put("uri",uri.toString()).put("before",android.util.Base64.encodeToString(old,2)).put("after",android.util.Base64.encodeToString(next,2)).toString().toByteArray());journal.finishWrite(out)
            val partial=next.clone();val changes=old.indices.filter{old[it]!=next[it]};check(changes.size>=2);partial[changes.first()]=old[changes.first()]
            val fd=app.contentResolver.openFileDescriptor(uri,"rw")!!;android.os.ParcelFileDescriptor.AutoCloseOutputStream(fd).use{it.write(partial);it.fd.sync()}
            check(RatingTags.recoverNow(app));check(data(uri).contentEquals(before)){"Recovery did not restore exact original"}
        }
        val conflicting=JSONObject().put("issue",JSONObject.NULL).put("values",org.json.JSONArray(listOf("4","2")));check(RatingTags.resolve(conflicting,5)==null)
        }finally{uris.forEach{app.contentResolver.delete(it,null,null)};Catalog.get(app).refresh(true).get(20,java.util.concurrent.TimeUnit.SECONDS)}
    }
}
