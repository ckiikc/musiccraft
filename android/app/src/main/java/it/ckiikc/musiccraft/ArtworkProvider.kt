package it.ckiikc.musiccraft

import android.content.*
import android.database.Cursor
import android.net.Uri
import android.os.ParcelFileDescriptor
import java.io.FileNotFoundException

/** Only validated cover IDs, read-only and restricted to explicitly granted media clients. */
class ArtworkProvider:ContentProvider() {
    override fun onCreate()=true
    override fun getType(uri:Uri)="image/jpeg"
    override fun openFile(uri:Uri,mode:String):ParcelFileDescriptor {
        if(mode!="r" || uri.authority!=ArtworkLibrary.AUTHORITY || uri.pathSegments.size!=2 || uri.pathSegments[0]!="cover" || !Regex("[0-9a-f]{64}").matches(uri.lastPathSegment.orEmpty()))throw FileNotFoundException()
        val ctx=context ?: throw FileNotFoundException()
        val song=Catalog.get(ctx).songs.firstOrNull{it.coverKey==uri.lastPathSegment} ?: throw FileNotFoundException()
        val file=ArtworkLibrary.get(ctx).resolve(song) ?: throw FileNotFoundException()
        return ParcelFileDescriptor.open(file,ParcelFileDescriptor.MODE_READ_ONLY)
    }
    override fun query(uri:Uri,projection:Array<out String>?,selection:String?,selectionArgs:Array<out String>?,sortOrder:String?):Cursor?=null
    override fun insert(uri:Uri,values:ContentValues?):Uri?=throw UnsupportedOperationException()
    override fun delete(uri:Uri,selection:String?,selectionArgs:Array<out String>?):Int=throw UnsupportedOperationException()
    override fun update(uri:Uri,values:ContentValues?,selection:String?,selectionArgs:Array<out String>?):Int=throw UnsupportedOperationException()
}
