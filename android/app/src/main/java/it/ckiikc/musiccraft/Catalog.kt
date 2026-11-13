package it.ckiikc.musiccraft

import android.Manifest
import android.content.ContentUris
import android.content.Context
import android.content.pm.PackageManager
import android.os.Build
import android.provider.MediaStore
import android.util.Base64
import androidx.media3.common.MediaItem
import androidx.media3.common.MediaMetadata
import com.google.common.util.concurrent.ListenableFuture
import com.google.common.util.concurrent.SettableFuture
import java.util.concurrent.Executors

object LibraryRules {
    fun pageBounds(size: Int, page: Int, pageSize: Int): IntRange? {
        if (page < 0 || pageSize < 1) return null
        val from = page.toLong() * pageSize.toLong()
        if (from >= size) return IntRange.EMPTY
        return from.toInt() until minOf(size.toLong(), from + pageSize).toInt()
    }
    fun matches(title: String, artist: String, album: String, query: String): Boolean =
        query.isBlank() || listOf(title, artist, album).any { it.contains(query.trim(), ignoreCase = true) }
}

data class Song(val id: String, val title: String, val artist: String, val album: String, val albumKey: String, val duration: Long, val uri: android.net.Uri, val artwork: android.net.Uri?, val trackNumber: Int, val displayName:String = "", val size:Long = 0, val modified:Long = 0,val year:Int=0,val genre:String="",val added:Long=0,val syncVersion:Long=0) {
    val coverKey:String by lazy { ArtworkKeys.file(id,added,size,modified,artist,album,title)+":"+syncVersion }
    val statKey:String get()="$id@$added"
    fun facts()=TrackFacts(id,title,artist,albumKey,genre,year,trackNumber,statKey,album)
    fun item(): MediaItem = MediaItem.Builder().setMediaId(id).setUri(uri).setMediaMetadata(
        MediaMetadata.Builder().setTitle(title).setArtist(artist).setAlbumTitle(album).setArtworkUri(ArtworkLibrary.uri(this))
            .setIsBrowsable(false).setIsPlayable(true).setMediaType(MediaMetadata.MEDIA_TYPE_MUSIC).build()
    ).build()
}

