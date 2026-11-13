package it.ckiikc.musiccraft

import android.content.Context
import android.net.Uri
import android.util.AtomicFile
import org.json.JSONObject
import java.io.File
import java.io.InputStream
import java.util.concurrent.atomic.AtomicBoolean

/** Shared Rust planner; Android adapters perform only the explicitly reviewed operations. */
object FolderSync {
    init {System.loadLibrary("musiccraft_tags")}
    @JvmStatic private external fun planNative(request:String):String
    private fun prefs(c:Context)=c.getSharedPreferences("folder-sync-v1",Context.MODE_PRIVATE)
    fun folder(c:Context)=prefs(c).getString("folder",null)
    fun setFolder(c:Context,uri:Uri){check(prefs(c).edit().putString("folder",uri.toString()).putBoolean("computer",false).commit());disableRemote()}
    fun computer(c:Context)=prefs(c).getBoolean("computer",false)
    fun computer(c:Context,enabled:Boolean){check(folder(c)!=null || !enabled);check(prefs(c).edit().putBoolean("computer",enabled).commit());if(!enabled)disableRemote()}
    fun tree(c:Context)=TreeSyncStorage(c,Uri.parse(folder(c) ?: error("Scegli una cartella musicale")))
    @Volatile var working=false
    val cancel=AtomicBoolean(false)
    private val ioLock=MusicWriteGate.lock
    private var remoteId:String?=null
    private var remotePlan:JSONObject?=null
    private var remoteState="none"
    private var expires=0L
    private val completed=mutableSetOf<Int>()
    private fun now()=android.os.SystemClock.elapsedRealtime()
    @Synchronized fun disableRemote(){remoteId=null;remotePlan=null;remoteState="none";completed.clear()}
    @Synchronized fun pending():Pair<String,JSONObject>?=if(remoteState=="pending" && now()<expires)remotePlan?.let{remoteId!! to it}else null
    @Synchronized fun approve(id:String,yes:Boolean){check(id==remoteId && now()<expires && remoteState=="pending");remoteState=if(yes)"approved"else"rejected"}
    @Synchronized fun status(id:String):JSONObject {check(id==remoteId && now()<expires);return JSONObject().put("state",remoteState)}
    fun finish(c:Context,id:String):JSONObject {
        val p=synchronized(this){check(id==remoteId && remoteState=="approved" && now()<expires);remotePlan ?: error("Anteprima scaduta")}
        check(ioLock.tryLock()){"Sincronizzazione in corso"}
        try {val expected=JSONObject(p.getJSONObject("before").getJSONObject("b").toString());val files=expected.getJSONObject("files");val ops=p.getJSONArray("operations")
        for(i in 0 until ops.length()){val op=ops.getJSONObject(i);val action=op.getJSONObject("action");if(action.optString("Copy")=="B")files.put(op.getString("path"),op.getJSONObject("a")) else if(action.optString("Delete")=="B")files.remove(op.getString("path"))}
        tree(c).use{s->check(sameSnapshot(SyncFiles.snapshot(s,SyncFiles.root(s),AtomicBoolean(false)),expected)){"Verifica finale telefono fallita"}}
        synchronized(this){check(id==remoteId);remoteState="completed"};return JSONObject().put("ok",true)
        }finally{ioLock.unlock()}
    }
    fun preview(a:JSONObject,b:JSONObject,direction:String,baseline:JSONObject?):JSONObject {
        val result=JSONObject(planNative(JSONObject().put("a",a).put("b",b).put("direction",direction).put("baseline",baseline ?: JSONObject.NULL).toString()))
        check(!result.has("error")){result.optString("error")};return result
    }
    fun sameSnapshot(a:JSONObject,b:JSONObject):Boolean {
        if(a.getString("root")!=b.getString("root"))return false
        val x=a.getJSONObject("files");val y=b.getJSONObject("files");return x.length()==y.length() && x.keys().asSequence().all{SyncFiles.same(x.getJSONObject(it),y.optJSONObject(it))}
    }
    fun save(file:File,value:JSONObject) {
        file.parentFile?.mkdirs();val atomic=AtomicFile(file);val stream=atomic.startWrite()
        try{stream.write(value.toString().toByteArray());atomic.finishWrite(stream)}catch(e:Exception){atomic.failWrite(stream);throw e}
    }
    fun submit(c:Context,id:String,plan:JSONObject):JSONObject {
        require(computer(c) && id.matches(Regex("[a-f0-9]{32}")) && !working)
        synchronized(this){if(remoteId==id)return status(id)}
        val before=plan.getJSONObject("before");val validated=preview(before.getJSONObject("a"),before.getJSONObject("b"),"Both",null)
        check(validated.has("operations"))
        val operations=plan.getJSONArray("operations");check(operations.length()<=100000)
        val seen=mutableSetOf<String>()
        for(i in 0 until operations.length()){val op=operations.getJSONObject(i);val path=op.getString("path");SyncFiles.checkPath(path);check(!path.split('/').any{it.startsWith('.')} && seen.add(path));val action=op.get("action");check(action is JSONObject && action.length()==1 && (action.has("Copy") || action.has("Delete")));check(action.optString("Copy",action.optString("Delete")) in listOf("A","B"));check(SyncFiles.same(op.optJSONObject("a"),before.getJSONObject("a").getJSONObject("files").optJSONObject(path)));check(SyncFiles.same(op.optJSONObject("b"),before.getJSONObject("b").getJSONObject("files").optJSONObject(path)))}
        tree(c).use{s->check(sameSnapshot(SyncFiles.snapshot(s,SyncFiles.root(s),AtomicBoolean(false)),before.getJSONObject("b"))){"La cartella del telefono è cambiata"}}
        val owned=JSONObject(plan.toString())
        return synchronized(this){check(computer(c) && !working);remoteId=id;remotePlan=owned;remoteState="pending";expires=now()+20*60*1000;completed.clear();JSONObject().put("state",remoteState)}
    }
    fun remoteApply(c:Context,id:String,index:Int,length:Long,input:InputStream):JSONObject {
        val plan=synchronized(this){check(computer(c) && id==remoteId && remoteState=="approved" && now()<expires && !working && !SharingGate.busy(c));check(index !in completed){"Operazione già eseguita"};remotePlan ?: error("Anteprima scaduta")}
        check(ioLock.tryLock()){"Sincronizzazione in corso"};working=true
        try {val operationCancel=AtomicBoolean(false);check(RatingTags.recoverNow(c)){"Recupero dei tag richiesto"};val op=plan.getJSONArray("operations").getJSONObject(index);val action=op.getJSONObject("action");check(action.optString("Copy",action.optString("Delete"))=="B")
        val source=if(action.has("Copy"))op.getJSONObject("a") else null
        check(length==(source?.getLong("size") ?: 0L))
        tree(c).use {s->check("folder:${SyncFiles.root(s)}"==plan.getJSONObject("before").getJSONObject("b").getString("root"))
            val ops=plan.getJSONArray("operations");SyncFiles.invalidate(s,(0 until ops.length()).map{ops.getJSONObject(it)}.filter{it.getJSONObject("action").optString("Copy",it.getJSONObject("action").optString("Delete"))=="B"}.map{it.getString("path")})
            save(File(c.filesDir,"sync-remote-journal.json"),JSONObject().put("id",id).put("index",index).put("operation",op))
            val bounded=object:InputStream(){var left=length;override fun read():Int {val b=ByteArray(1);return if(read(b,0,1)<0)-1 else b[0].toInt() and 255};override fun read(b:ByteArray,off:Int,len:Int):Int {if(left==0L)return -1;val n=input.read(b,off,minOf(len.toLong(),left).toInt());if(n<0)throw java.io.EOFException();left-=n;return n}}
            SyncFiles.apply(s,op.getString("path"),op.optJSONObject("b"),source?.let{it to bounded},id,operationCancel){computer(c) && !SharingGate.busy(c) && synchronized(this){id==remoteId && remoteState=="approved"}}
        }
        synchronized(this){if(id==remoteId){completed+=index;expires=now()+20*60*1000}};Catalog.get(c).refresh(force=true).get(20,java.util.concurrent.TimeUnit.SECONDS)
        return JSONObject().put("ok",true)
        }finally{working=false;ioLock.unlock()}
    }
    fun execute(c:Context,plan:JSONObject,a:SyncStorage,b:SyncStorage,progress:(Int,Int)->Unit):JSONObject {
        check(ioLock.tryLock()){"Sincronizzazione già in corso"};working=true;cancel.set(false)
        try {
            check(RatingTags.recoverNow(c)){"Recupero dei tag richiesto"}
            val before=plan.getJSONObject("before");val aId=SyncFiles.root(a);val bId=SyncFiles.root(b)
            check(sameSnapshot(SyncFiles.snapshot(a,aId,cancel),before.getJSONObject("a")) && sameSnapshot(SyncFiles.snapshot(b,bId,cancel),before.getJSONObject("b"))){"Cartelle cambiate: genera una nuova anteprima"}
            val operations=plan.getJSONArray("operations");val run=SyncFiles.id()
            for((side,storage)in listOf("A" to a,"B" to b))SyncFiles.invalidate(storage,(0 until operations.length()).map{operations.getJSONObject(it)}.filter{val action=it.optJSONObject("action");action!=null && action.optString("Copy",action.optString("Delete"))==side}.map{it.getString("path")})
            for(i in 0 until operations.length()){
                val op=operations.getJSONObject(i);val action=op.optJSONObject("action") ?: error("Risolvi i conflitti")
                val copy=action.has("Copy");val target=action.getString(if(copy)"Copy" else "Delete");val dest=if(target=="A")a else b;val src=if(target=="A")b else a;val path=op.getString("path")
                save(File(c.filesDir,"sync-local-journal.json"),JSONObject().put("transaction",run).put("plan",plan).put("completed",i))
                val old=op.optJSONObject(if(target=="A")"a" else "b");val fingerprint=op.optJSONObject(if(target=="A")"b" else "a")
                if(copy)src.read(path).use{SyncFiles.apply(dest,path,old,(fingerprint ?: error("Origine assente")) to it,run,cancel)} else SyncFiles.apply(dest,path,old,null,run,cancel)
                progress(i+1,operations.length())
            }
            val after=JSONObject(before.toString())
            for(i in 0 until operations.length()){val op=operations.getJSONObject(i);val action=op.getJSONObject("action");val copy=action.has("Copy");val target=action.getString(if(copy)"Copy" else "Delete").lowercase();val files=after.getJSONObject(target).getJSONObject("files");if(copy)files.put(op.getString("path"),op.getJSONObject(if(target=="a")"b" else "a")) else files.remove(op.getString("path"))}
            check(sameSnapshot(SyncFiles.snapshot(a,aId,cancel),after.getJSONObject("a")) && sameSnapshot(SyncFiles.snapshot(b,bId,cancel),after.getJSONObject("b"))){"Verifica finale fallita; originali conservati nel recupero"}
            return after
        }finally{working=false;ioLock.unlock()}
    }
}
