package it.ckiikc.musiccraft

import android.content.Context
import android.util.AtomicFile
import org.json.JSONArray
import org.json.JSONObject
import java.util.UUID
import java.util.concurrent.Executors

/** Private atomic persistence. Writes are serialized away from playback/UI callbacks. */
class ListeningStore private constructor(context:Context) {
    private val file=AtomicFile(java.io.File(context.filesDir,"listening-v1.json"))
    private val worker=Executors.newSingleThreadExecutor()
    @Volatile var stats:Map<String,TrackStats> = emptyMap();private set
    @Volatile var rules:List<SmartRule> = presets();private set
    @Volatile var playlists:List<NamedPlaylist> = emptyList();private set
    @Volatile var queue=QueueSnapshot();private set
    @Volatile var revision=0L;private set
    @Volatile var libraryRevision=0L;private set
    private var loadFailed=false
    @Volatile var error:String?=null;private set
    init {
        if(file.baseFile.exists()) try {
            val bytes=file.openRead().use{it.readBytes()};require(bytes.size<=8*1024*1024)
            val json=JSONObject(String(bytes,Charsets.UTF_8));require(json.getInt("version")==1)
            val data=json.optJSONObject("stats") ?: JSONObject();val map=mutableMapOf<String,TrackStats>()
            for(key in data.keys()){val s=data.getJSONObject(key);map[key]=TrackStats(s.optInt("rating").coerceIn(0,5),s.optLong("plays").coerceAtLeast(0),s.optLong("last").coerceAtLeast(0))};stats=map
            rules=json.optJSONArray("rules")?.let{a->(0 until a.length()).mapNotNull{decodeRule(a.getJSONObject(it))}} ?: presets()
            playlists=json.optJSONArray("playlists")?.let{a->(0 until a.length()).map{val p=a.getJSONObject(it);NamedPlaylist(p.getString("id"),p.getString("name"),strings(p.optJSONArray("ids")))}} ?: emptyList()
            json.optJSONObject("queue")?.let{q->queue=QueueSnapshot(strings(q.optJSONArray("ids")),q.optInt("index"),q.optLong("position").coerceAtLeast(0),q.optString("rule").takeIf{it.isNotEmpty()},strings(q.optJSONArray("pending")),strings(q.optJSONArray("seen")).toSet())}
        }catch(_:Exception){loadFailed=true;error="Impossibile leggere lo stato salvato. Il file originale è conservato."}
    }
    @Synchronized fun rate(key:String,rating:Int){require(rating in 0..5);stats=stats+(key to (stats[key] ?: TrackStats()).copy(rating=rating));save()}
    @Synchronized fun played(key:String){val old=stats[key] ?: TrackStats();stats=stats+(key to old.copy(plays=old.plays+1,lastPlayed=System.currentTimeMillis()));save()}
    @Synchronized fun put(rule:SmartRule){require(rule.valid());rules=rules.filterNot{it.id==rule.id}+rule;libraryRevision++;save()}
    @Synchronized fun removeRule(id:String){rules=rules.filterNot{it.id==id};libraryRevision++;save()}
    @Synchronized fun putPlaylist(name:String,ids:List<String>,id:String=UUID.randomUUID().toString()){require(name.isNotBlank() && name.length<=100 && ids.size<=20000);playlists=playlists.filterNot{it.id==id}+NamedPlaylist(id,name,ids);libraryRevision++;save()}
    @Synchronized fun removePlaylist(id:String){playlists=playlists.filterNot{it.id==id};libraryRevision++;save()}
    @Synchronized fun saveQueue(snapshot:QueueSnapshot){queue=snapshot;save(false)}
    private fun save(changed:Boolean=true) {
        if(changed) revision++
        // Preserve an unreadable original instead of silently overwriting it.
        if(loadFailed) return
        val statCopy=stats;val ruleCopy=rules;val listCopy=playlists;val q=queue
        worker.execute {
            val json=JSONObject().put("version",1);val values=JSONObject()
            statCopy.forEach{(id,s)->values.put(id,JSONObject().put("rating",s.rating).put("plays",s.plays).put("last",s.lastPlayed))}
            json.put("stats",values).put("rules",JSONArray(ruleCopy.map(::encodeRule))).put("playlists",JSONArray(listCopy.map{JSONObject().put("id",it.id).put("name",it.name).put("ids",JSONArray(it.ids))}))
            json.put("queue",JSONObject().put("ids",JSONArray(q.ids)).put("index",q.index).put("position",q.position).put("rule",q.ruleId ?: "").put("pending",JSONArray(q.pending)).put("seen",JSONArray(q.seen.toList())))
            var stream:java.io.FileOutputStream?=null
            try {stream=file.startWrite();stream.write(json.toString().toByteArray());file.finishWrite(stream);error=null}catch(_:Exception){file.failWrite(stream);error="Salvataggio non riuscito. Controlla lo spazio disponibile."}
        }
    }
    companion object {
        @Volatile private var instance:ListeningStore?=null
        fun get(context:Context)=instance ?: synchronized(this){instance ?: ListeningStore(context.applicationContext).also{instance=it}}
        private fun strings(a:JSONArray?):List<String> = if(a==null)emptyList() else (0 until minOf(a.length(),20000)).map{a.getString(it)}
        fun encodeRule(r:SmartRule)=JSONObject().put("id",r.id).put("name",r.name).put("unit",r.unit.name).put("order",r.order.name).put("played",r.played.name).put("genre",r.genre).put("from",r.fromYear).put("to",r.toYear).put("min",r.minimumRating).put("max",r.maximumRating).put("query",r.query)
        fun decodeRule(j:JSONObject):SmartRule?=try{SmartRule(j.getString("id"),j.getString("name"),MixUnit.valueOf(j.getString("unit")),MixOrder.valueOf(j.getString("order")),PlayedFilter.valueOf(j.getString("played")),j.optString("genre"),j.optInt("from"),j.optInt("to"),j.optInt("min"),j.optInt("max",5),j.optString("query")).takeIf{it.valid()}}catch(_:Exception){null}
        fun presets()=listOf(
            SmartRule("random","Brani casuali"),SmartRule("unplayed","Brani mai ascoltati",played=PlayedFilter.UNPLAYED),SmartRule("played","Brani già ascoltati",played=PlayedFilter.PLAYED),
            SmartRule("albums","Album casuali",unit=MixUnit.ALBUMS),SmartRule("artists","Artisti casuali",unit=MixUnit.ARTISTS),
            SmartRule("top","Brani più votati",order=MixOrder.RATING_AVERAGE,minimumRating=1),SmartRule("most","Brani più ascoltati",order=MixOrder.MOST_PLAYED),
            SmartRule("partial","Album ascoltati in parte",unit=MixUnit.ALBUMS,played=PlayedFilter.PARTLY_PLAYED))
    }
}
