package it.ckiikc.musiccraft

import android.content.Context
import android.graphics.Bitmap
import android.graphics.Color
import android.net.Uri
import java.io.ByteArrayOutputStream
import java.io.File
import java.util.concurrent.TimeUnit

/** Synthetic images only, isolated from the user's archive and always emulator guarded. */
class ArtworkChecks(private val context:Context) {
    fun online(){
        check(android.os.Build.HARDWARE.contains("ranchu") || android.os.Build.MODEL.contains("sdk"))
        val dir=File(context.filesDir,"qa-online-artwork");dir.deleteRecursively()
        val song=Song("qa:public-network","So What","Miles Davis","Kind of Blue","album:qa",1000,Uri.parse("content://missing/audio"),null,1)
        try{
            ArtworkLibrary(context,dir,enabledOverride={true}).use{library->check(library.resolve(song)?.isFile==true){"Public cover retrieval unavailable"}}
            ArtworkLibrary(context,dir,{error("Unexpected second network request")},{true}).use{library->check(library.resolve(song)?.isFile==true)}
        }finally{dir.deleteRecursively()}
    }
    fun run(){
        check(android.os.Build.HARDWARE.contains("ranchu") || android.os.Build.MODEL.contains("sdk"))
        val dir=File(context.filesDir,"qa-artwork-archive");dir.deleteRecursively()
        fun jpeg(color:Int):ByteArray {val b=Bitmap.createBitmap(48,48,Bitmap.Config.ARGB_8888);b.eraseColor(color);return try{ByteArrayOutputStream().use{b.compress(Bitmap.CompressFormat.JPEG,90,it);it.toByteArray()}}finally{b.recycle()}}
        val a=jpeg(Color.BLUE);val b=jpeg(Color.RED)
        try{
            ArtworkArchive(dir).use{archive->
                val first=archive.put("track-a",a)!!;val same=archive.put("track-b",a)!!;val different=archive.put("track-c",b)!!
                check(first==same && first!=different);check(dir.listFiles()!!.count{it.extension=="jpg"}==2)
                archive.put("missing",null,123456,"not-found");archive.link("album-alias",archive.entry("track-a")!!)
            }
            // A process-independent index reopening and cache-directory clearing must preserve all art.
            File(context.cacheDir,"qa-cover-cache").apply{mkdirs();File(this,"volatile").writeText("temporary");deleteRecursively()}
            ArtworkArchive(dir).use{archive->check(archive.entry("track-a")!!.file!!.readBytes().contentEquals(a));check(archive.entry("track-c")!!.file!!.readBytes().contentEquals(b));check(archive.entry("missing")!!.retry==123456L);check(archive.entry("album-alias")!!.file==archive.entry("track-a")!!.file)}
            ArtworkArchive(File(dir,"budget"),a.size.toLong()).use{archive->check(archive.put("first",a)!=null);check(archive.put("same",a)!=null);check(archive.put("full",b)==null);check(archive.entry("first")!!.file!!.exists())}
            var downloads=0
            val fake=Song("qa:one","One","QA Artist","QA Album","album:qa",3000,Uri.parse("content://missing/audio"),null,1,added=1)
            val onlineDir=File(dir,"downloaded")
            ArtworkLibrary(context,onlineDir,{downloads++;ArtworkLibrary.normalize(a)},{true}).use{library->
                val first=library.resolve(fake)!!;check(first.exists());check(downloads==1)
                check(library.resolve(fake)==first && downloads==1)
                check(library.resolve(fake.copy(id="qa:two",title="Two"))==first && downloads==1)
            }
            ArtworkLibrary(context,onlineDir,{error("Unexpected redownload")},{true}).use{library->check(library.resolve(fake)!=null)}
            var misses=0
            ArtworkLibrary(context,File(dir,"negative"),{misses++;null},{true}).use{library->check(library.resolve(fake)==null);check(library.resolve(fake)==null);check(library.resolve(fake.copy(id="qa:three"))==null);check(misses==1)}
            // Loose files have independent query identities, even with the same artist and directory.
            var singles=0
            ArtworkLibrary(context,File(dir,"singles"),{song->singles++;if(song.title=="One")a else b},{true}).use{library->
                val one=library.resolve(fake.copy(album="Album sconosciuto"))!!
                val two=library.resolve(fake.copy(id="qa:four",album="Album sconosciuto",title="Two"))!!
                check(one!=two && singles==2)
            }
            check(ArtworkLibrary.normalize(byteArrayOf(1,2,3))==null)
            check(runCatching{ArtworkLibrary.bounded(java.io.ByteArrayInputStream(ByteArray(33)),32)}.isFailure)
            val catalog=Catalog.get(context);catalog.refresh(true).get(20,TimeUnit.SECONDS)
            val song=catalog.songs.first();val library=ArtworkLibrary.get(context);check(!library.online)
            val alpha=catalog.songs.single{it.title=="QA Alpha"};val beta=catalog.songs.single{it.title=="QA Beta"}
            check(alpha.albumKey==beta.albumKey)
            val alphaArt=library.resolve(alpha)!!;val betaArt=library.resolve(beta)!!
            check(alphaArt!=betaArt){"Embedded covers from mixed tracks were grouped by album"}
            context.contentResolver.openInputStream(ArtworkLibrary.uri(alpha))!!.use{check(android.graphics.BitmapFactory.decodeStream(it)!=null)}
            // Once archived, the image can be read even if the source cannot be opened.
            check(library.resolve(alpha.copy(uri=Uri.parse("content://missing/audio")))==alphaArt)
            // Provider rejects mutation and traversal, and gracefully represents missing cover art.
            check(runCatching{context.contentResolver.openFileDescriptor(Uri.parse("content://${ArtworkLibrary.AUTHORITY}/cover/../bad"),"r")}.isFailure)
            check(runCatching{context.contentResolver.openFileDescriptor(ArtworkLibrary.uri(song),"rw")}.isFailure)
        }finally{dir.deleteRecursively()}
    }
}
