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
    @Volatile var queues:Map<String,QueueSnapshot> = mapOf(CarQueues.PHONE to QueueSnapshot());private set
    @Volatile var cars:List<CarProfile> = CarQueues.defaults();private set
    @Volatile var activeProfile=CarQueues.PHONE;private set
    @Volatile var lastCar=CarQueues.DEFAULT_CAR;private set
    @Volatile var automaticCars=false;private set
    val queue:QueueSnapshot get()=queues[activeProfile] ?: QueueSnapshot()
    @Volatile var revision=0L;private set
    @Volatile var libraryRevision=0L;private set
    private var loadFailed=false
    @Volatile var error:String?=null;private set
    init {
        if(file.baseFile.exists()) try {
            require(file.baseFile.length()<=32*1024*1024)
            val bytes=file.openRead().use{it.readBytes()};require(bytes.size<=32*1024*1024)
            val json=JSONObject(String(bytes,Charsets.UTF_8));require(json.getInt("version")==1)
            val data=json.optJSONObject("stats") ?: JSONObject();val map=mutableMapOf<String,TrackStats>()
            for(key in data.keys()){val s=data.getJSONObject(key);map[key]=TrackStats(s.optInt("rating").coerceIn(0,5),s.optLong("plays").coerceAtLeast(0),s.optLong("last").coerceAtLeast(0),s.optBoolean("ratingSet",true),s.optBoolean("ratingKnown",true))};stats=map
            rules=json.optJSONArray("rules")?.let{a->(0 until a.length()).mapNotNull{decodeRule(a.getJSONObject(it))}} ?: presets()
            playlists=json.optJSONArray("playlists")?.let{a->(0 until a.length()).map{val p=a.getJSONObject(it);NamedPlaylist(p.getString("id"),p.getString("name"),strings(p.optJSONArray("ids")))}} ?: emptyList()
            val legacy=json.optJSONObject("queue")?.let(::decodeQueue) ?: QueueSnapshot()
            cars=json.optJSONArray("cars")?.let{a->require(a.length()<=CarQueues.MAX_CARS);(0 until a.length()).map{val c=a.getJSONObject(it);CarProfile(c.getString("id"),c.getString("name"),c.optString("bluetooth").takeIf{it.isNotEmpty()})}} ?: CarQueues.defaults()
            require(cars.any{it.id==CarQueues.DEFAULT_CAR} && cars.map{it.id}.distinct().size==cars.size)
            require(cars.all{it.id.startsWith("car:") && it.name.isNotBlank() && it.name.length<=100})
            val saved=json.optJSONObject("queues")
            queues=(listOf(CarQueues.PHONE)+cars.map{it.id}).associateWith{id->saved?.optJSONObject(id)?.let(::decodeQueue) ?: if(id==CarQueues.PHONE)legacy else QueueSnapshot()}
            lastCar=json.optString("lastCar",CarQueues.DEFAULT_CAR).takeIf{id->cars.any{it.id==id}} ?: CarQueues.DEFAULT_CAR
            automaticCars=json.optBoolean("automaticCars",false)
        }catch(_:Exception){loadFailed=true;error="Impossibile leggere lo stato salvato. Il file originale è conservato."}
    }
    @Synchronized fun rate(key:String,rating:Int){require(rating in 0..5);stats=stats+(key to (stats[key] ?: TrackStats()).copy(rating=rating,ratingSet=true,ratingKnown=true));libraryRevision++;save()}
    @Synchronized fun importRating(key:String,rating:Int?){importRatings(mapOf(key to rating))}
    @Synchronized fun importRatings(values:Map<String,Int?>){
        val changes=mutableMapOf<String,TrackStats>()
        for((key,rating) in values){
            require(rating==null || rating in 0..5);val old=stats[key] ?: TrackStats()
            if(!old.ratingSet && (old.rating!=(rating ?: 0) || old.ratingKnown!=(rating!=null)))changes[key]=old.copy(rating=rating ?: 0,ratingKnown=rating!=null)
        }
        if(changes.isEmpty())return
        stats=stats+changes;libraryRevision++;save()
    }
    @Synchronized fun played(key:String){val old=stats[key] ?: TrackStats();stats=stats+(key to old.copy(plays=old.plays+1,lastPlayed=System.currentTimeMillis()));save()}
    @Synchronized fun put(rule:SmartRule){require(rule.valid());rules=rules.filterNot{it.id==rule.id}+rule;libraryRevision++;save()}
    @Synchronized fun removeRule(id:String){rules=rules.filterNot{it.id==id};libraryRevision++;save()}
    @Synchronized fun putPlaylist(name:String,ids:List<String>,id:String=UUID.randomUUID().toString()){require(name.isNotBlank() && name.length<=100 && ids.size<=20000);playlists=playlists.filterNot{it.id==id}+NamedPlaylist(id,name,ids);libraryRevision++;save()}
    @Synchronized fun removePlaylist(id:String){playlists=playlists.filterNot{it.id==id};libraryRevision++;save()}
    @Synchronized fun saveQueue(snapshot:QueueSnapshot){require(snapshot.ids.size<=20000);queues=queues+(activeProfile to snapshot);save(false)}
    @Synchronized fun selectProfile(id:String){require(id==CarQueues.PHONE || cars.any{it.id==id});activeProfile=id;if(id!=CarQueues.PHONE)lastCar=id;save()}
    @Synchronized fun putCar(name:String,id:String="car:"+UUID.randomUUID().toString(),bluetoothKey:String?=cars.find{it.id==id}?.bluetoothKey){
        require(name.isNotBlank() && name.length<=100 && id.startsWith("car:") && id!=CarQueues.DEFAULT_CAR);require(cars.any{it.id==id} || cars.size<CarQueues.MAX_CARS){"Massimo dodici profili auto"}
        require(bluetoothKey==null || bluetoothKey.matches(Regex("[a-f0-9]{64}")))
        require(bluetoothKey==null || cars.none{it.id!=id && it.bluetoothKey==bluetoothKey}){"Dispositivo già associato a un'altra auto"}
        cars=cars.filterNot{it.id==id}+CarProfile(id,name,bluetoothKey);libraryRevision++;save()
    }
    @Synchronized fun removeCar(id:String){require(id!=activeProfile && id!=CarQueues.DEFAULT_CAR);cars=cars.filterNot{it.id==id};queues=queues-id;if(lastCar==id)lastCar=CarQueues.DEFAULT_CAR;libraryRevision++;save()}
    @Synchronized fun setAutomaticCars(enabled:Boolean){automaticCars=enabled;save()}
    private fun save(changed:Boolean=true) {
        if(changed) revision++
        // Preserve an unreadable original instead of silently overwriting it.
        if(loadFailed) return
        val statCopy=stats;val ruleCopy=rules;val listCopy=playlists;val queueCopy=queues;val carCopy=cars;val automatic=automaticCars;val last=lastCar
        worker.execute {
            val json=JSONObject().put("version",1);val values=JSONObject()
            statCopy.forEach{(id,s)->values.put(id,JSONObject().put("rating",s.rating).put("plays",s.plays).put("last",s.lastPlayed).put("ratingSet",s.ratingSet).put("ratingKnown",s.ratingKnown))}
            json.put("stats",values).put("rules",JSONArray(ruleCopy.map(::encodeRule))).put("playlists",JSONArray(listCopy.map{JSONObject().put("id",it.id).put("name",it.name).put("ids",JSONArray(it.ids))}))
            val savedQueues=JSONObject();queueCopy.forEach{(id,q)->savedQueues.put(id,encodeQueue(q))}
            json.put("queues",savedQueues).put("queue",encodeQueue(queueCopy[CarQueues.PHONE] ?: QueueSnapshot()))
            json.put("cars",JSONArray(carCopy.map{JSONObject().put("id",it.id).put("name",it.name).put("bluetooth",it.bluetoothKey ?: "")}))
            json.put("automaticCars",automatic).put("lastCar",last)
            var stream:java.io.FileOutputStream?=null
            try {stream=file.startWrite();stream.write(json.toString().toByteArray());file.finishWrite(stream);error=null}catch(_:Exception){file.failWrite(stream);error="Salvataggio non riuscito. Controlla lo spazio disponibile."}
        }
    }
    companion object {
        @Volatile private var instance:ListeningStore?=null
        fun get(context:Context)=instance ?: synchronized(this){instance ?: ListeningStore(context.applicationContext).also{instance=it}}
        private fun strings(a:JSONArray?):List<String> = if(a==null)emptyList() else (0 until minOf(a.length(),20000)).map{a.getString(it)}
        fun encodeQueue(q:QueueSnapshot)=JSONObject().put("ids",JSONArray(q.ids)).put("index",q.index).put("position",q.position).put("rule",q.ruleId ?: "").put("pending",JSONArray(q.pending)).put("seen",JSONArray(q.seen.toList())).put("name",q.name).put("source",q.sourceId ?: "").put("shuffle",q.shuffle).put("repeat",q.repeat)
        fun decodeQueue(q:JSONObject)=QueueSnapshot(strings(q.optJSONArray("ids")),q.optInt("index").coerceAtLeast(0),q.optLong("position").coerceAtLeast(0),q.optString("rule").takeIf{it.isNotEmpty()},strings(q.optJSONArray("pending")),strings(q.optJSONArray("seen")).toSet(),q.optString("name","Coda manuale").take(100),q.optString("source").takeIf{it.isNotEmpty()}?.take(200),q.optBoolean("shuffle"),q.optInt("repeat").coerceIn(0,2))
        fun encodeRule(r:SmartRule)=JSONObject().put("id",r.id).put("name",r.name).put("unit",r.unit.name).put("order",r.order.name).put("played",r.played.name).put("genre",r.genre).put("from",r.fromYear).put("to",r.toYear).put("min",r.minimumRating).put("max",r.maximumRating).put("query",r.query).put("minPlays",r.minimumPlays).put("maxPlays",r.maximumPlays ?: JSONObject.NULL)
        fun decodeRule(j:JSONObject):SmartRule?=try{SmartRule(j.getString("id"),j.getString("name"),MixUnit.valueOf(j.getString("unit")),MixOrder.valueOf(j.getString("order")),PlayedFilter.valueOf(j.getString("played")),j.optString("genre"),j.optInt("from"),j.optInt("to"),j.optInt("min"),j.optInt("max",5),j.optString("query"),j.optLong("minPlays",0),if(j.has("maxPlays") && !j.isNull("maxPlays"))j.getLong("maxPlays") else null).takeIf{it.valid()}}catch(_:Exception){null}
        fun presets()=listOf(
            SmartRule("random","Brani casuali"),SmartRule("unplayed","Brani mai ascoltati",played=PlayedFilter.UNPLAYED),SmartRule("played","Brani già ascoltati",played=PlayedFilter.PLAYED),
            SmartRule("albums","Album casuali",unit=MixUnit.ALBUMS),SmartRule("artists","Artisti casuali",unit=MixUnit.ARTISTS),
            SmartRule("top","Brani più votati",order=MixOrder.RATING_AVERAGE,minimumRating=1),SmartRule("most","Brani più ascoltati",order=MixOrder.MOST_PLAYED),
            SmartRule("partial","Album ascoltati in parte",unit=MixUnit.ALBUMS,played=PlayedFilter.PARTLY_PLAYED))
    }
}
