package it.ckiikc.musiccraft

/** Named queues are local to this installation; a Bluetooth device must be explicitly associated. */
data class CarProfile(val id:String,val name:String,val bluetoothKey:String?=null)
data class CarRoute(val profileId:String,val notice:String)
object CarQueues {
    const val PHONE="phone"
    const val DEFAULT_CAR="car:default"
    const val MAX_CARS=12
    fun defaults()=listOf(CarProfile(DEFAULT_CAR,"Auto non riconosciuta"))
    fun profileName(id:String,cars:List<CarProfile>)=if(id==PHONE)"Telefono" else cars.find{it.id==id}?.name ?: "Auto"
    /** No guessing from device names, no use of unassociated headphones, no first-device wins. */
    fun recognized(cars:List<CarProfile>,connected:Set<String>):String? = cars.filter{it.bluetoothKey!=null && it.bluetoothKey in connected}.singleOrNull()?.id
    fun route(projected:Boolean,automatic:Boolean,permitted:Boolean,cars:List<CarProfile>,connected:Set<String>):CarRoute? {
        if(!projected)return null
        if(!automatic)return CarRoute(DEFAULT_CAR,"Scegli il profilo da Auto e code")
        if(!permitted)return CarRoute(DEFAULT_CAR,"Riconoscimento Bluetooth non autorizzato: scegli il profilo auto")
        val matches=cars.filter{it.bluetoothKey!=null && it.bluetoothKey in connected}
        return when(matches.size){
            1->CarRoute(matches.single().id,"Auto riconosciuta tramite Bluetooth")
            0->CarRoute(DEFAULT_CAR,"Auto non riconosciuta: scegli il profilo da Auto e code")
            else->CarRoute(DEFAULT_CAR,"Più auto riconosciute: scegli il profilo da Auto e code")
        }
    }
    fun queueEntry(profile:String,revision:Long,index:Int)="queue-entry:$profile:$revision:$index"
    fun queueIndex(id:String,profile:String,revision:Long,size:Int):Int? = id.removePrefix("queue-entry:$profile:$revision:").takeIf{id.startsWith("queue-entry:$profile:$revision:")}?.toIntOrNull()?.takeIf{it in 0 until size}
}
