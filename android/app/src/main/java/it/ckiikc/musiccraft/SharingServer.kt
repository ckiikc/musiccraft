package it.ckiikc.musiccraft

import android.content.Context
import android.util.Base64
import org.json.JSONArray
import org.json.JSONObject
import java.io.*
import java.net.*
import java.security.MessageDigest
import java.util.Collections
import java.util.concurrent.ArrayBlockingQueue
import java.util.concurrent.ThreadPoolExecutor
import java.util.concurrent.TimeUnit
import javax.net.ssl.SSLServerSocket
import javax.net.ssl.SSLSocket

/** TLS, bounded concurrency, private-interface binding; only authenticated GET/HEAD. */
class SharingServer(private val context:Context, private val identity:SharingIdentity, host:InetAddress):Closeable {
    private val workers=ThreadPoolExecutor(4,4,10,TimeUnit.SECONDS,ArrayBlockingQueue(8))
    private val clients=Collections.synchronizedSet(mutableSetOf<Socket>())
    private val lifecycle=Any()
    private val cleanup=AsyncClose()
    private val listeners=mutableListOf<SSLServerSocket>()
    private val catalog=Catalog.get(context)
    @Volatile private var closed=false
    private val hostAddress=host.hostAddress!!
    val port:Int
    init {
        check(RatingTags.recoverNow(context)){"Rating metadata recovery required"}
        fun bind(address:InetAddress,port:Int):SSLServerSocket=(identity.tls.serverSocketFactory.createServerSocket() as SSLServerSocket).apply {
            useClientMode=false;needClientAuth=false;reuseAddress=true;bind(InetSocketAddress(address,port),8);enabledProtocols=supportedProtocols.filter{it=="TLSv1.2" || it=="TLSv1.3"}.toTypedArray()
        }
        // Prefer a stable port, but handle collisions without claiming another app's port.
        val primary=try {bind(host,8737)} catch(_:BindException) {bind(host,0)}
        listeners+=primary;port=primary.localPort
        // Authenticated loopback permits private USB forwarding and repeatable testing.
        try {listeners+=bind(InetAddress.getByName("127.0.0.1"),port)} catch(_:IOException){}
        for(listener in listeners) Thread({
            while(!closed) {
                try {
                    val socket=listener.accept() as SSLSocket;socket.useClientMode=false;socket.soTimeout=6000
                    val accepted=synchronized(lifecycle){if(closed)false else{clients.add(socket);true}}
                    if(!accepted){socket.close();continue}
                    try {workers.execute {socket.use {try{serve(it)}catch(e:Exception){if(BuildConfig.DEBUG) android.util.Log.w("MusicCraftSharing","Transfer failed",e)}finally{clients.remove(it)}}}}
                    catch(_:java.util.concurrent.RejectedExecutionException){clients.remove(socket);socket.close()}
                } catch(_:IOException){if(closed) break}
            }
        },"MusicCraft sharing listener").apply {isDaemon=true;start()}
    }
    private fun detachClients():List<Socket> = synchronized(clients){clients.toList().also{clients.clear()}}
    fun suspendTransfers() {synchronized(lifecycle){if(!closed)cleanup.release(detachClients())}}
    override fun close() {synchronized(lifecycle){
        if(closed)return
        closed=true;PairingWindow.close()
        cleanup.release(listeners+detachClients());cleanup.close();workers.shutdownNow()
    }}
    private fun line(input:InputStream,limit:Int):String {
        val out=ByteArrayOutputStream()
        while(out.size()<limit) {
            val c=input.read();if(c<0) throw EOFException()
            if(c==10) return out.toString("US-ASCII").removeSuffix("\r")
            if(c==0) throw IOException("Invalid header")
            out.write(c)
        }
        throw IOException("Header too long")
    }
    private fun serve(socket:SSLSocket) {
        val input=BufferedInputStream(socket.inputStream);val output=BufferedOutputStream(socket.outputStream)
        val request=line(input,4096).split(' ');if(request.size!=3) return
        val headers=mutableMapOf<String,String>();var total=0
        while(true) {val row=line(input,4096);total+=row.length;if(total>16384) return;if(row.isEmpty()) break;val i=row.indexOf(':');if(i<=0) return;val k=row.substring(0,i).lowercase(java.util.Locale.ROOT);if(headers.put(k,row.substring(i+1).trim())!=null) return}
        val method=request[0];val path=request[1]
        fun response(code:Int,data:ByteArray=ByteArray(0),type:String="application/json",extra:String="") {
            val reason=when(code){200->"OK";202->"Accepted";206->"Partial Content";401->"Unauthorized";404->"Not Found";405->"Method Not Allowed";416->"Range Not Satisfiable";else->"Service Unavailable"}
            output.write("HTTP/1.1 $code $reason\r\nContent-Length: ${data.size}\r\nContent-Type: $type\r\nCache-Control: no-store\r\nConnection: close\r\n$extra\r\n".toByteArray(Charsets.US_ASCII));if(method!="HEAD") output.write(data);output.flush()
        }
        // Pairing is a user-opened PAKE exchange, not an unauthenticated media endpoint.
        if(path=="/v1/pair/start" || path=="/v1/pair/finish") {
            if(method!="POST") {response(405,extra="Allow: POST\r\n");return}
            if(headers.containsKey("transfer-encoding")) {response(404);return}
            val length=headers["content-length"]?.toIntOrNull() ?: 0
            if(length !in 1..4096) {response(404);return}
            val data=ByteArray(length);var offset=0
            while(offset<length){val n=input.read(data,offset,length-offset);if(n<0) return;offset+=n}
            try {
                val body=JSONObject(String(data,Charsets.UTF_8))
                val result=if(path.endsWith("/start")) PairingWindow.start(body) else PairingWindow.finish(body,identity.pairing(hostAddress,port))
                if(result==null) response(202,"{}".toByteArray()) else response(200,result.toString().toByteArray())
            }catch(_:Exception){response(401)}
            return
        }
        val expected="Basic "+Base64.encodeToString("musiccraft:${identity.token}".toByteArray(),Base64.NO_WRAP)
        if(!MessageDigest.isEqual(expected.toByteArray(),headers["authorization"].orEmpty().toByteArray())) {response(401);return}
        if(path.startsWith("/v1/sync/")) {
            if(!FolderSync.computer(context) || SharingGate.busy(context)){response(503);return}
            try {
                val parsed=android.net.Uri.parse(path)
                val route=parsed.path.orEmpty()
                fun body(limit:Int):JSONObject {
                    check(!headers.containsKey("transfer-encoding"));val length=headers["content-length"]?.toIntOrNull() ?: error("Missing size");check(length in 1..limit)
                    val bytes=ByteArray(length);var at=0;while(at<length){val n=input.read(bytes,at,length-at);if(n<0)throw EOFException();at+=n};return JSONObject(String(bytes,Charsets.UTF_8))
                }
                when {
                    method=="GET" && route=="/v1/sync/snapshot" -> {
                        val snapshot=FolderSync.tree(context).use{SyncFiles.snapshot(it,SyncFiles.root(it),java.util.concurrent.atomic.AtomicBoolean(false)){!closed && !SharingGate.busy(context) && FolderSync.computer(context)}}
                        if(SharingGate.busy(context) || closed){response(503);return};response(200,snapshot.toString().toByteArray())
                    }
                    method=="POST" && route=="/v1/sync/plan" -> {val b=body(48*1024*1024);response(200,FolderSync.submit(context,b.getString("id"),b.getJSONObject("plan")).toString().toByteArray())}
                    method=="POST" && Regex("/v1/sync/finish/[a-f0-9]{32}").matches(route) -> response(200,FolderSync.finish(context,route.substringAfterLast('/')).toString().toByteArray())
                    method=="GET" && Regex("/v1/sync/plan/[a-f0-9]{32}").matches(route) -> response(200,FolderSync.status(route.substringAfterLast('/')).toString().toByteArray())
                    method=="POST" && Regex("/v1/sync/apply/[a-f0-9]{32}/[0-9]{1,6}").matches(route) -> {
                        check(!headers.containsKey("transfer-encoding"));val parts=route.split('/');val length=headers["content-length"]?.toLongOrNull() ?: error("Missing size");check(length in 0..(8L*1024*1024*1024))
                        response(200,FolderSync.remoteApply(context,parts[4],parts[5].toInt(),length,input).toString().toByteArray())
                    }
                    method=="GET" && route=="/v1/sync/file" -> {
                        val relative=parsed.getQueryParameter("path") ?: error("Missing path");SyncFiles.checkPath(relative);check(!relative.split('/').any{it.startsWith('.')})
                        val temporary=File.createTempFile("sync-transfer-",".tmp",context.cacheDir)
                        try {
                            FolderSync.tree(context).use{s->s.read(relative).use{source->temporary.outputStream().use{dest->val bytes=ByteArray(65536);var total=0L;while(true){check(!closed && !SharingGate.busy(context));val n=source.read(bytes);if(n<0)break;total+=n;check(total<=8L*1024*1024*1024);dest.write(bytes,0,n)}}}}
                            output.write("HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Length: ${temporary.length()}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n".toByteArray(Charsets.US_ASCII))
                            temporary.inputStream().use{source->val b=ByteArray(65536);while(true){check(!closed && !SharingGate.busy(context));val n=source.read(b);if(n<0)break;output.write(b,0,n)}};output.flush()
                        }finally{temporary.delete()}
                    }
                    else -> response(405)
                }
            }catch(_:Exception){response(503)}
            return
        }
        val ratingRoute=Regex("/v1/ratings/(\\d+)(?:/([a-f0-9]{32}))?").matchEntire(path)
        if(ratingRoute!=null){
            if(SharingGate.busy(context)){response(503);return}
            if(!catalog.permitted()){response(503);return}
            val song=catalog.refresh().get(20,TimeUnit.SECONDS).find{it.id=="song:${ratingRoute.groupValues[1]}"}
            if(song==null){response(404);return}
            if(method=="POST" && ratingRoute.groupValues[2].isEmpty()){
                val length=headers["content-length"]?.toIntOrNull() ?: 0
                if(headers.containsKey("transfer-encoding") || length !in 1..1024){response(405);return}
                try{val b=ByteArray(length);var at=0;while(at<length){val n=input.read(b,at,length-at);if(n<0)return;at+=n}
                    val v=JSONObject(String(b,Charsets.UTF_8));val result=RatingTags.enqueue(context,ratingRoute.groupValues[1],v.getInt("rating"),v.getInt("scale"),v.getString("request"));response(202,result.toString().toByteArray())
                }catch(_:Exception){response(405)}
            }else if(method=="GET" && ratingRoute.groupValues[2].isNotEmpty()){
                val result=RatingTags.status(context,ratingRoute.groupValues[2]);if(result==null || result.optString("song")!=ratingRoute.groupValues[1])response(404) else response(200,result.toString().toByteArray())
            }else response(405)
            return
        }
        if(method!="GET" && method!="HEAD") {response(405,extra="Allow: GET, HEAD\r\n");return}
        if(path=="/v1/status") {response(200,JSONObject().put("protocol",1).put("device_id",identity.id).put("sharing",!SharingGate.busy(context)).toString().toByteArray());return}
        if(SharingGate.busy(context)) {response(503,"{\"error\":\"local_playback\"}".toByteArray(),extra="Retry-After: 5\r\n");return}
        if(!catalog.permitted()) {response(503);return}
        val songs=catalog.refresh().get(20,TimeUnit.SECONDS)
        if(path=="/v1/catalog") {
            val tracks=JSONArray()
            for(song in songs) {
                val ext=extension(song);if(ext==null || song.size<=0) continue
                tracks.put(JSONObject().put("id",song.id.substringAfter(':')).put("filename",song.id.substringAfter(':')+"."+ext).put("title",song.title.take(1024)).put("artist",song.artist.take(1024)).put("album",song.album.take(1024)).put("duration",song.duration/1000.0).put("number",song.trackNumber%1000).put("disc",song.trackNumber/1000).put("bytes",song.size).put("modified",song.modified).put("sync_version",song.syncVersion))
            }
            val bytes=JSONObject().put("protocol",1).put("device_id",identity.id).put("tracks",tracks).toString().toByteArray()
            if(SharingGate.busy(context)) {response(503);return}
            if(bytes.size>16*1024*1024) {response(503);return};response(200,bytes);return
        }
        val match=Regex("/v1/audio/(\\d+)\\.([a-z0-9]+)").matchEntire(path)
        if(match==null) {response(404);return}
        val song=songs.find {it.id=="song:"+match.groupValues[1] && extension(it)==match.groupValues[2]}
        if(song==null) {response(404);return}
        val range=ShareRange.parse(headers["range"],song.size)
        if(range==null) {response(416,extra="Content-Range: bytes */${song.size}\r\n");return}
        context.contentResolver.openAssetFileDescriptor(song.uri,"r")?.use {descriptor ->
            descriptor.createInputStream().use {file ->
                file.channel.position(descriptor.startOffset+range.first)
                val length=range.last-range.first+1;val partial=headers["range"]!=null
                val header="HTTP/1.1 ${if(partial) "206 Partial Content" else "200 OK"}\r\nContent-Type: application/octet-stream\r\nContent-Length: $length\r\nAccept-Ranges: bytes\r\nCache-Control: private, no-store\r\nConnection: close\r\n"+(if(partial)"Content-Range: bytes ${range.first}-${range.last}/${song.size}\r\n" else "")+"\r\n"
                output.write(header.toByteArray(Charsets.US_ASCII))
                if(method!="HEAD") {var left=length;val bytes=ByteArray(64*1024);while(left>0) {if(closed || SharingGate.busy(context)) throw IOException("Sharing suspended");val n=file.read(bytes,0,minOf(left,bytes.size.toLong()).toInt());if(n<0) throw EOFException();output.write(bytes,0,n);left-=n}}
                output.flush()
            }
        } ?: response(404)
    }
    companion object {
        fun extension(song:Song):String? {
            val ext=song.displayName.substringAfterLast('.',"").lowercase(java.util.Locale.ROOT)
            return ext.takeIf{it in setOf("flac","wav","aif","aiff","m4a","alac","mp3","aac","ogg","oga","mp4")}
        }
    }
}
