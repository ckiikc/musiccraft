package it.ckiikc.musiccraft

import com.hierynomus.msdtyp.AccessMask
import com.hierynomus.msfscc.FileAttributes
import com.hierynomus.mssmb2.SMB2CreateDisposition
import com.hierynomus.mssmb2.SMB2CreateOptions
import com.hierynomus.mssmb2.SMB2Dialect
import com.hierynomus.mssmb2.SMB2ShareAccess
import com.hierynomus.mssmb2.SMBApiException
import com.hierynomus.mserref.NtStatus
import com.hierynomus.security.bc.BCSecurityProvider
import com.hierynomus.smbj.SMBClient
import com.hierynomus.smbj.SmbConfig
import com.hierynomus.smbj.auth.AuthenticationContext
import com.hierynomus.smbj.share.DiskShare
import java.io.FilterInputStream
import java.io.FilterOutputStream
import java.io.InputStream
import java.io.OutputStream
import java.util.EnumSet
import java.util.concurrent.TimeUnit

/** SMB2/3 only, mandatory signing, no global crypto-provider changes or stored password. */
class SmbSyncStorage(host:String,shareName:String,private val folder:String,user:String,password:CharArray,domain:String="",port:Int=445,override val fingerprintCache:java.io.File?=null):SyncStorage {
    private val client=SMBClient(SmbConfig.builder().withDialects(SMB2Dialect.SMB_2_1,SMB2Dialect.SMB_3_0,SMB2Dialect.SMB_3_0_2,SMB2Dialect.SMB_3_1_1).withSigningRequired(true).withSecurityProvider(BCSecurityProvider()).withTimeout(30,TimeUnit.SECONDS).withSoTimeout(35000).build())
    private val connection=client.connect(host,port)
    private val session=try{connection.authenticate(AuthenticationContext(user,password,domain))}catch(e:Exception){connection.close();client.close();throw e}
    private val share=try{session.connectShare(shareName) as DiskShare}catch(e:Exception){session.close();connection.close();client.close();throw e}
    init {if(folder.isNotEmpty())SyncFiles.checkPath(folder);share.list(path(""))}
    private fun path(relative:String):String {
        if(relative.isNotEmpty())SyncFiles.checkPath(relative)
        val result=listOf(folder,relative).filter{it.isNotEmpty()}.joinToString("/").replace('/','\\')
        // A reparse point can escape the chosen subtree; reject every existing ancestor.
        var current=""
        for(part in result.split('\\').filter{it.isNotEmpty()}){current=if(current.isEmpty())part else "$current\\$part";val info=try{share.getFileInformation(current)}catch(e:SMBApiException){if(e.statusCode in listOf(NtStatus.STATUS_NO_SUCH_FILE.value,NtStatus.STATUS_OBJECT_NAME_NOT_FOUND.value,NtStatus.STATUS_OBJECT_PATH_NOT_FOUND.value))null else throw e};if(info!=null)check(info.basicInformation.fileAttributes and FileAttributes.FILE_ATTRIBUTE_REPARSE_POINT.value==0L){"Collegamenti simbolici SMB non supportati"}}
        return result
    }
    override fun children(path:String):List<SyncNode> = share.list(path(path)).filter{it.fileName!="." && it.fileName!=".."}.map {
        check(it.fileAttributes and FileAttributes.FILE_ATTRIBUTE_REPARSE_POINT.value==0L){"Collegamenti simbolici SMB non supportati"}
        SyncNode(it.fileName,it.fileAttributes and FileAttributes.FILE_ATTRIBUTE_DIRECTORY.value!=0L,it.endOfFile,it.lastWriteTime.toEpochMillis())
    }
    override fun mkdir(path:String){share.mkdir(path(path))}
    override fun read(path:String):InputStream {
        val file=share.openFile(path(path),EnumSet.of(AccessMask.GENERIC_READ),null,EnumSet.of(SMB2ShareAccess.FILE_SHARE_READ),SMB2CreateDisposition.FILE_OPEN,EnumSet.of(SMB2CreateOptions.FILE_NON_DIRECTORY_FILE))
        return object:FilterInputStream(file.inputStream){override fun close(){try{super.close()}finally{file.close()}}}
    }
    override fun create(path:String):OutputStream {
        val file=share.openFile(path(path),EnumSet.of(AccessMask.GENERIC_WRITE),null,EnumSet.noneOf(SMB2ShareAccess::class.java),SMB2CreateDisposition.FILE_CREATE,EnumSet.of(SMB2CreateOptions.FILE_NON_DIRECTORY_FILE))
        return object:FilterOutputStream(file.outputStream){override fun write(bytes:ByteArray,offset:Int,length:Int){out.write(bytes,offset,length)};override fun close(){try{flush();file.flush();super.close()}finally{file.close()}}}
    }
    override fun move(from:String,to:String){val destination=path(to);share.openFile(path(from),EnumSet.of(AccessMask.DELETE,AccessMask.GENERIC_READ),null,EnumSet.noneOf(SMB2ShareAccess::class.java),SMB2CreateDisposition.FILE_OPEN,EnumSet.of(SMB2CreateOptions.FILE_NON_DIRECTORY_FILE)).use{it.rename(destination,false)}}
    override fun removeTemporary(path:String){require(path.startsWith(".musiccraft-sync/staging/"));share.rm(path(path))}
    override fun close(){try{share.close()}finally{try{session.close()}finally{try{connection.close()}finally{client.close()}}}}
}
