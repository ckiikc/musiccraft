package it.ckiikc.musiccraft

import android.content.Context
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.media.MediaMetadataRetriever
import android.net.Uri
import org.json.JSONObject
import java.io.ByteArrayOutputStream
import java.io.File
import java.net.HttpURLConnection
import java.net.URL
import java.net.URLEncoder
import java.util.concurrent.*

/** One bounded worker; no artwork I/O or network on the main thread. */
class ArtworkLibrary internal constructor(private val context:Context,private val directory:File=File(context.filesDir,"artwork"),private val lookup:((Song)->ByteArray?)?=null,private val enabledOverride:(()->Boolean)?=null):AutoCloseable {
    val preferences=context.getSharedPreferences("artwork",Context.MODE_PRIVATE)
    var online:Boolean get()=enabledOverride?.invoke() ?: preferences.getBoolean("online",false);set(value){preferences.edit().putBoolean("online",value).apply()}
    private val worker=ThreadPoolExecutor(1,1,0,TimeUnit.SECONDS,ArrayBlockingQueue(48),ThreadPoolExecutor.AbortPolicy())
    private val jobs=ConcurrentHashMap<String,Future<File?>>()
    private val archive by lazy{ArtworkArchive(directory)}
    private var lastRequest=0L;private var cooldown=preferences.getLong("cooldown",0)
    fun summary(done:(String)->Unit){worker.execute{val value=archive.summary();android.os.Handler(context.mainLooper).post{done(value)}}}
    fun resolve(song:Song):File? {
        check(android.os.Looper.myLooper()!=android.os.Looper.getMainLooper())
        val key=song.coverKey
        val job=synchronized(jobs){jobs[key] ?: FutureTask<File?>{load(song)}.also{jobs[key]=it;try{worker.execute(it)}catch(_:RejectedExecutionException){jobs.remove(key);return null}}}
        return try{job.get(35,TimeUnit.SECONDS)}catch(_:Exception){null}finally{if(job.isDone)jobs.remove(key,job)}
    }
    private fun load(song:Song):File? {
        val now=System.currentTimeMillis();val existing=archive.entry(song.coverKey) ?: archive.migrate(song.previousCoverKey,song.coverKey)
        existing?.file?.let{return it}
        if(existing==null || existing.source=="local-missing" && existing.retry<=now){
            val embedded=try{MediaMetadataRetriever().let{r->try{r.setDataSource(context,song.uri);r.embeddedPicture?.takeIf{it.size<=MAX_BYTES}}finally{r.release()}}}catch(_:Exception){null}
            val local=embedded ?: if(ArtworkKeys.known(song.album))try{song.artwork?.let{context.contentResolver.openInputStream(it)?.use{input->bounded(input,MAX_BYTES)}}}catch(_:Exception){null} else null
            local?.let{normalize(it)}?.let{archive.put(song.coverKey,it)?.let{return it}}
        }
        if(existing==null || existing.source=="local-missing" && existing.retry<=now)archive.put(song.coverKey,null,now+24*60*60_000L,"local-missing")
        if(!online)return null
        val query=ArtworkKeys.query(song.artist,song.album,song.title) ?: run{archive.put(song.coverKey,null,now+WEEK,"unknown");return null}
        val alias="query:"+ArtworkKeys.hash(query)
        archive.entry(alias)?.let{e->if(e.file!=null){archive.link(song.coverKey,e);return e.file};if(e.retry>now)return null}
        if(existing!=null && existing.source!="local-missing" && existing.retry>now)return null
        if(now<cooldown || archive.remaining()<256*1024)return null
        try{
            val cover=if(lookup!=null)lookup.invoke(song) else findOnline(song)
            if(cover!=null){val file=archive.put(alias,cover,source="Cover Art Archive");if(file==null){archive.put(alias,null,now+WEEK,"storage-full");return null};archive.entry(alias)?.let{archive.link(song.coverKey,it)};return file}
            archive.put(alias,null,now+WEEK,"not-found");archive.put(song.coverKey,null,now+WEEK,"not-found")
        }catch(_:Exception){archive.put(alias,null,maxOf(now+15*60_000,cooldown),"retry")}
        return null
    }
    private fun findOnline(song:Song):ByteArray? {
        if(!online)return null
        val album=ArtworkKeys.known(song.album)
        fun quote(s:String)="\""+s.replace("\\","\\\\").replace("\"","\\\"")+"\""
        val q=if(album)"releasegroup:${quote(song.album)} AND artist:${quote(song.artist)}" else "recording:${quote(song.title)} AND artist:${quote(song.artist)}"
        val endpoint=if(album)"release-group" else "recording"
        val json=JSONObject(String(fetch("https://musicbrainz.org/ws/2/$endpoint/?query=${URLEncoder.encode(q,"UTF-8")}&fmt=json&limit=5",1_000_000) ?: return null))
        val entries=json.optJSONArray(if(album)"release-groups" else "recordings") ?: return null
        // Accept only exact metadata and one recording/album identity; never guess between namesakes.
        val matches=(0 until entries.length()).map{entries.getJSONObject(it)}.filter{entry->
            val credits=entry.optJSONArray("artist-credit")
            val artist=credits?.let{a->(0 until a.length()).joinToString(""){i->val c=a.getJSONObject(i);c.optString("name",c.optJSONObject("artist")?.optString("name").orEmpty())+c.optString("joinphrase")}}
            ArtworkKeys.matches(if(album)song.album else song.title,entry.optString("title")) && artist!=null && ArtworkKeys.matches(song.artist,artist)
        }
        if(matches.size!=1)return null
        val entry=matches.single()
        val id=if(album)entry.optString("id") else entry.optJSONArray("releases")?.takeIf{it.length()==1}?.getJSONObject(0)?.optString("id") ?: return null
        if(!Regex("[0-9a-fA-F-]{36}").matches(id))return null
        return fetch("https://coverartarchive.org/${if(album)"release-group" else "release"}/$id/front-500",MAX_BYTES)?.let{normalize(it)}
    }
    private fun fetch(address:String,limit:Int):ByteArray? {
        check(online);val wait=1100-(android.os.SystemClock.elapsedRealtime()-lastRequest);if(wait>0)Thread.sleep(wait);check(online)
        if(System.currentTimeMillis()<cooldown)throw java.io.IOException("Backoff")
        var url=URL(address)
        repeat(6){
            require(url.protocol=="https" && (url.host in setOf("musicbrainz.org","coverartarchive.org","archive.org") || url.host.endsWith(".archive.org")))
            val connection=url.openConnection() as HttpURLConnection
            try{
                connection.instanceFollowRedirects=false;connection.connectTimeout=7000;connection.readTimeout=7000
                connection.setRequestProperty("User-Agent","MusicCraft/${BuildConfig.VERSION_NAME} (https://github.com/ckiikc/musiccraft)")
                lastRequest=android.os.SystemClock.elapsedRealtime()
                val code=connection.responseCode
                if(code==429 || code==503){cooldown=System.currentTimeMillis()+(connection.getHeaderField("Retry-After")?.toLongOrNull()?.coerceIn(60,86400) ?: 900)*1000;preferences.edit().putLong("cooldown",cooldown).apply();throw java.io.IOException("Rate limited")}
                if(code in 300..399){url=URL(url,connection.getHeaderField("Location") ?: throw java.io.IOException("Redirect"))}
                else if(code==404)return null
                else {require(code==200);require(connection.contentLengthLong<=limit);return connection.inputStream.use{bounded(it,limit)}}
            }finally{connection.disconnect()}
        }
        throw java.io.IOException("Too many redirects")
    }
    override fun close(){worker.shutdown();worker.awaitTermination(40,TimeUnit.SECONDS);archive.close()}
    companion object {
        const val AUTHORITY="it.ckiikc.musiccraft.artwork";private const val MAX_BYTES=8*1024*1024;private const val WEEK=7*24*60*60_000L
        @Volatile private var instance:ArtworkLibrary?=null
        fun get(context:Context)=instance ?: synchronized(this){instance ?: ArtworkLibrary(context.applicationContext).also{instance=it}}
        fun uri(song:Song)=Uri.parse("content://$AUTHORITY/cover/${song.coverKey}")
        fun bounded(input:java.io.InputStream,limit:Int):ByteArray {val out=ByteArrayOutputStream();val buffer=ByteArray(8192);while(true){val n=input.read(buffer);if(n<0)break;require(out.size()+n<=limit);out.write(buffer,0,n)};return out.toByteArray()}
        fun normalize(bytes:ByteArray):ByteArray? {val bounds=BitmapFactory.Options().apply{inJustDecodeBounds=true};BitmapFactory.decodeByteArray(bytes,0,bytes.size,bounds);if(bounds.outWidth<=0 || bounds.outHeight<=0)return null
            var sample=1;while(maxOf(bounds.outWidth,bounds.outHeight)/sample>1024)sample*=2
            val bitmap=BitmapFactory.decodeByteArray(bytes,0,bytes.size,BitmapFactory.Options().apply{inSampleSize=sample}) ?: return null
            return try{ByteArrayOutputStream().use{out->bitmap.compress(Bitmap.CompressFormat.JPEG,90,out);out.toByteArray()}}finally{bitmap.recycle()}}
    }
}