class Catalog private constructor(private val context: Context) {
    @Volatile var songs: List<Song> = emptyList(); private set
    @Volatile var lastError: String? = null; private set
    @Volatile var revision:Long=0;private set
    private val worker = Executors.newSingleThreadExecutor()
    private var loading: ListenableFuture<List<Song>>? = null
    private var loadedAt = 0L
    private var hadPermission = false
    fun permitted(): Boolean = context.checkSelfPermission(if (Build.VERSION.SDK_INT >= 33) Manifest.permission.READ_MEDIA_AUDIO else Manifest.permission.READ_EXTERNAL_STORAGE) == PackageManager.PERMISSION_GRANTED
    @Synchronized fun refresh(force: Boolean = false): ListenableFuture<List<Song>> {
        val allowed = permitted()
        if (!force && allowed == hadPermission && android.os.SystemClock.elapsedRealtime() - loadedAt < 30_000) return com.google.common.util.concurrent.Futures.immediateFuture(songs)
        loading?.takeIf { !it.isDone }?.let { return it }
        val result = SettableFuture.create<List<Song>>()
        loading = result
        worker.execute {
            try {
                songs = if (permitted()) readSongs() else emptyList()
                revision++
                lastError = null
                hadPermission = allowed
                loadedAt = android.os.SystemClock.elapsedRealtime()
                result.set(songs)
                RatingTags.index(context,songs)
            } catch (e: Exception) {
                songs = emptyList()
                lastError = if (e is SecurityException) "Autorizza l’accesso alla musica sul telefono." else "Lettura della libreria non riuscita. Riprova."
                result.setException(e)
            }
        }
        return result
    }
    private fun readSongs(): List<Song> {
        val out = ArrayList<Song>()
        // MediaStore includes indexed audio on shared primary and removable storage.
        val collection = if (Build.VERSION.SDK_INT >= 29) MediaStore.Audio.Media.getContentUri(MediaStore.VOLUME_EXTERNAL) else MediaStore.Audio.Media.EXTERNAL_CONTENT_URI
        val projection = arrayOf("_id", "title", "artist", "album", "album_id", "duration", "track", "_display_name", "_size", "date_modified", "year", if(Build.VERSION.SDK_INT>=30) "genre" else "year", "date_added")
        (context.contentResolver.query(collection, projection, "is_music != 0", null, "title COLLATE NOCASE ASC, _id ASC") ?: throw java.io.IOException("MediaStore unavailable")).use { cursor ->
            while (cursor.moveToNext()) {
                val id = cursor.getLong(0)
                val albumId = cursor.getLong(4)
                fun text(column: Int, fallback: String) = cursor.getString(column)?.takeUnless { it.isBlank() || it == "<unknown>" } ?: fallback
                out += Song("song:$id", text(1, "Senza titolo"), text(2, "Artista sconosciuto"), text(3, "Album sconosciuto"), "album:$albumId", cursor.getLong(5), ContentUris.withAppendedId(collection, id), if (albumId > 0) ContentUris.withAppendedId(android.net.Uri.parse("content://media/external/audio/albumart"), albumId) else null, cursor.getInt(6), cursor.getString(7).orEmpty(), cursor.getLong(8), cursor.getLong(9),cursor.getInt(10),if(Build.VERSION.SDK_INT>=30) cursor.getString(11).orEmpty() else "",cursor.getLong(12))
            }
        }
        val aliases=SyncIdentity.aliases(context);val reserved=aliases.values.map{it.id}.toSet()
        return out.map{song->val actual=song.id.substringAfter(':').toLong();val stable=aliases[actual];if(stable!=null)song.copy(id="song:${stable.id}",added=stable.added,syncVersion=stable.version)else if(actual in reserved)song.copy(id="song:${Long.MAX_VALUE-actual}")else song}
    }
    fun folder(id: String, title: String, type: Int): MediaItem {
        val icon=when(id){"now-playing"->"ic_player";"dynamic"->"ic_shuffle";"playlists","playlist-menu"->"ic_playlist";"cars"->"ic_car";"artists"->"ic_artist";"songs"->"ic_player";else->"ic_album"}
        return MediaItem.Builder().setMediaId(id).setMediaMetadata(MediaMetadata.Builder().setTitle(title).setArtworkUri(android.net.Uri.parse("android.resource://${context.packageName}/drawable/$icon")).setIsBrowsable(true).setIsPlayable(false).setMediaType(type).build()).build()
    }
    fun artistId(artist: String) = "artist:" + Base64.encodeToString(artist.toByteArray(Charsets.UTF_8), Base64.URL_SAFE or Base64.NO_WRAP)
    fun children(parent: String): List<MediaItem> = when (parent) {
        "root" -> listOf(folder("songs", "Brani", MediaMetadata.MEDIA_TYPE_FOLDER_MIXED), folder("albums", "Album", MediaMetadata.MEDIA_TYPE_FOLDER_ALBUMS), folder("artists", "Artisti", MediaMetadata.MEDIA_TYPE_FOLDER_ARTISTS),folder("dynamic","Playlist dinamiche",MediaMetadata.MEDIA_TYPE_FOLDER_PLAYLISTS),folder("playlists","Playlist salvate",MediaMetadata.MEDIA_TYPE_FOLDER_PLAYLISTS))
        "dynamic" -> ListeningStore.get(context).rules.map{MediaItem.Builder().setMediaId("mix:"+it.id).setMediaMetadata(MediaMetadata.Builder().setTitle(it.name).setIsBrowsable(false).setIsPlayable(true).setMediaType(MediaMetadata.MEDIA_TYPE_PLAYLIST).build()).build()}
        "playlists" -> ListeningStore.get(context).playlists.map{MediaItem.Builder().setMediaId("playlist:"+it.id).setMediaMetadata(MediaMetadata.Builder().setTitle(it.name).setIsBrowsable(false).setIsPlayable(true).setMediaType(MediaMetadata.MEDIA_TYPE_PLAYLIST).build()).build()}
        "songs" -> songs.map { it.item() }
        "albums" -> songs.distinctBy { it.albumKey }.sortedBy { it.album.lowercase() }.map { folder(it.albumKey, it.album, MediaMetadata.MEDIA_TYPE_ALBUM) }
        "artists" -> songs.map { it.artist }.distinct().sortedBy { it.lowercase() }.map { folder(artistId(it), it, MediaMetadata.MEDIA_TYPE_ARTIST) }
        else -> songs.filter { it.albumKey == parent || artistId(it.artist) == parent }.sortedWith(compareBy<Song> { it.album }.thenBy { it.trackNumber }.thenBy { it.title }).map { it.item() }
    }
    fun item(id: String): MediaItem? = songs.firstOrNull { it.id == id }?.item() ?: when (id) {
        "root" -> folder("root", "MusicCraft", MediaMetadata.MEDIA_TYPE_FOLDER_MIXED)
        "songs", "albums", "artists", "dynamic", "playlists" -> children("root").find { it.mediaId == id }
        else -> (children("albums") + children("artists") + children("dynamic") + children("playlists")).find { it.mediaId == id }
    }
    companion object {
        @Volatile private var instance: Catalog? = null
        fun get(context: Context): Catalog = instance ?: synchronized(this) { instance ?: Catalog(context.applicationContext).also { instance = it } }
    }
}
