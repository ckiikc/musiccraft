package it.ckiikc.musiccraft

import org.junit.Assert.*
import org.junit.Test

class CarQueuesTest {
    private val cars=listOf(CarProfile("car:a","Auto A","a"),CarProfile("car:b","Auto B","b"))
    @Test fun onlyAssociatedUniqueCarsAreRecognized(){
        assertEquals("car:a",CarQueues.recognized(cars,setOf("a","headphones")))
        assertEquals("car:b",CarQueues.recognized(cars,setOf("b")))
        assertNull(CarQueues.recognized(cars,setOf("headphones")))
        assertNull(CarQueues.recognized(cars,setOf("a","b")))
        assertNull(CarQueues.recognized(cars,emptySet()))
    }
    @Test fun queueLinksAreScopedToProfileAndTimeline(){
        val id=CarQueues.queueEntry("car:a",7,2)
        assertEquals(2,CarQueues.queueIndex(id,"car:a",7,3))
        assertNull(CarQueues.queueIndex(id,"car:b",7,3))
        assertNull(CarQueues.queueIndex(id,"car:a",8,3))
        assertNull(CarQueues.queueIndex(id,"car:a",7,2))
        assertNull(CarQueues.queueIndex("2","car:a",7,3))
    }
    @Test fun snapshotsRetainPlaylistAndMixStateIndependently(){
        val phone=QueueSnapshot(listOf("song:1"),position=1234,name="Telefono")
        val a=QueueSnapshot(listOf("song:2"),position=4321,ruleId="dynamic",seen=setOf("song:3"),name="Rock",shuffle=true,repeat=2)
        val queues=mapOf(CarQueues.PHONE to phone,"car:a" to a)
        val changed=queues+("car:b" to QueueSnapshot(listOf("song:4"),name="Jazz"))
        assertEquals(phone,changed[CarQueues.PHONE]);assertEquals(a,changed["car:a"])
        assertEquals(4321L,changed["car:a"]!!.position)
        assertEquals(setOf("song:3"),changed["car:a"]!!.seen)
    }
    @Test fun automaticRoutingRequiresProjectionPermissionAndOptIn(){
        assertNull(CarQueues.route(false,true,true,cars,setOf("a")))
        assertEquals(CarQueues.DEFAULT_CAR,CarQueues.route(true,false,true,cars,setOf("a"))!!.profileId)
        assertEquals(CarQueues.DEFAULT_CAR,CarQueues.route(true,true,false,cars,setOf("a"))!!.profileId)
        assertEquals("car:a",CarQueues.route(true,true,true,cars,setOf("a"))!!.profileId)
        assertEquals(CarQueues.DEFAULT_CAR,CarQueues.route(true,true,true,cars,setOf("a","b"))!!.profileId)
        assertEquals(CarQueues.DEFAULT_CAR,CarQueues.route(true,true,true,cars,setOf("headphones"))!!.profileId)
    }

}
