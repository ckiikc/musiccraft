package it.ckiikc.musiccraft

import android.app.Activity
import android.app.AlertDialog
import android.content.Context
import android.content.Intent
import android.os.Build
import android.os.ParcelFileDescriptor
import android.provider.MediaStore
import android.util.AtomicFile
import org.json.JSONObject
import java.io.File
import java.security.MessageDigest
import java.util.concurrent.Executors

/** Only VOTE/RATING metadata patches; no endpoint can upload or replace an audio payload. */
object RatingTags {
    init { System.loadLibrary("musiccraft_tags") }
    @JvmStatic private external fun inspectNative(fd:Int):String
    @JvmStatic private external fun patchNative(fd:Int,rating:Int,scale:Int):String
    private val indexWorker=Executors.newSingleThreadExecutor()
    @Volatile private var indexed=0
    private val worker=Executors.newSingleThreadExecutor()
    private var showing:String?=null
    private var permission:String?=null
    private const val REQUEST=8452
    private fun prefs(c:Context)=c.getSharedPreferences("rating-tag-edits-v1",Context.MODE_PRIVATE)
    private fun requests(c:Context)=JSONObject(prefs(c).getString("requests","{}")!!)
    @Synchronized private fun save(c:Context,key:String,value:JSONObject){val all=requests(c);all.put(key,value);check(prefs(c).edit().putString("requests",all.toString()).commit())}
    @Synchronized fun enqueue(c:Context,id:String,rating:Int,scale:Int,request:String):JSONObject {
        require(id.isNotEmpty() && id.all{it in '0'..'9'} && rating in 0..5 && scale in listOf(5,100) && request.matches(Regex("[a-f0-9]{32}")))
        val all=requests(c)
        all.optJSONObject(request)?.let{require(it.getString("song")==id && it.getInt("rating")==rating && it.getInt("scale")==scale);return it}
        require(all.keys().asSequence().map{all.getJSONObject(it)}.count{it.optString("state")=="waiting"}<16)
        // Retain bounded request history. Never evict an awaiting approval.
        if(all.length()>=128) all.keys().asSequence().firstOrNull{all.getJSONObject(it).optString("state")!="waiting"}?.let{all.remove(it)}
        val value=JSONObject().put("song",id).put("rating",rating).put("scale",scale).put("state","waiting")
        val song=Catalog.get(c).songs.find{it.id=="song:$id"}
        if(song!=null)try{val summary=inspect(c,song.uri);if(resolve(summary,scale)==rating)value.put("state","done").put("summary",summary)}catch(_:Exception){}

        all.put(request,value);check(prefs(c).edit().putString("requests",all.toString()).commit());return value
    }
    @Synchronized fun status(c:Context,request:String):JSONObject? = requests(c).optJSONObject(request)
    fun inspect(c:Context,uri:android.net.Uri):JSONObject = c.contentResolver.openFileDescriptor(uri,"r")!!.use{JSONObject(inspectNative(it.fd))}
    fun local(c:Context,id:String,rating:Int){val scale=prefs(c).getInt("scale",0);require(scale==5 || scale==100){"Scegli la scala dei voti dal menu Voti nei tag"};enqueue(c,id,rating,scale,java.util.UUID.randomUUID().toString().replace("-",""))}
    fun settings(a:Activity){val values=intArrayOf(0,5,100);val current=prefs(a).getInt("scale",0)
        AlertDialog.Builder(a).setTitle("Scala di VOTE/RATING").setSingleChoiceItems(arrayOf("Solo scala dichiarata (es. 4/5)","Da 0 a 5 stelle","Da 0 a 100"),values.indexOf(current)){d,n->prefs(a).edit().putInt("scale",values[n]).apply();indexed=0;index(a.applicationContext,Catalog.get(a).songs);d.dismiss()}.setNegativeButton("Chiudi",null).show()
    }
    fun index(c:Context,songs:List<Song>){
        val scale=prefs(c).getInt("scale",0);val signature=31*songs.hashCode()+scale
        if(indexed==signature)return;indexed=signature
        indexWorker.execute{
            android.os.Process.setThreadPriority(android.os.Process.THREAD_PRIORITY_BACKGROUND)
            val db=RatingIndex(c).writableDatabase
            val batch=mutableMapOf<String,Int?>()
            try{for(song in songs){
                if(SharingGate.busy(c)) {indexed=0;break}
                try{
                    val fingerprint="${song.size}:${song.modified}";var summary:JSONObject?=null
                    db.rawQuery("SELECT summary FROM ratings WHERE id=? AND fingerprint=?",arrayOf(song.statKey,fingerprint)).use{if(it.moveToFirst())summary=JSONObject(it.getString(0))}
                    if(summary==null){summary=inspect(c,song.uri);db.execSQL("INSERT OR REPLACE INTO ratings(id,fingerprint,summary) VALUES(?,?,?)",arrayOf(song.statKey,fingerprint,summary.toString()))}
                    val rating=resolve(summary!!,scale);batch[song.statKey]=rating
                    if(batch.size>=256){ListeningStore.get(c).importRatings(batch);batch.clear()}
                }catch(_:Exception){}
            };if(batch.isNotEmpty())ListeningStore.get(c).importRatings(batch)}finally{db.close()}
        }
    }
    internal fun resolve(summary:JSONObject,scale:Int):Int?{
        if(!summary.isNull("issue"))return null;val values=summary.optJSONArray("values") ?: return null
        var result:Int?=null
        for(i in 0 until values.length()){
            val v=values.getString(i).trim().split('/');val n=v[0].trim().toIntOrNull() ?: return null
            val max=if(v.size==2)v[1].trim().toIntOrNull() ?: return null else if(v.size==1)scale else return null
            if(max !in listOf(5,100) || n !in 0..max || n*5%max!=0)return null
            val rating=n*5/max;if(result!=null && result!=rating)return null;result=rating
        }
        return result ?: 0
    }
    /** Approval is in the phone UI. An authenticated computer cannot silently edit media. */
    fun present(a:Activity){
        if(showing!=null || permission!=null || a.isFinishing || SharingGate.busy(a))return
        val all=requests(a);val key=all.keys().asSequence().firstOrNull{all.getJSONObject(it).optString("state")=="waiting"} ?: return
        val request=all.getJSONObject(key);val song=Catalog.get(a).songs.find{it.id=="song:${request.getString("song")}"} ?: return
        showing=key
        AlertDialog.Builder(a).setTitle("Aggiorna voto nel file audio")
            .setMessage("${song.artist} — ${song.title}\n\nNuovo voto: ${request.getInt("rating")}/5. Verranno aggiornati solo VOTE/RATING. I dati audio restano identici. Android può richiedere l’autorizzazione alla modifica.")
            .setPositiveButton("Aggiorna tag"){_,_->showing=null;permission=key
                if(Build.VERSION.SDK_INT>=30) try{a.startIntentSenderForResult(MediaStore.createWriteRequest(a.contentResolver,listOf(song.uri)).intentSender,REQUEST,null,0,0,0)}catch(e:Exception){permission=null;fail(a,key,e.message ?: "Autorizzazione non disponibile")}
                else {permission=null;apply(a.applicationContext,key)}
            }.setNegativeButton("Annulla"){_,_->showing=null;fail(a,key,"Modifica annullata; file originale conservato")}
            .setOnCancelListener{showing=null;fail(a,key,"Modifica annullata; file originale conservato")}.show()
    }
    fun result(a:Activity,requestCode:Int,resultCode:Int):Boolean {
        if(requestCode!=REQUEST)return false
        val key=permission ?: return true;permission=null
        if(resultCode==Activity.RESULT_OK)apply(a.applicationContext,key) else fail(a,key,"Autorizzazione negata; file originale conservato")
        return true
    }
    @Synchronized private fun fail(c:Context,key:String,error:String){status(c,key)?.let{it.put("state","error").put("error",error.take(512));save(c,key,it);android.os.Handler(android.os.Looper.getMainLooper()).post{android.widget.Toast.makeText(c,"Voto nel tag non salvato: $error",android.widget.Toast.LENGTH_LONG).show()}}}
    private fun hash(b:ByteArray)=MessageDigest.getInstance("SHA-256").digest(b).joinToString(""){"%02x".format(it)}
    private fun prefix(c:Context,uri:android.net.Uri,n:Int):ByteArray {val b=ByteArray(n);c.contentResolver.openInputStream(uri)!!.use{var at=0;while(at<n){val m=it.read(b,at,n-at);check(m>0);at+=m}};return b}
    private fun write(c:Context,uri:android.net.Uri,b:ByteArray){
        val fd=c.contentResolver.openFileDescriptor(uri,"rw") ?: error("File non scrivibile")
        ParcelFileDescriptor.AutoCloseOutputStream(fd).use{out->out.channel.position(0);out.write(b);out.flush();out.fd.sync()}
    }
    private fun journal(c:Context)=AtomicFile(File(c.filesDir,"rating-tag-journal.json"))
    private fun commitJournal(c:Context,v:JSONObject){val j=journal(c);val stream=j.startWrite();try{stream.write(v.toString().toByteArray());j.finishWrite(stream)}catch(e:Exception){j.failWrite(stream);throw e}}
    private fun array(v:org.json.JSONArray)=ByteArray(v.length()){v.getInt(it).also{n->require(n in 0..255)}.toByte()}
    internal fun apply(c:Context,key:String){worker.execute{
        try {
            check(recoverNow(c)){"Una modifica precedente richiede recupero; nessun altro file verrà modificato"}
            val request=status(c,key) ?: return@execute
            val song=Catalog.get(c).songs.find{it.id=="song:${request.getString("song")}"} ?: error("Brano non disponibile")
            check(!SharingGate.busy(c)){"Riproduzione attiva: modifica rinviata"}
            val patch=c.contentResolver.openFileDescriptor(song.uri,"r")!!.use{JSONObject(patchNative(it.fd,request.getInt("rating"),request.getInt("scale")))}
            check(!patch.has("error")){patch.optString("error")}
            val before=array(patch.getJSONArray("before"));val after=array(patch.getJSONArray("after"));require(before.size==after.size && before.size<=8*1024*1024)
            check(hash(prefix(c,song.uri,before.size))==patch.getString("before_sha256")){"I tag sono cambiati durante la modifica"}
            // Persist original metadata before the first write. Audio bytes are never written.
            commitJournal(c,JSONObject().put("request",key).put("uri",song.uri.toString()).put("before",android.util.Base64.encodeToString(before,2)).put("after",android.util.Base64.encodeToString(after,2)))
            write(c,song.uri,after)
            check(hash(prefix(c,song.uri,after.size))==patch.getString("after_sha256")){"Verifica dei tag non riuscita"}
            val summary=inspect(c,song.uri);request.put("state","done").put("summary",summary);save(c,key,request)
            ListeningStore.get(c).rate(song.statKey,request.getInt("rating"));
            RatingIndex(c).writableDatabase.use{it.delete("ratings","id=?",arrayOf(song.statKey))};indexed=0
            c.contentResolver.notifyChange(song.uri,null);journal(c).delete()
            android.os.Handler(android.os.Looper.getMainLooper()).post{android.widget.Toast.makeText(c,"Voto salvato nei tag audio",android.widget.Toast.LENGTH_SHORT).show()}
        }catch(e:Exception){recoverNow(c);fail(c,key,e.message ?: "Salvataggio non riuscito; copia di recupero conservata")}
    }}
    /** A partially written prefix can only contain old/new metadata bytes; never overwrite unrelated edits. */
    @Synchronized fun recoverNow(c:Context):Boolean {
        val j=journal(c);if(!j.baseFile.exists())return true
        return try {
            require(j.baseFile.length()<=24*1024*1024)
            val v=JSONObject(j.openRead().use{String(it.readBytes())});val uri=android.net.Uri.parse(v.getString("uri"))
            val before=android.util.Base64.decode(v.getString("before"),2);val after=android.util.Base64.decode(v.getString("after"),2)
            require(before.size==after.size && before.size<=8*1024*1024)
            val now=prefix(c,uri,before.size)
            if(now.contentEquals(after)){
                status(c,v.getString("request"))?.let{it.put("state","done").put("summary",inspect(c,uri));save(c,v.getString("request"),it)}
            }else{
                check(now.indices.all{now[it]==before[it] || now[it]==after[it]})
                write(c,uri,before);check(prefix(c,uri,before.size).contentEquals(before))
            }
            j.delete();true
        }catch(_:Exception){false}
    }
}
