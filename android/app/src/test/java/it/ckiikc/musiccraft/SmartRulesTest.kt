package it.ckiikc.musiccraft
import org.junit.Assert.*
import org.junit.Test
import kotlin.random.Random
class SmartRulesTest {
    private val songs=listOf(TrackFacts("a2","Two","A","albumA","Rock",1992,2),TrackFacts("a1","One","A","albumA","Rock",1992,1),TrackFacts("b","Other","B","albumB","Jazz",1980,1),TrackFacts("c","Unknown","C","albumC","",0,1))
    private val stats=mapOf("a1" to TrackStats(5,3,20),"b" to TrackStats(2,1,40))
    @Test fun exactPlayCountBoundsCombineWithRatingsAndRefresh(){
        val r=SmartRule("test","Four stars, no more than two plays",minimumRating=4,maximumPlays=2)
        val initial=mapOf("a1" to TrackStats(rating=5,plays=2),"b" to TrackStats(rating=4,plays=0))
        assertEquals(setOf("a1","b"),SmartRules.select(songs,initial,r).map{it.id}.toSet())
        assertEquals(listOf("b"),SmartRules.select(songs,initial+("a1" to initial.getValue("a1").copy(plays=3)),r).map{it.id})
        assertEquals(listOf("a1"),SmartRules.select(songs,initial,r.copy(minimumPlays=2)).map{it.id})
        assertTrue(SmartRules.select(songs,initial,r.copy(maximumPlays=0,minimumPlays=1)).isEmpty())
    }
    @Test fun ratingUnknownIsDifferentFromExplicitZero(){
        val r=SmartRule("r","Unrated",maximumRating=0)
        assertTrue(SmartRules.select(songs,emptyMap(),r).isEmpty())
        assertEquals(listOf("a1"),SmartRules.select(songs,mapOf("a1" to TrackStats(ratingSet=true)),r).map{it.id})
    }
    @Test fun filtersAndUnknownYearsArePrecise(){
        val selected=SmartRules.select(songs,stats,SmartRule("test","Rock nineties",genre="rock",fromYear=1990,toYear=1999),Random(1));assertEquals(setOf("a1","a2"),selected.map{it.id}.toSet())
        assertFalse(SmartRules.select(songs,stats,SmartRule("test","Before",toYear=1999)).any{it.id=="c"})
        assertEquals(listOf("a1"),SmartRules.select(songs,stats,SmartRule("test","Rated",minimumRating=4)).map{it.id})
    }
    @Test fun albumGroupingKeepsTrackOrderAndPartialMeansMixedHistory(){
        val rule=SmartRule("test","Partial",unit=MixUnit.ALBUMS,played=PlayedFilter.PARTLY_PLAYED)
        assertEquals(listOf("a1","a2"),SmartRules.select(songs,stats,rule).map{it.id})
        assertTrue(SmartRules.select(songs,stats,rule.copy(unit=MixUnit.SONGS)).isEmpty())
    }
    @Test fun playedAndUnplayedUseActualPlayCounts(){
        assertEquals(setOf("a2","c"),SmartRules.select(songs,stats,SmartRule("u","Unplayed",played=PlayedFilter.UNPLAYED)).map{it.id}.toSet())
        assertEquals(setOf("a1","b"),SmartRules.select(songs,stats,SmartRule("p","Played",played=PlayedFilter.PLAYED)).map{it.id}.toSet())
    }
    @Test fun rankingsDistinguishTotalAndAverage(){
        val many=(1..4).map{TrackFacts("x$it","X$it","X","X")};val one=TrackFacts("y","Y","Y","Y")
        val ratings=many.associate{it.id to TrackStats(2,1)}+("y" to TrackStats(5,3))
        val rule=SmartRule("r","Ranked",unit=MixUnit.ALBUMS,order=MixOrder.RATING_TOTAL)
        assertEquals("x1",SmartRules.select(many+one,ratings,rule).first().id)
        assertEquals("y",SmartRules.select(many+one,ratings,rule.copy(order=MixOrder.RATING_AVERAGE)).first().id)
        assertEquals("x1",SmartRules.select(many+one,ratings,rule.copy(order=MixOrder.MOST_PLAYED)).first().id)
        assertEquals("y",SmartRules.select(many+one,ratings,rule.copy(order=MixOrder.MOST_PLAYED_AVERAGE)).first().id)
    }
    @Test fun cursorDoesNotRepeatWithinACycleOrDuplicateUpcomingItems(){
        val cursor=MixCursor();val rule=SmartRule("r","Random");val first=cursor.take(songs,emptyMap(),rule,emptySet(),3)
        assertEquals(3,first.toSet().size)
        val rest=cursor.take(songs,emptyMap(),rule,first.toSet(),5);assertEquals(1,rest.size);assertTrue(rest.none{it in first})
        val next=cursor.take(songs,emptyMap(),rule,setOf(rest.single()),3);assertEquals(first.toSet(),next.toSet())
    }
    @Test fun unplayedMixStopsAndPendingIdsAreRevalidated(){
        val cursor=MixCursor();val r=SmartRule("r","New",played=PlayedFilter.UNPLAYED)
        val initial=cursor.take(songs,stats,r,emptySet(),1);assertEquals(1,initial.size)
        val allPlayed=songs.associate{it.id to TrackStats(plays=1)}
        assertTrue(cursor.take(songs,allPlayed,r,emptySet(),4).isEmpty())
    }
    @Test fun invalidRulesFailClosed(){assertFalse(SmartRule("x"," ").valid());assertFalse(SmartRule("x","Bad",fromYear=2000,toYear=1990).valid());assertFalse(SmartRule("x","Bad",minimumRating=5,maximumRating=3).valid())}
    @Test fun albumSearchUsesTheDisplayTitle(){val song=TrackFacts("x","Track","Artist","album:123",albumTitle="Unique Album");assertEquals(1,SmartRules.select(listOf(song),emptyMap(),SmartRule("r","Search",query="Unique")).size)}
}
