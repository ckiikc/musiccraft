package it.ckiikc.musiccraft
import org.junit.Assert.*
import org.junit.Test
class LibraryRulesTest {
    @Test fun paginationRejectsInvalidAndOverflowingRequests() {
        assertNull(LibraryRules.pageBounds(10,-1,2));assertNull(LibraryRules.pageBounds(10,0,0))
        assertEquals(listOf(4,5,6,7), LibraryRules.pageBounds(10,1,4)!!.toList())
        assertTrue(LibraryRules.pageBounds(10,Int.MAX_VALUE,Int.MAX_VALUE)!!.isEmpty())
        assertEquals(listOf(8,9),LibraryRules.pageBounds(10,2,4)!!.toList())
    }
    @Test fun searchCoversTitleArtistAndAlbum() {
        assertTrue(LibraryRules.matches("Fixture Song","Fixture Artist","Fixture Album"," aRTiST "))
        assertTrue(LibraryRules.matches("Fixture Song","Fixture Artist","Fixture Album","album"))
        assertFalse(LibraryRules.matches("Fixture Song","Fixture Artist","Fixture Album","unrelated"))
    }
}
