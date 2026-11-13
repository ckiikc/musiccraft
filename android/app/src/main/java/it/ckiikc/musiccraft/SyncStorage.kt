package it.ckiikc.musiccraft

import android.content.Context
import android.net.Uri
import android.provider.DocumentsContract as Docs
import org.json.JSONObject
import java.io.*
import java.security.MessageDigest
import java.util.UUID
import java.util.concurrent.atomic.AtomicBoolean

data class SyncNode(val name:String,val directory:Boolean,val size:Long=0,val modified:Long=0)
interface SyncStorage:Closeable {
    fun beforeReplacement(path:String) {}
    val fingerprintCache:File? get()=null
    fun children(path:String):List<SyncNode>
    fun read(path:String):InputStream
    fun create(path:String):OutputStream
    fun mkdir(path:String)
    fun move(from:String,to:String)
    fun removeTemporary(path:String)
    override fun close() {}
}

/** Paths originate only in a validated preview. Originals are moved to recovery, never deleted. */
object SyncFiles {
    fun id()=UUID.randomUUID().toString().replace("-","")
    fun checkPath(path:String){require(path.isNotEmpty() && path.length<=1024 && path.split('/').all{it.isNotEmpty() && it!="." && it!=".." && !it.contains('\\') && !it.contains(':') && it.none{c->c.isISOControl()}}){"Percorso non valido"}}
    fun exists(s:SyncStorage,path:String):Boolean {checkPath(path);return s.children(path.substringBeforeLast('/',"")).any{it.name==path.substringAfterLast('/')}}
    fun parents(s:SyncStorage,path:String){var part="";for(p in path.split('/').dropLast(1)){part=if(part.isEmpty())p else "$part/$p";if(!exists(s,part))s.mkdir(part)}}
    fun stamp(input:InputStream,cancel:AtomicBoolean=AtomicBoolean(false),allowed:()->Boolean={true}):JSONObject {
        val hash=MessageDigest.getInstance("SHA-256");var size=0L;val b=ByteArray(65536)
        while(true){check(!cancel.get() && !Thread.currentThread().isInterrupted && allowed()){"Sincronizzazione annullata"};val n=input.read(b);if(n<0)break;hash.update(b,0,n);size+=n}
        return JSONObject().put("size",size).put("sha256",hash.digest().joinToString(""){"%02x".format(it)})
    }
    fun same(a:JSONObject?,b:JSONObject?)=if(a==null || b==null)a==null && b==null else a.optLong("size",-1)==b.optLong("size",-2) && a.optString("sha256")==b.optString("sha256")
    fun invalidate(s:SyncStorage,paths:List<String>){val file=s.fingerprintCache ?: return;if(!file.isFile)return;val cache=try{if(file.length()<32*1024*1024)JSONObject(file.readText())else JSONObject()}catch(_:Exception){JSONObject()};val files=cache.optJSONObject("files") ?: JSONObject();paths.forEach{files.remove(it)};FolderSync.save(file,JSONObject().put("root",root(s)).put("files",files))}
    fun current(s:SyncStorage,path:String,cancel:AtomicBoolean):JSONObject?=if(exists(s,path))s.read(path).use{stamp(it,cancel)}else null
    fun root(s:SyncStorage):String {
        val marker=".musiccraft-sync/root-id";parents(s,marker)
        if(!exists(s,marker))s.create(marker).use{it.write(id().toByteArray())}
        val value=s.read(marker).use{String(it.takeBytes(128),Charsets.UTF_8)}
        require(value.matches(Regex("[a-f0-9]{32}"))){"Identità cartella non valida"};return value
    }
    fun snapshot(s:SyncStorage,root:String,cancel:AtomicBoolean,allowed:()->Boolean={true}):JSONObject {
        check(root(s)==root){"La cartella è cambiata o non è disponibile"}
        val files=JSONObject();var count=0;val now=System.currentTimeMillis();val cacheFile=s.fingerprintCache
        val wrapper=try{if(cacheFile!=null && cacheFile.isFile && cacheFile.length()<32*1024*1024)JSONObject(cacheFile.readText())else JSONObject()}catch(_:Exception){JSONObject()};val cache=if(wrapper.optString("root")==root)wrapper.optJSONObject("files") ?: JSONObject()else JSONObject();val next=JSONObject()
        fun walk(path:String,depth:Int){check(depth<=64){"Troppe sottocartelle"};val nodes=s.children(path);check(nodes.map{it.name}.distinct().size==nodes.size){"Nomi duplicati"}
            for(n in nodes){if(n.name.startsWith('.'))continue;check(++count<=200000){"Cartella troppo grande"};val p=if(path.isEmpty())n.name else "$path/${n.name}";checkPath(p);check(!cancel.get() && !Thread.currentThread().isInterrupted && allowed()){"Sincronizzazione annullata"}
                if(n.directory)walk(p,depth+1) else {check(files.length()<100000){"Troppi file"};val signature="${n.size}:${n.modified}";val old=cache.optJSONObject(p);val age=now-(old?.optLong("checked",0) ?: 0)
                    val hit=old!=null && n.modified>0 && old.optString("signature")==signature && age in 0..86399999
                    val fingerprint=if(hit)old!!.getJSONObject("stamp")else s.read(p).use{stamp(it,cancel,allowed)}
                    files.put(p,fingerprint);next.put(p,JSONObject().put("signature",signature).put("checked",if(hit)old!!.getLong("checked")else now).put("stamp",fingerprint))
                }
            }
        }
        walk("",0);check(root(s)==root){"Cartella scollegata durante l’anteprima"};if(cacheFile!=null)FolderSync.save(cacheFile,JSONObject().put("root",root).put("files",next));return JSONObject().put("root","folder:$root").put("files",files)
    }
    fun apply(s:SyncStorage,path:String,expected:JSONObject?,source:Pair<JSONObject,InputStream>?,run:String,cancel:AtomicBoolean,canCommit:()->Boolean={true}) {
        checkPath(path);require(!path.split('/').any{it.startsWith('.')});require(run.matches(Regex("[a-f0-9]{32}")))
        check(same(current(s,path,cancel),expected)){"File cambiato dopo l’anteprima: $path"}
        val temp=".musiccraft-sync/staging/${id()}.tmp";val recovery=".musiccraft-sync/recovery/$run/$path";var staged=false;var archived=false
        try {
            if(source!=null){parents(s,temp);s.create(temp).use{output->val b=ByteArray(65536);var size=0L;while(true){check(!cancel.get()){"Sincronizzazione annullata"};val n=source.second.read(b);if(n<0)break;size+=n;check(size<=source.first.getLong("size")){"Trasferimento oltre il limite"};output.write(b,0,n)}};staged=true
                check(same(s.read(temp).use{stamp(it,cancel)},source.first)){"Verifica del file trasferito fallita"}
            }
            check(canCommit()){"Sincronizzazione sospesa: accesso revocato o riproduzione avviata"}
            check(same(current(s,path,cancel),expected)){"Destinazione modificata durante il trasferimento"}
            if(source!=null)s.beforeReplacement(path)
            if(expected!=null){parents(s,recovery);check(!exists(s,recovery));s.move(path,recovery);archived=true;check(same(s.read(recovery).use{stamp(it,cancel)},expected)){"File modificato durante lo spostamento nel recupero"}}
            if(source!=null){parents(s,path);s.move(temp,path);staged=false}
        } catch(e:Exception){if(archived && !exists(s,path)){try{s.move(recovery,path)}catch(_:Exception){}};throw e}
        finally{if(staged)try{s.removeTemporary(temp)}catch(_:Exception){}}
    }
}

