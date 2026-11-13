package it.ckiikc.musiccraft

import org.junit.Assert.*
import org.junit.Test

class ProfileCleanupTest {
    @Test fun synchronousDisconnectDuringReleaseDoesNotSkipOtherProfiles() {
        val profiles=linkedMapOf(1 to "headset",2 to "a2dp")
        val released=mutableListOf<String>()
        closeProfileProxies(profiles) { id,proxy ->
            released+=proxy
            // BluetoothAdapter.closeProfileProxy can call ServiceListener inline.
            profiles.remove(id)
        }
        assertEquals(listOf("headset","a2dp"),released)
        assertTrue(profiles.isEmpty())
    }

    @Test fun nestedShutdownReleasesEachProxyOnce() {
        val profiles=linkedMapOf(1 to "headset",2 to "a2dp")
        val released=mutableListOf<String>()
        closeProfileProxies(profiles) { _,proxy ->
            released+=proxy
            assertTrue("Resources must be detached before callbacks",profiles.isEmpty())
            closeProfileProxies(profiles) { _,_ -> error("Proxy released twice") }
        }
        closeProfileProxies(profiles) { _,_ -> error("Proxy released twice") }
        assertEquals(listOf("headset","a2dp"),released)
    }
}
