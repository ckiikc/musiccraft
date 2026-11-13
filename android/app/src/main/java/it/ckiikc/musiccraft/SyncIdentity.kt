package it.ckiikc.musiccraft

import android.content.ContentUris
import android.content.ContentValues
import android.content.Context
import android.database.sqlite.SQLiteDatabase
import android.database.sqlite.SQLiteOpenHelper
import android.net.Uri
import android.os.Build
import android.provider.DocumentsContract
import android.provider.MediaStore

/** Additive aliases for files replaced by our sync engine; existing playlist/stat keys stay intact. */
object SyncIdentity {
    @Volatile var revision=0L;private set
    data class Identity(val id:Long,val added:Long,val version:Long=0)
    private class Store(context:Context):SQLiteOpenHelper(context,"sync-identities.db",null,2) {
        override fun onCreate(db:SQLiteDatabase){db.execSQL("CREATE TABLE files (document TEXT PRIMARY KEY, uri TEXT NOT NULL, stable INTEGER NOT NULL, added INTEGER NOT NULL, version INTEGER NOT NULL DEFAULT 0)")}
        override fun onUpgrade(db:SQLiteDatabase,oldVersion:Int,newVersion:Int){if(oldVersion<2)db.execSQL("ALTER TABLE files ADD COLUMN version INTEGER NOT NULL DEFAULT 0")}
    }
    @Volatile private var helper:Store?=null
    private fun store(c:Context)=helper ?: synchronized(this){helper ?: Store(c.applicationContext).also{helper=it}}
    private fun key(uri:Uri)="${uri.authority}:${DocumentsContract.getDocumentId(uri)}"
    private fun media(c:Context,document:Uri):Identity? {
        if(Build.VERSION.SDK_INT<29)return null
        val uri=MediaStore.getMediaUri(c,document) ?: return null
        val audio=ContentUris.withAppendedId(MediaStore.Audio.Media.EXTERNAL_CONTENT_URI,ContentUris.parseId(uri))
        return c.contentResolver.query(audio,arrayOf("_id","date_added"),null,null,null)?.use{
            if(!it.moveToFirst())null else Identity(it.getLong(0),it.getLong(1))
        }
    }
    fun capture(c:Context,document:Uri) {
        check(Build.VERSION.SDK_INT>=29){"La sincronizzazione richiede Android 10 o successivo"}
        val actual=try{media(c,document)}catch(_:Exception){null}
        val db=store(c).writableDatabase;val documentKey=key(document)
        val previous=db.query("files",arrayOf("stable","added","version"),"document=?",arrayOf(documentKey),null,null,null).use{if(it.moveToFirst())Identity(it.getLong(0),it.getLong(1),it.getLong(2))else null}
        val song=Catalog.get(c).songs.find{runCatching{ContentUris.parseId(it.uri)==actual?.id}.getOrDefault(false)}
        val stable=previous ?: song?.let{Identity(it.id.substringAfter(':').toLong(),it.added)} ?: actual ?: return
        db.insertWithOnConflict("files",null,ContentValues().apply{put("document",documentKey);put("uri",document.toString());put("stable",stable.id);put("added",stable.added);put("version",stable.version+1)},SQLiteDatabase.CONFLICT_REPLACE).also{check(it!=-1L)}
        revision++
    }
    fun aliases(c:Context):Map<Long,Identity> {
        if(Build.VERSION.SDK_INT<29)return emptyMap()
        val rows=mutableListOf<Pair<Uri,Identity>>()
        store(c).readableDatabase.query("files",arrayOf("uri","stable","added","version"),null,null,null,null,null).use{while(it.moveToNext())rows+=Uri.parse(it.getString(0)) to Identity(it.getLong(1),it.getLong(2),it.getLong(3))}
        val result=mutableMapOf<Long,Identity>()
        for((document,stable)in rows){val actual=try{media(c,document)}catch(_:Exception){null};if(actual!=null)result[actual.id]=stable}
        return result
    }
}
