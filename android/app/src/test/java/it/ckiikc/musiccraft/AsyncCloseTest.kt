package it.ckiikc.musiccraft

import java.io.Closeable
import java.io.IOException
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import org.junit.Assert.*
import org.junit.Test

class AsyncCloseTest {
    @Test fun aBlockedSocketDoesNotBlockCallerAndShutdownDrainsLaterResources() {
        val entered=CountDownLatch(1);val unblock=CountDownLatch(1);val done=CountDownLatch(1)
        val cleanup=AsyncClose();val caller=Thread.currentThread()
        try {
            cleanup.release(listOf(Closeable{entered.countDown();check(unblock.await(5,TimeUnit.SECONDS))}))
            assertTrue(entered.await(5,TimeUnit.SECONDS))
            cleanup.release(listOf(Closeable{assertNotSame(caller,Thread.currentThread());done.countDown()}))
            cleanup.close();cleanup.close()
            assertEquals(1L,done.count)
        }finally{unblock.countDown();cleanup.close()}
        assertTrue(done.await(5,TimeUnit.SECONDS))
    }
    @Test fun aFailedSocketCloseDoesNotSkipTheRest() {
        val done=CountDownLatch(1);val cleanup=AsyncClose()
        cleanup.release(listOf(Closeable{throw IOException("Synthetic socket failure")},Closeable{done.countDown()}))
        cleanup.close()
        assertTrue(done.await(5,TimeUnit.SECONDS))
    }
}
