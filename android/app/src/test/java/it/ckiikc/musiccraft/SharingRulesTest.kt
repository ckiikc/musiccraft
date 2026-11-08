package it.ckiikc.musiccraft
import org.junit.Assert.*
import org.junit.Test
class SharingRulesTest {
    @Test fun rangesAreBoundedAndSingle() {
        assertEquals(0L..9L,ShareRange.parse(null,10))
        assertEquals(2L..9L,ShareRange.parse("bytes=2-",10))
        assertEquals(2L..9L,ShareRange.parse("bytes=2-999",10))
        assertEquals(2L..4L,ShareRange.parse("bytes=2-4",10))
        for(value in listOf("bytes=-3","bytes=10-","bytes=2-1","bytes=1-2,4-5","bytes=999999999999999999999999-","items=0-1")) assertNull(ShareRange.parse(value,10))
        assertNull(ShareRange.parse(null,0))
    }
}
