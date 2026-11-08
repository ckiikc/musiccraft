package it.ckiikc.musiccraft

import kotlin.random.Random

/** Pure selection rules; browsing and text searches never mutate a playback queue. */
data class TrackFacts(val id:String,val title:String,val artist:String,val album:String,val genre:String="",val year:Int=0,val number:Int=0,val statKey:String=id,val albumTitle:String=album)
data class TrackStats(val rating:Int=0,val plays:Long=0,val lastPlayed:Long=0)
enum class MixUnit { SONGS, ALBUMS, ARTISTS }
enum class PlayedFilter { ALL, UNPLAYED, PLAYED, PARTLY_PLAYED }
enum class MixOrder { RANDOM, MOST_PLAYED, LEAST_PLAYED, MOST_PLAYED_AVERAGE, LEAST_PLAYED_AVERAGE, RATING_AVERAGE, RATING_TOTAL, RATED_COUNT, RECENT }
data class SmartRule(val id:String,val name:String,val unit:MixUnit=MixUnit.SONGS,val order:MixOrder=MixOrder.RANDOM,val played:PlayedFilter=PlayedFilter.ALL,val genre:String="",val fromYear:Int=0,val toYear:Int=0,val minimumRating:Int=0,val maximumRating:Int=5,val query:String="") {
    fun valid()=id.isNotBlank() && name.isNotBlank() && name.length<=100 && genre.length<=200 && query.length<=200 && fromYear in 0..9999 && toYear in 0..9999 && (fromYear==0 || toYear==0 || fromYear<=toYear) && minimumRating in 0..5 && maximumRating in minimumRating..5 && !(unit==MixUnit.SONGS && played==PlayedFilter.PARTLY_PLAYED)
}
data class NamedPlaylist(val id:String,val name:String,val ids:List<String>)
data class QueueSnapshot(val ids:List<String> = emptyList(),val index:Int=0,val position:Long=0,val ruleId:String?=null,val pending:List<String> = emptyList(),val seen:Set<String> = emptySet())

object SmartRules {
    fun select(library:List<TrackFacts>,stats:Map<String,TrackStats>,rule:SmartRule,random:Random=Random.Default):List<TrackFacts> {
        if(!rule.valid()) return emptyList()
        val filtered=library.filter {t->
            val s=stats[t.statKey] ?: TrackStats()
            (rule.genre.isBlank() || t.genre.equals(rule.genre,true)) && (rule.fromYear==0 || t.year>=rule.fromYear) && (rule.toYear==0 || t.year in 1..rule.toYear) && s.rating in rule.minimumRating..rule.maximumRating && (rule.query.isBlank() || listOf(t.title,t.artist,t.albumTitle).any{it.contains(rule.query,true)})
        }.distinctBy{it.id}
        val groups=filtered.groupBy {when(rule.unit){MixUnit.SONGS->it.id;MixUnit.ALBUMS->it.album;MixUnit.ARTISTS->it.artist}}.values.filter {group->
            val played=group.count{(stats[it.statKey]?.plays ?: 0)>0}
            when(rule.played){PlayedFilter.ALL->true;PlayedFilter.UNPLAYED->played==0;PlayedFilter.PLAYED->played==group.size;PlayedFilter.PARTLY_PLAYED->played in 1 until group.size}
        }
        fun score(group:List<TrackFacts>):Double {
            val values=group.map{stats[it.statKey] ?: TrackStats()}
            return when(rule.order){
                MixOrder.MOST_PLAYED,MixOrder.LEAST_PLAYED->values.sumOf{it.plays}.toDouble()
                MixOrder.MOST_PLAYED_AVERAGE,MixOrder.LEAST_PLAYED_AVERAGE->values.map{it.plays.toDouble()}.average()
                MixOrder.RATING_AVERAGE->values.map{it.rating.toDouble()}.average()
                MixOrder.RATING_TOTAL->values.sumOf{it.rating}.toDouble()
                MixOrder.RATED_COUNT->values.count{it.rating>0}.toDouble()
                MixOrder.RECENT->values.maxOf{it.lastPlayed}.toDouble()
                MixOrder.RANDOM->0.0
            }
        }
        val ordered=when(rule.order){
            MixOrder.RANDOM->groups.shuffled(random)
            MixOrder.LEAST_PLAYED,MixOrder.LEAST_PLAYED_AVERAGE->groups.sortedWith(compareBy<List<TrackFacts>>{score(it)}.thenBy{it.first().id})
            else->groups.sortedWith(compareByDescending<List<TrackFacts>>{score(it)}.thenBy{it.first().id})
        }
        return ordered.flatMap {group->if(rule.unit==MixUnit.SONGS)group else group.sortedWith(compareBy<TrackFacts>{it.album}.thenBy{it.number}.thenBy{it.title}.thenBy{it.id})}
    }
}
/** A bounded look-ahead reserves each song once per cycle; user-added queue items are excluded. */
class MixCursor(pending:List<String> = emptyList(),seen:Set<String> = emptySet()) {
    val pending=pending.toMutableList()
    val seen=seen.toMutableSet()
    fun take(library:List<TrackFacts>,stats:Map<String,TrackStats>,rule:SmartRule,exclude:Set<String>,limit:Int):List<String> {
        if(limit<=0)return emptyList()
        val eligible=SmartRules.select(library,stats,rule);val allowed=eligible.map{it.id}.toSet();pending.removeAll{it !in allowed}
        if(pending.isEmpty()) {
            var next=eligible.map{it.id}.filter{it !in seen && it !in exclude}
            if(next.isEmpty() && rule.played!=PlayedFilter.UNPLAYED){seen.clear();next=eligible.map{it.id}.filter{it !in exclude}}
            pending.addAll(next)
        }
        val result=mutableListOf<String>()
        while(pending.isNotEmpty() && result.size<limit){val id=pending.removeAt(0);if(id !in exclude && id !in result){result+=id;seen+=id}}
        return result
    }
}
