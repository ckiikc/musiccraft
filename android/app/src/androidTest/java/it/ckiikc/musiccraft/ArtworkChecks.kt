package it.ckiikc.musiccraft

import android.content.Context
import android.graphics.Bitmap
import android.graphics.Color
import android.net.Uri
import java.io.ByteArrayOutputStream
import java.io.File
import java.util.concurrent.TimeUnit

/** Synthetic images only, isolated from the user's archive and always emulator guarded. */
class ArtworkChecks(private val context:Context,private val fixtureContext:Context?=null) {
    fun online(){
        check(android.os.Build.HARDWARE.contains("ranchu") || android.os.Build.MODEL.contains("sdk"))
        val dir=File(context.filesDir,"qa-online-artwork");dir.deleteRecursively()
        val song=Song("qa:public-network","So What","Miles Davis","Kind of Blue","album:qa",1000,Uri.parse("content://missing/audio"),null,1)
        try{
            ArtworkLibrary(context,dir,enabledOverride={true}).use{library->check(library.resolve(song)?.isFile==true){"Public cover retrieval unavailable"}}
            ArtworkLibrary(context,dir,{error("Unexpected second network request")},{true}).use{library->check(library.resolve(song)?.isFile==true)}
        }finally{dir.deleteRecursively()}
    }
    private fun fakeSong()=Song("qa:migration","One","QA Artist","QA Album","album:qa",3000,Uri.parse("content://missing/audio"),null,1,added=1)
    fun run(instrumentation:android.app.Instrumentation?=null,external:Boolean=false){
        check(android.os.Build.HARDWARE.contains("ranchu") || android.os.Build.MODEL.contains("sdk"))
        val dir=File(context.filesDir,"qa-artwork-archive");dir.deleteRecursively()
        fun jpeg(color:Int):ByteArray {val b=Bitmap.createBitmap(48,48,Bitmap.Config.ARGB_8888);b.eraseColor(color);return try{ByteArrayOutputStream().use{b.compress(Bitmap.CompressFormat.JPEG,90,it);it.toByteArray()}}finally{b.recycle()}}
        fun withPicture(audio:ByteArray,image:ByteArray):ByteArray {
            check(String(audio,0,4,Charsets.US_ASCII)=="fLaC")
            val picture=ByteArrayOutputStream().also{bytes->java.io.DataOutputStream(bytes).use{out->
                out.writeInt(3);out.writeInt(10);out.writeBytes("image/jpeg");out.writeInt(0)
                out.writeInt(48);out.writeInt(48);out.writeInt(24);out.writeInt(0);out.writeInt(image.size);out.write(image)
            }}.toByteArray()
            val out=ByteArrayOutputStream();out.write(audio,0,4);var offset=4
            while(true){val header=audio[offset].toInt() and 255;val size=((audio[offset+1].toInt() and 255) shl 16) or ((audio[offset+2].toInt() and 255) shl 8) or (audio[offset+3].toInt() and 255)
                out.write(header and 127);out.write(audio,offset+1,size+3);offset+=size+4;if(header and 128!=0)break}
            out.write(0x86);out.write(picture.size ushr 16);out.write(picture.size ushr 8);out.write(picture.size);out.write(picture);out.write(audio,offset,audio.size-offset)
            return out.toByteArray()
        }
        val a=jpeg(Color.BLUE);val b=jpeg(Color.RED)
        val fixtures=mutableListOf<Uri>()
        try{
            for(name in listOf("QA Alpha.flac","QA Beta.flac")){
                val values=android.content.ContentValues().apply{put(android.provider.MediaStore.Audio.Media.DISPLAY_NAME,"artwork-${java.util.UUID.randomUUID()}-$name");put(android.provider.MediaStore.Audio.Media.TITLE,name.removeSuffix(".flac"));put(android.provider.MediaStore.Audio.Media.ALBUM,"MusicCraft QA");put(android.provider.MediaStore.Audio.Media.MIME_TYPE,"audio/flac");put(android.provider.MediaStore.Audio.Media.RELATIVE_PATH,"Music/MusicCraftArtworkQA");put(android.provider.MediaStore.Audio.Media.IS_PENDING,1)}
                val uri=context.contentResolver.insert(android.provider.MediaStore.Audio.Media.EXTERNAL_CONTENT_URI,values) ?: error("Fixture insert failed");fixtures+=uri
                context.contentResolver.openOutputStream(uri,"w")!!.use{output->checkNotNull(fixtureContext).assets.open(name).use{output.write(withPicture(it.readBytes(),if(fixtures.size==1)a else b))}}
                context.contentResolver.update(uri,android.content.ContentValues().apply{put(android.provider.MediaStore.Audio.Media.IS_PENDING,0)},null,null)
            }
            ArtworkArchive(dir).use{archive->
                val first=archive.put("track-a",a)!!;val same=archive.put("track-b",a)!!;val different=archive.put("track-c",b)!!
                check(first==same && first!=different);check(dir.listFiles()!!.count{it.extension=="jpg"}==2)
                archive.put("missing",null,123456,"not-found");archive.link("album-alias",archive.entry("track-a")!!)
            }
            // A process-independent index reopening and cache-directory clearing must preserve all art.
            File(context.cacheDir,"qa-cover-cache").apply{mkdirs();File(this,"volatile").writeText("temporary");deleteRecursively()}
            ArtworkArchive(dir).use{archive->check(archive.entry("track-a")!!.file!!.readBytes().contentEquals(a));check(archive.entry("track-c")!!.file!!.readBytes().contentEquals(b));check(archive.entry("missing")!!.retry==123456L);check(archive.entry("album-alias")!!.file==archive.entry("track-a")!!.file)}
            ArtworkArchive(File(dir,"budget"),a.size.toLong()).use{archive->check(archive.put("first",a)!=null);check(archive.put("same",a)!=null);check(archive.put("full",b)==null);check(archive.entry("first")!!.file!!.exists())}
            val migrationDir=File(dir,"migration")
            val migrated=fakeSong()
            ArtworkArchive(migrationDir).use{archive->archive.put(migrated.previousCoverKey,a);archive.put("old-negative",null,123456,"not-found");val negative=archive.migrate("old-negative","new-negative")!!;check(negative.file==null && negative.retry==123456L && negative.source=="not-found")}
            ArtworkLibrary(context,migrationDir,{error("Existing artwork was downloaded again")},{true}).use{library->check(library.resolve(migrated)!!.readBytes().contentEquals(a))}
            ArtworkArchive(migrationDir).use{archive->check(archive.entry(migrated.coverKey)!!.file==archive.entry(migrated.previousCoverKey)!!.file);check(migrationDir.listFiles()!!.count{it.extension=="jpg"}==1)}
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
            val song=catalog.songs.first{it.uri==fixtures[0]};val library=ArtworkLibrary.get(context);check(!library.online)
            val alpha=catalog.songs.single{it.uri==fixtures[0]};val beta=catalog.songs.single{it.uri==fixtures[1]}
            check(alpha.albumKey==beta.albumKey)
            val alphaArt=library.resolve(alpha)!!;val betaArt=library.resolve(beta)!!
            check(alphaArt!=betaArt){"Embedded covers from mixed tracks were grouped by album"}
            context.contentResolver.openInputStream(ArtworkLibrary.uri(alpha))!!.use{check(android.graphics.BitmapFactory.decodeStream(it)!=null)}
            check(Regex("[0-9a-f]{64}").matches(alpha.coverKey))
            if(instrumentation!=null)checkAutoProtocol(instrumentation,alpha)
            if(external)externalReader(alpha)
            // Once archived, the image can be read even if the source cannot be opened.
            check(library.resolve(alpha.copy(uri=Uri.parse("content://missing/audio")))==alphaArt)
            // Provider rejects mutation and traversal, and gracefully represents missing cover art.
            check(runCatching{context.contentResolver.openFileDescriptor(Uri.parse("content://${ArtworkLibrary.AUTHORITY}/cover/../bad"),"r")}.isFailure)
            check(runCatching{context.contentResolver.openFileDescriptor(ArtworkLibrary.uri(song),"rw")}.isFailure)
        }finally{fixtures.forEach{context.contentResolver.delete(it,null,null)};dir.deleteRecursively()}
    }
    private fun checkAutoProtocol(instrumentation:android.app.Instrumentation,song:Song){
        fun main(block:()->Unit)=instrumentation.runOnMainSync(block)
        val connected=java.util.concurrent.CountDownLatch(1)
        lateinit var browser:android.media.browse.MediaBrowser
        var controller:android.media.session.MediaController?=null
        main{browser=android.media.browse.MediaBrowser(context,android.content.ComponentName(context,PlaybackService::class.java),object:android.media.browse.MediaBrowser.ConnectionCallback(){override fun onConnected(){connected.countDown()}},null);browser.connect()}
        try{
            check(connected.await(15,TimeUnit.SECONDS)){"Android Auto browser connection timed out"}
            fun children(parent:String):List<android.media.browse.MediaBrowser.MediaItem>{
                val done=java.util.concurrent.CountDownLatch(1);val result=java.util.concurrent.atomic.AtomicReference<List<android.media.browse.MediaBrowser.MediaItem>>()
                main{browser.subscribe(parent,object:android.media.browse.MediaBrowser.SubscriptionCallback(){override fun onChildrenLoaded(parentId:String,children:MutableList<android.media.browse.MediaBrowser.MediaItem>){result.set(children);done.countDown()}})}
                check(done.await(15,TimeUnit.SECONDS)){"Android Auto $parent did not load"};main{browser.unsubscribe(parent)};return result.get()
            }
            val track=children("songs").single{it.mediaId==song.id};check(track.description.iconUri==ArtworkLibrary.uri(song))
            val album=children("albums").single{it.mediaId==song.albumKey};check(album.description.iconUri?.authority==ArtworkLibrary.AUTHORITY){"Album list uses a generic icon instead of cover art"}
            context.contentResolver.openInputStream(album.description.iconUri!!)!!.use{check(android.graphics.BitmapFactory.decodeStream(it)!=null)}
            main{controller=android.media.session.MediaController(context,browser.sessionToken);controller!!.transportControls.playFromMediaId(song.id,null)}
            val end=System.currentTimeMillis()+10000;var artwork:Uri?=null
            while(System.currentTimeMillis()<end){
                main{val metadata=controller!!.metadata;if(metadata?.getString(android.media.MediaMetadata.METADATA_KEY_MEDIA_ID)==song.id){artwork=listOf(android.media.MediaMetadata.METADATA_KEY_DISPLAY_ICON_URI,android.media.MediaMetadata.METADATA_KEY_ART_URI,android.media.MediaMetadata.METADATA_KEY_ALBUM_ART_URI).firstNotNullOfOrNull{metadata.getString(it)?.let(Uri::parse)}}}
                if(artwork!=null)break;Thread.sleep(50)
            }
            check(artwork==ArtworkLibrary.uri(song)){"Playing-item metadata has no readable artwork URI"}
            context.contentResolver.openInputStream(artwork!!)!!.use{check(android.graphics.BitmapFactory.decodeStream(it)!=null)}
        }finally{main{controller?.transportControls?.pause();browser.disconnect();context.stopService(android.content.Intent(context,PlaybackService::class.java))}}
    }
    /** Handshake for a real different-UID reader; only synthetic media in the guarded emulator. */
    private fun externalReader(song:Song){
        val marker=File(context.filesDir,"qa-artwork-uri");val command=File(context.filesDir,"qa-artwork-command")
        val uri=ArtworkLibrary.uri(song);val prefix=Uri.parse("content://${ArtworkLibrary.AUTHORITY}/cover")
        command.delete();marker.writeText(uri.toString())
        try{
            val end=System.currentTimeMillis()+60000
            while(System.currentTimeMillis()<end && (!command.exists() || command.readText()!="grant"))Thread.sleep(50)
            check(command.exists() && command.readText()=="grant"){"External artwork reader did not request the test grant"}
            context.grantUriPermission("com.android.shell",prefix,android.content.Intent.FLAG_GRANT_READ_URI_PERMISSION or android.content.Intent.FLAG_GRANT_PREFIX_URI_PERMISSION)
            marker.writeText("granted\n$uri")
            while(System.currentTimeMillis()<end && command.readText()!="done")Thread.sleep(50)
            check(command.readText()=="done"){"External artwork verification did not complete"}
        }finally{context.revokeUriPermission("com.android.shell",prefix,android.content.Intent.FLAG_GRANT_READ_URI_PERMISSION);marker.delete();command.delete()}
    }

}