/** Explicit persisted directory grant, restricted to the device's ExternalStorageProvider. */
class TreeSyncStorage(private val context:Context,private val tree:Uri):SyncStorage {
    override val fingerprintCache=File(context.filesDir,"sync-index/${MessageDigest.getInstance("SHA-256").digest(tree.toString().toByteArray()).joinToString(""){"%02x".format(it)}}.json")
    private val resolver=context.contentResolver
    init {check(android.os.Build.VERSION.SDK_INT>=29){"La sincronizzazione richiede Android 10 o successivo"};require(tree.authority=="com.android.externalstorage.documents"){"Scegli una cartella sul telefono o sulla scheda SD"};require(context.contentResolver.persistedUriPermissions.any{it.uri==tree && it.isReadPermission && it.isWritePermission}){"Autorizza lettura e scrittura della cartella"}}
    private val root=Docs.buildDocumentUriUsingTree(tree,Docs.getTreeDocumentId(tree))
    private fun entries(parent:Uri):List<Pair<SyncNode,Uri>> {
        val uri=Docs.buildChildDocumentsUriUsingTree(tree,Docs.getDocumentId(parent));val rows=mutableListOf<Pair<SyncNode,Uri>>()
        val cursor=resolver.query(uri,arrayOf(Docs.Document.COLUMN_DOCUMENT_ID,Docs.Document.COLUMN_DISPLAY_NAME,Docs.Document.COLUMN_MIME_TYPE,Docs.Document.COLUMN_SIZE,Docs.Document.COLUMN_LAST_MODIFIED),null,null,null) ?: error("Cartella non disponibile")
        cursor.use{while(it.moveToNext()){val name=it.getString(1) ?: error("Nome non valido");require(!name.contains('/') && !name.contains('\\'));rows+=SyncNode(name,it.getString(2)==Docs.Document.MIME_TYPE_DIR,it.getLong(3),it.getLong(4)) to Docs.buildDocumentUriUsingTree(tree,it.getString(0))}}
        return rows
    }
    private fun resolve(path:String):Uri {if(path.isEmpty())return root;SyncFiles.checkPath(path);var uri=root;for(part in path.split('/'))uri=entries(uri).singleOrNull{it.first.name==part}?.second ?: error("File non disponibile: $path");return uri}
    override fun children(path:String)=entries(resolve(path)).map{it.first}
    override fun beforeReplacement(path:String){if(path.substringAfterLast('.',"").lowercase(java.util.Locale.ROOT) in setOf("flac","mp3","m4a","aac","alac","mp4","ogg","oga","opus","wav","aif","aiff","ape","wma","wv","dsf","dff"))SyncIdentity.capture(context,Docs.buildDocumentUriUsingTree(tree,Docs.getDocumentId(root)+"/"+path))}
    override fun read(path:String)=resolver.openInputStream(resolve(path)) ?: error("Lettura non disponibile")
    override fun mkdir(path:String){SyncFiles.checkPath(path);val parent=resolve(path.substringBeforeLast('/',""));val made=Docs.createDocument(resolver,parent,Docs.Document.MIME_TYPE_DIR,path.substringAfterLast('/')) ?: error("Creazione cartella negata");check(entries(parent).any{it.second==made && it.first.name==path.substringAfterLast('/')})}
    override fun create(path:String):OutputStream {
        SyncFiles.checkPath(path);check(!SyncFiles.exists(this,path)){"Il file esiste già"};val parent=resolve(path.substringBeforeLast('/',""));val made=Docs.createDocument(resolver,parent,"application/octet-stream",path.substringAfterLast('/')) ?: error("Scrittura non consentita")
        check(entries(parent).any{it.second==made && it.first.name==path.substringAfterLast('/')}){"Il provider ha cambiato il nome"}
        val fd=resolver.openFileDescriptor(made,"w") ?: error("Scrittura non disponibile")
        return object:android.os.ParcelFileDescriptor.AutoCloseOutputStream(fd){override fun close(){try{flush();fd.fileDescriptor.sync()}finally{super.close()}}}
    }
    override fun move(from:String,to:String) {
        SyncFiles.checkPath(from);SyncFiles.checkPath(to);check(!SyncFiles.exists(this,to)){"Destinazione già presente"}
        val srcParent=resolve(from.substringBeforeLast('/',""));val dstParent=resolve(to.substringBeforeLast('/',""));var uri=resolve(from)
        val originalName=from.substringAfterLast('/');val wanted=to.substringAfterLast('/');var moved=false
        try {
            if(srcParent!=dstParent){uri=Docs.moveDocument(resolver,uri,srcParent,dstParent) ?: error("Spostamento non supportato");moved=true}
            if(originalName!=wanted)uri=Docs.renameDocument(resolver,uri,wanted) ?: error("Rinomina non supportata")
            check(entries(dstParent).any{it.second==uri && it.first.name==wanted}){"La destinazione è cambiata durante il trasferimento"}
        }catch(e:Exception){
            try {val name=entries(if(moved)dstParent else srcParent).find{it.second==uri}?.first?.name
                if(name!=null && name!=originalName)uri=Docs.renameDocument(resolver,uri,originalName) ?: uri
                if(moved)Docs.moveDocument(resolver,uri,dstParent,srcParent)
            }catch(_:Exception){}
            throw e
        }
    }
    override fun removeTemporary(path:String){require(path.startsWith(".musiccraft-sync/staging/"));Docs.deleteDocument(resolver,resolve(path))}
}

private fun InputStream.takeBytes(limit:Int):ByteArray {val out=ByteArrayOutputStream();val b=ByteArray(64);while(out.size()<limit){val n=read(b,0,minOf(b.size,limit-out.size()));if(n<0)break;out.write(b,0,n)};return out.toByteArray()}
