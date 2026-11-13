package it.ckiikc.musiccraft
import android.content.Context
import android.database.sqlite.SQLiteDatabase
import android.database.sqlite.SQLiteOpenHelper
/** Raw values cached independently of manual ratings; never marks imported ratings as user overrides. */
class RatingIndex(c:Context):SQLiteOpenHelper(c,"rating-tags-v1.db",null,1){
    override fun onCreate(db:SQLiteDatabase){db.execSQL("CREATE TABLE ratings(id TEXT PRIMARY KEY,fingerprint TEXT NOT NULL,summary TEXT NOT NULL)")}
    override fun onUpgrade(db:SQLiteDatabase,oldVersion:Int,newVersion:Int){error("Unsupported rating cache version")}
}
