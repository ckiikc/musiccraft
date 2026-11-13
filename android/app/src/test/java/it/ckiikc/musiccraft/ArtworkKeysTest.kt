package it.ckiikc.musiccraft

import org.junit.Assert.*
import org.junit.Test

class ArtworkKeysTest {
    @Test fun mixedFolderTracksRemainIndependent(){
        val a=ArtworkKeys.file("song:1",1,100,10,"A","Singles","One")
        val b=ArtworkKeys.file("song:2",1,100,10,"B","Singles","Two")
        assertNotEquals(a,b);assertEquals(64,a.length)
    }
    @Test fun syncVersionsStayValidForTheArtworkProviderAndReusePreSyncKeys(){
        val original=ArtworkKeys.file("song:1",1,100,10,"A","B","C")
        assertEquals(original,ArtworkKeys.versioned(original,0))
        val first=ArtworkKeys.versioned(original,1)
        val second=ArtworkKeys.versioned(original,2)
        assertTrue(Regex("[0-9a-f]{64}").matches(first))
        assertTrue(Regex("[0-9a-f]{64}").matches(second))
        assertNotEquals(original,first);assertNotEquals(first,second)
        assertEquals(first,ArtworkKeys.versioned(original,1))
    }
    @Test fun replacingFileInvalidatesAssociation(){assertNotEquals(ArtworkKeys.file("song:1",1,100,10,"A","B","C"),ArtworkKeys.file("song:1",1,100,11,"A","B","C"))}
    @Test fun knownAlbumsReuseQueriesWhileLooseTracksDoNot(){
        assertEquals(ArtworkKeys.query("Björk","Debut","One"),ArtworkKeys.query("Bjork","Debut","Two"))
        assertNotEquals(ArtworkKeys.query("Artist","Album sconosciuto","One"),ArtworkKeys.query("Artist","Album sconosciuto","Two"))
        assertNull(ArtworkKeys.query("Artista sconosciuto","Album","Song"))
    }
    @Test fun exactMatchingRejectsNamesakesAndEditions(){assertTrue(ArtworkKeys.matches("Björk","BJORK"));assertFalse(ArtworkKeys.matches("Debut","Debut Deluxe"));assertFalse(ArtworkKeys.matches("Artist","Artist Tribute"))}
}
