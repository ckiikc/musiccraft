package it.ckiikc.musiccraft

import java.security.MessageDigest
import java.text.Normalizer
import java.util.Locale

/** Identity never depends on a directory: loose tracks cannot overwrite one another. */
object ArtworkKeys {
    fun hash(value:String)=hash(value.toByteArray(Charsets.UTF_8))
    fun hash(value:ByteArray):String {val bytes=MessageDigest.getInstance("SHA-256").digest(value);val digits="0123456789abcdef";return String(CharArray(bytes.size*2){i->val b=bytes[i/2].toInt() and 255;digits[if(i%2==0)b ushr 4 else b and 15]})}
    fun normalized(value:String)=Normalizer.normalize(value,Normalizer.Form.NFKD).replace(Regex("\\p{M}"),"").lowercase(Locale.ROOT).replace(Regex("[^\\p{L}\\p{N}]+")," ").trim()
    fun known(value:String)=normalized(value).let{it.isNotBlank() && it !in setOf("unknown","unknown album","unknown artist","album sconosciuto","artista sconosciuto","sconosciuto","various artists","artisti vari")}
    fun file(id:String,added:Long,size:Long,modified:Long,artist:String,album:String,title:String)=hash(listOf(id,added,size,modified,artist,album,title).joinToString("\u0000"))
    fun query(artist:String,album:String,title:String):String? {
        if(!known(artist))return null
        return if(known(album))"album\u0000${normalized(artist)}\u0000${normalized(album)}" else if(known(title))"track\u0000${normalized(artist)}\u0000${normalized(title)}" else null
    }
    fun matches(expected:String,actual:String)=normalized(expected)==normalized(actual)
}
