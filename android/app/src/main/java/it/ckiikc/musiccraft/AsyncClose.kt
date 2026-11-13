package it.ckiikc.musiccraft

import java.io.Closeable
import java.util.concurrent.Executors

/** TLS close may send close_notify or wait on an in-flight write: never do it on the UI thread. */
internal class AsyncClose:Closeable {
    private val worker=Executors.newSingleThreadExecutor {r->Thread(r,"MusicCraft TLS cleanup").apply{isDaemon=true}}
    private val pending=linkedSetOf<Closeable>()
    private var scheduled=false
    private var closed=false

    @Synchronized fun release(resources:Collection<Closeable>) {
        check(!closed)
        pending.addAll(resources)
        // Coalesce batches; at most one drain task is queued or running.
        if(pending.isNotEmpty() && !scheduled) {
            scheduled=true
            worker.execute {drain()}
        }
    }
    private fun drain() {
        while(true) {
            val batch=synchronized(this) {
                if(pending.isEmpty()){scheduled=false;return}
                pending.toList().also{pending.clear()}
            }
            for(resource in batch)try{resource.close()}catch(_:Exception){}
        }
    }
    @Synchronized override fun close() {
        if(closed)return
        closed=true
        // Drain resources already handed over, without waiting on the caller/UI thread.
        worker.shutdown()
    }
}
