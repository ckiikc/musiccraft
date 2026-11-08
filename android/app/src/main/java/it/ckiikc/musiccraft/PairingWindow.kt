package it.ckiikc.musiccraft

import android.os.SystemClock
import android.util.Base64
import org.bouncycastle.crypto.agreement.srp.SRP6Server
import org.bouncycastle.crypto.agreement.srp.SRP6VerifierGenerator
import org.bouncycastle.crypto.agreement.srp.SRP6StandardGroups
import org.bouncycastle.crypto.digests.SHA256Digest
import org.bouncycastle.util.BigIntegers
import org.json.JSONObject
import java.math.BigInteger
import java.security.MessageDigest
import java.security.SecureRandom
import java.util.UUID
import javax.crypto.Cipher
import javax.crypto.spec.GCMParameterSpec
import javax.crypto.spec.SecretKeySpec

/** A user-opened, short-lived PAKE window. Four digits never travel over the network. */
object PairingWindow {
    private val random=SecureRandom()
    private var code:String?=null
    private var expires=0L
    private var attempts=0
    private var session:Session?=null
    private data class Session(val id:String,val server:SRP6Server,val a:BigInteger,val b:BigInteger,var secret:BigInteger?=null,var approved:Boolean=false,var payload:JSONObject?=null)
    @Synchronized fun open():String {close();code=random.nextInt(10000).toString().padStart(4,'0');expires=SystemClock.elapsedRealtime()+120000;return code!!}
    @Synchronized fun close(){code=null;session=null;expires=0;attempts=0}
    @Synchronized fun visibleCode():String?=code.takeIf{SystemClock.elapsedRealtime()<expires && attempts<=3}
    @Synchronized fun pending()=visibleCode()!=null && session?.secret!=null && session?.payload==null
    @Synchronized fun completed()=session?.payload!=null
    @Synchronized fun approve(){if(pending()) session?.approved=true}
    private fun bytes(value:BigInteger)=BigIntegers.asUnsignedByteArray(384,value)
    private fun digest(vararg values:ByteArray)=MessageDigest.getInstance("SHA-256").run {for(value in values) update(value);digest()}
    private fun encode(value:ByteArray)=Base64.encodeToString(value,Base64.NO_WRAP)
    private fun decode(value:String,max:Int):ByteArray {require(value.length<=max*2);return Base64.decode(value,Base64.NO_WRAP).also{require(it.size<=max)}}
    @Synchronized fun start(body:JSONObject):JSONObject {
        val pin=visibleCode() ?: error("Pairing closed")
        check(attempts<3);attempts++;session=null
        val a=BigInteger(1,decode(body.getString("a"),384));val group=SRP6StandardGroups.rfc5054_3072
        require(a>BigInteger.ZERO && a<group.n)
        val salt=ByteArray(32).also(random::nextBytes)
        val generator=SRP6VerifierGenerator().apply{init(group,SHA256Digest())}
        val verifier=generator.generateVerifier(salt,"musiccraft-pair".toByteArray(),pin.toByteArray())
        val server=SRP6Server().apply{init(group,verifier,SHA256Digest(),random)}
        val b=server.generateServerCredentials();val id=UUID.randomUUID().toString()
        session=Session(id,server,a,b)
        return JSONObject().put("session",id).put("salt",encode(salt)).put("b",encode(bytes(b)))
    }
    /** null means the phone still needs explicit approval; credentials remain encrypted. */
    @Synchronized fun finish(body:JSONObject,link:String):JSONObject? {
        check(visibleCode()!=null);val current=session ?: error("No session")
        check(body.getString("session")==current.id)
        val proof=decode(body.getString("proof"),32);require(proof.size==32)
        val secret=current.secret ?: current.server.calculateSecret(current.a)
        val expected=digest(bytes(current.a),bytes(current.b),bytes(secret))
        if(!MessageDigest.isEqual(proof,expected)){session=null;throw SecurityException("Pairing refused")}
        current.secret=secret
        if(!current.approved) return null
        current.payload?.let{return it}
        val serverProof=digest(bytes(current.a),bytes(BigInteger(1,expected)),bytes(secret))
        val nonce=ByteArray(12).also(random::nextBytes)
        val cipher=Cipher.getInstance("AES/GCM/NoPadding").apply {init(Cipher.ENCRYPT_MODE,SecretKeySpec(digest(bytes(secret)),"AES"),GCMParameterSpec(128,nonce));updateAAD("musiccraft-pair-v1".toByteArray())}
        val payload=JSONObject().put("proof",encode(serverProof)).put("nonce",encode(nonce)).put("payload",encode(cipher.doFinal(link.toByteArray())))
        current.payload=payload;return payload
    }
}
