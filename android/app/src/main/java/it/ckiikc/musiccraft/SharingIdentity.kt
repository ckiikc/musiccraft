package it.ckiikc.musiccraft

import android.content.Context
import android.net.Uri
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import java.math.BigInteger
import java.security.*
import java.security.spec.ECGenParameterSpec
import java.util.Date
import java.util.UUID
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.spec.GCMParameterSpec
import javax.net.ssl.*
import javax.security.auth.x500.X500Principal

/** Per-installation identity. No credentials or trusted device are shipped in the APK. */
class SharingIdentity(context: Context) {
    val id: String
    val token: String
    val pin: String
    val tls: SSLContext
    init {
        val prefs=context.getSharedPreferences("sharing_identity",Context.MODE_PRIVATE)
        val store=KeyStore.getInstance("AndroidKeyStore").apply {load(null)}
        val random=SecureRandom()
        if(!store.containsAlias("MusicCraftSharingTLSv1")) {
            KeyPairGenerator.getInstance(KeyProperties.KEY_ALGORITHM_EC,"AndroidKeyStore").apply {
                initialize(KeyGenParameterSpec.Builder("MusicCraftSharingTLSv1",KeyProperties.PURPOSE_SIGN or KeyProperties.PURPOSE_VERIFY)
                    .setAlgorithmParameterSpec(ECGenParameterSpec("secp256r1")).setDigests(KeyProperties.DIGEST_NONE,KeyProperties.DIGEST_SHA256,KeyProperties.DIGEST_SHA384,KeyProperties.DIGEST_SHA512)
                    .setCertificateSubject(X500Principal("CN=MusicCraft"))
                    .setCertificateSerialNumber(BigInteger(120,random).add(BigInteger.ONE))
                    .setCertificateNotBefore(Date(System.currentTimeMillis()-86400000))
                    .setCertificateNotAfter(Date(System.currentTimeMillis()+10L*365*86400000)).build())
                generateKeyPair()
            }
        }
        if(!store.containsAlias("MusicCraftSharingSecret")) KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES,"AndroidKeyStore").apply {
            init(KeyGenParameterSpec.Builder("MusicCraftSharingSecret",KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT).setBlockModes(KeyProperties.BLOCK_MODE_GCM).setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE).build());generateKey()
        }
        val key=store.getKey("MusicCraftSharingSecret",null)
        id=prefs.getString("id",null) ?: UUID.randomUUID().toString().also {prefs.edit().putString("id",it).apply()}
        val encrypted=prefs.getString("token",null)
        token=if(encrypted!=null) {
            val bytes=Base64.decode(encrypted,Base64.NO_WRAP)
            Cipher.getInstance("AES/GCM/NoPadding").run {init(Cipher.DECRYPT_MODE,key,GCMParameterSpec(128,bytes.copyOfRange(0,12)));String(doFinal(bytes.copyOfRange(12,bytes.size)),Charsets.UTF_8)}
        } else {
            val fresh=Base64.encodeToString(ByteArray(32).also(random::nextBytes),Base64.URL_SAFE or Base64.NO_WRAP or Base64.NO_PADDING)
            val c=Cipher.getInstance("AES/GCM/NoPadding").apply {init(Cipher.ENCRYPT_MODE,key)}
            prefs.edit().putString("token",Base64.encodeToString(c.iv+c.doFinal(fresh.toByteArray()),Base64.NO_WRAP)).apply();fresh
        }
        pin=Base64.encodeToString(MessageDigest.getInstance("SHA-256").digest(store.getCertificate("MusicCraftSharingTLSv1").publicKey.encoded),Base64.NO_WRAP)
        val alias="MusicCraftSharingTLSv1"
        val manager=object:X509ExtendedKeyManager() {
            override fun chooseServerAlias(type:String?,issuers:Array<java.security.Principal>?,socket:java.net.Socket?):String? {return if(type=="EC") alias else null}
            override fun chooseEngineServerAlias(type:String?,issuers:Array<java.security.Principal>?,engine:SSLEngine?)=if(type=="EC") alias else null
            override fun getServerAliases(type:String?,issuers:Array<java.security.Principal>?)=if(type=="EC") arrayOf(alias) else null
            override fun getCertificateChain(name:String?)=if(name==alias) store.getCertificateChain(alias).map{it as java.security.cert.X509Certificate}.toTypedArray() else null
            override fun getPrivateKey(name:String?)=if(name==alias) store.getKey(alias,null) as PrivateKey else null
            override fun getClientAliases(type:String?,issuers:Array<java.security.Principal>?)=null
            override fun chooseClientAlias(types:Array<String>?,issuers:Array<java.security.Principal>?,socket:java.net.Socket?)=null
        }
        tls=SSLContext.getInstance("TLS").apply {init(arrayOf(manager),null,random)}
    }
    fun pairing(host:String,port:Int):String=Uri.Builder().scheme("musiccraft").authority("pair").appendQueryParameter("v","1").appendQueryParameter("host",host).appendQueryParameter("port",port.toString()).appendQueryParameter("id",id).appendQueryParameter("pin",pin).appendQueryParameter("token",token).build().toString()
}

object SharingGate {
    @Volatile var localPlayback=false
    @Volatile var running=false
    @Volatile var state="Condivisione disattivata"
    @Volatile var pairLink:String?=null
    fun busy(context:Context):Boolean {
        val audio=context.getSystemService(android.media.AudioManager::class.java)
        return localPlayback || audio.isMusicActive || audio.mode!=android.media.AudioManager.MODE_NORMAL
    }
}

object ShareRange {
    /** null header means full file; invalid/multipart/out-of-bounds requests fail closed. */
    fun parse(value:String?,size:Long):LongRange? {
        if(size<=0) return null
        if(value==null) return 0 until size
        val match=Regex("bytes=(\\d+)-(\\d*)").matchEntire(value) ?: return null
        val start=match.groupValues[1].toLongOrNull() ?: return null
        val end=if(match.groupValues[2].isEmpty()) size-1 else match.groupValues[2].toLongOrNull() ?: return null
        return if(start>=0 && start<size && end>=start) start..minOf(end,size-1) else null
    }
}
