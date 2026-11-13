package it.ckiikc.musiccraft
import android.content.Context
import android.util.Base64
import java.net.InetAddress
import java.net.URL
import java.security.MessageDigest
import java.security.SecureRandom
import java.security.cert.X509Certificate
import javax.net.ssl.*
import org.json.JSONObject
/** Synthetic, emulator-only caller supplied by the instrumentation runner. */
class SharingChecks(private val context:Context,serviceEndpoint:String?=null):java.io.Closeable {
    private val identity=SharingIdentity(context)
    private val server=if(serviceEndpoint==null)SharingServer(context,identity,InetAddress.getByName("127.0.0.1")) else null
    private val endpoint=serviceEndpoint ?: "https://127.0.0.1:${server!!.port}"
    private val tls=SSLContext.getInstance("TLS").apply {init(null,arrayOf(object:X509TrustManager {
        override fun getAcceptedIssuers()=emptyArray<X509Certificate>()
        override fun checkClientTrusted(c:Array<X509Certificate>,a:String){error("Not a client trust manager")}
        override fun checkServerTrusted(c:Array<X509Certificate>,a:String){check(c.isNotEmpty());check(Base64.encodeToString(MessageDigest.getInstance("SHA-256").digest(c[0].publicKey.encoded),Base64.NO_WRAP)==identity.pin)}
    }),SecureRandom())}
    data class Reply(val code:Int,val bytes:ByteArray,val length:Long)
    private fun request(path:String,method:String="GET",auth:Boolean=true,range:String?=null,body:String?=null):Reply {
        val connection=URL("$endpoint$path").openConnection() as HttpsURLConnection
        connection.sslSocketFactory=tls.socketFactory;connection.hostnameVerifier=HostnameVerifier{_,_->true};connection.connectTimeout=3000;connection.readTimeout=10000;connection.requestMethod=method;connection.instanceFollowRedirects=false
        if(auth) connection.setRequestProperty("Authorization","Basic "+Base64.encodeToString("musiccraft:${identity.token}".toByteArray(),Base64.NO_WRAP))
        if(range!=null) connection.setRequestProperty("Range",range)
        if(body!=null){connection.doOutput=true;connection.outputStream.use{it.write(body.toByteArray())}}
        return try {val code=connection.responseCode;val bytes=(if(code<400)connection.inputStream else connection.errorStream)?.use{it.readBytes()} ?: ByteArray(0);Reply(code,bytes,connection.contentLengthLong)} finally {connection.disconnect()}
    }
    fun ratingFailures(id:String){
        check(request("/v1/ratings/$id",method="POST",auth=false).code==401)
        for(body in listOf("{}","{\"rating\":6,\"scale\":5,\"request\":\"${"a".repeat(32)}\"}","{\"rating\":4,\"scale\":255,\"request\":\"${"b".repeat(32)}\"}")){
            check(request("/v1/ratings/$id",method="POST",body=body).code==405)
        }
        check(request("/v1/ratings/$id/${"c".repeat(32)}").code==404)
        check(request("/v1/ratings/99999999999999999",method="POST",body="{}").code==404)
    }
    fun idle() {
        val code=PairingWindow.open();check(code.length==4 && code.all{it.isDigit()});check(!PairingWindow.pending())
        for(i in 1..3) {
            val challenge=PairingWindow.start(JSONObject().put("a",Base64.encodeToString(byteArrayOf(2),Base64.NO_WRAP)))
            check(runCatching {PairingWindow.finish(JSONObject().put("session",challenge.getString("session")).put("proof",Base64.encodeToString(ByteArray(32),Base64.NO_WRAP)),"unused")}.isFailure)
        }
        check(runCatching {PairingWindow.start(JSONObject().put("a","Ag=="))}.isFailure)
        PairingWindow.close();check(PairingWindow.visibleCode()==null)

        check(SharingIdentity(context).let{it.id==identity.id && it.token==identity.token && it.pin==identity.pin}) {"Identity did not persist"}
        check(request("/v1/catalog",auth=false).code==401)
        check(request("/v1/catalog",method="DELETE").code==405)
        check(request("/v1/status").code==200)
        val response=request("/v1/catalog");check(response.code==200)
        val json=JSONObject(String(response.bytes));check(json.getString("device_id")==identity.id)
        val songs=json.getJSONArray("tracks");check(songs.length()==3)
        val song=songs.getJSONObject(0);val path="/v1/audio/"+song.getString("filename")
        val expected=context.contentResolver.openInputStream(Catalog.get(context).songs.first{it.id=="song:"+song.getString("id")}.uri)!!.use{it.readBytes()}
        val head=request(path,method="HEAD");check(head.code==200 && head.bytes.isEmpty() && head.length==expected.size.toLong())
        val partial=request(path,range="bytes=2-99");check(partial.code==206 && partial.bytes.contentEquals(expected.copyOfRange(2,100)))
        check(request(path,range="bytes=${expected.size}-").code==416)
        check(request("/v1/audio/../private").code==404)
        check(request(path).bytes.contentEquals(expected))
    }
    fun suspended() {check(request("/v1/catalog").code==503);check(!JSONObject(String(request("/v1/status").bytes)).getBoolean("sharing"))}
    fun resumed() {check(request("/v1/catalog").code==200)}
    fun pendingTlsClient():SSLSocket {
        val url=URL(endpoint)
        return (tls.socketFactory.createSocket(url.host,url.port) as SSLSocket).apply {
            soTimeout=10000;startHandshake()
            outputStream.write("GET /v1/status HTTP/1.1\r\n".toByteArray());outputStream.flush()
        }
    }
    fun awaitDisconnected(socket:SSLSocket) {
        try {check(socket.inputStream.read()==-1){"Pending request should be cancelled"}}
        catch(e:java.net.SocketTimeoutException){throw e}
        catch(_:java.io.IOException){}
    }
    fun cancelConnectedTransfers(onMain:(()->Unit)->Unit) {
        val owned=checkNotNull(server)
        pendingTlsClient().use {socket->onMain{owned.suspendTransfers()};awaitDisconnected(socket)}
        check(request("/v1/status").code==200)
        pendingTlsClient().use {socket->onMain{owned.close()};awaitDisconnected(socket)}
    }
    override fun close(){server?.close()}
}
