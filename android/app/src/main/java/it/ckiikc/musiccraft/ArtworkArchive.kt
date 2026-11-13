package it.ckiikc.musiccraft

import android.content.ContentValues
import android.database.sqlite.SQLiteDatabase
import java.io.File

/** Permanent app files, deliberately outside cacheDir. Used only on artwork workers. */
class ArtworkArchive(private val directory:File,private val budget:Long=256L*1024*1024):AutoCloseable {
    private val db:SQLiteDatabase
    init{directory.mkdirs();db=SQLiteDatabase.openOrCreateDatabase(File(directory,"index.db"),null);db.execSQL("CREATE TABLE IF NOT EXISTS art (key TEXT PRIMARY KEY, blob TEXT, retry INTEGER NOT NULL, source TEXT NOT NULL)")}
    data class Entry(val file:File?,val retry:Long,val source:String)
    @Synchronized fun entry(key:String):Entry?=db.rawQuery("SELECT blob,retry,source FROM art WHERE key=?",arrayOf(key)).use{c->if(!c.moveToFirst())null else Entry(c.getString(0)?.let{File(directory,"$it.jpg")}.takeIf{it?.isFile==true},c.getLong(1),c.getString(2))}
    @Synchronized fun put(key:String,bytes:ByteArray?,retry:Long=0,source:String="local"):File? {
        val hash=bytes?.let{ArtworkKeys.hash(it)};val file=hash?.let{File(directory,"$it.jpg")}
        if(file!=null && !file.exists()){
            val content=checkNotNull(bytes)
            if(content.size>remaining())return null
            val temp=File.createTempFile("cover-",".tmp",directory)
            try{temp.outputStream().use{it.write(content)};check(temp.renameTo(file))}finally{temp.delete()}
        }
        db.insertWithOnConflict("art",null,ContentValues().apply{put("key",key);put("blob",hash);put("retry",retry);put("source",source)},SQLiteDatabase.CONFLICT_REPLACE)
        return file
    }
    @Synchronized fun link(key:String,existing:Entry){val file=existing.file ?: return;db.insertWithOnConflict("art",null,ContentValues().apply{put("key",key);put("blob",file.nameWithoutExtension);put("retry",0);put("source",existing.source)},SQLiteDatabase.CONFLICT_REPLACE)}
    @Synchronized fun remaining():Long=budget-(directory.listFiles()?.filter{it.extension=="jpg"}?.sumOf{it.length()} ?: 0)
    @Synchronized fun summary():String {val files=directory.listFiles()?.filter{it.extension=="jpg"}.orEmpty();return "${files.size} immagini · ${files.sumOf{it.length()}/1024/1024} MB / ${budget/1024/1024} MB"}
    @Synchronized override fun close(){db.close()}
}
