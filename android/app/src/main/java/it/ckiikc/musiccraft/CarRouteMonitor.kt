package it.ckiikc.musiccraft

import android.Manifest
import android.bluetooth.*
import android.content.*
import android.content.pm.PackageManager
import android.os.Build
import android.os.Handler
import android.os.Looper
import androidx.car.app.connection.CarConnection
import androidx.lifecycle.Observer
import java.security.MessageDigest

/** Connection events only: no Bluetooth discovery, location permission or polling. */
@Suppress("MissingPermission")
class CarRouteMonitor(private val context:Context,private val store:ListeningStore,private val coordinator:PlaybackCoordinator) {
    private val connection=CarConnection(context)
    private val adapter=(context.getSystemService(Context.BLUETOOTH_SERVICE) as? BluetoothManager)?.adapter
    private val handler=Handler(Looper.getMainLooper())
    private val proxies=mutableMapOf<Int,BluetoothProfile>()
    private val requesting=mutableSetOf<Int>()
    private var projected=false
    private var manual=false
    private var closed=false
    private var registered=false
    private val observer=Observer<Int>{type->
        if(closed)return@Observer
        val before=projected;projected=type==CarConnection.CONNECTION_TYPE_PROJECTION
        if(before!=projected)manual=false
        if(before && !projected){coordinator.switchProfile(CarQueues.PHONE);coordinator.routingNotice("")} else refresh()
    }
    private val receiver=object:BroadcastReceiver(){override fun onReceive(c:Context,intent:Intent){refresh()}}
    private val listener=object:BluetoothProfile.ServiceListener {
        override fun onServiceConnected(profile:Int,proxy:BluetoothProfile){
            onMain {
            requesting.remove(profile)
            if(closed){try{adapter?.closeProfileProxy(profile,proxy)}catch(_:SecurityException){};return@onMain}
            proxies[profile]=proxy;evaluate()
            }
        }
        override fun onServiceDisconnected(profile:Int){onMain{proxies.remove(profile);requesting.remove(profile);if(!closed)evaluate()}}
    }
    private fun onMain(block:()->Unit){if(Looper.myLooper()==handler.looper)block() else handler.post{block()}}
    fun start(){
        if(closed || registered)return
        val filter=IntentFilter().apply{addAction(BluetoothA2dp.ACTION_CONNECTION_STATE_CHANGED);addAction(BluetoothHeadset.ACTION_CONNECTION_STATE_CHANGED);addAction(BluetoothAdapter.ACTION_STATE_CHANGED)}
        // Bluetooth broadcasts originate from a privileged system Bluetooth process.
        if(Build.VERSION.SDK_INT>=33)context.registerReceiver(receiver,filter,Context.RECEIVER_EXPORTED) else context.registerReceiver(receiver,filter)
        registered=true
        connection.type.observeForever(observer)
    }
    fun manualSelection(){manual=true}
    fun automaticChanged(){manual=!store.automaticCars;refresh()}
    fun refresh(){
        if(closed)return
        if(store.automaticCars && permitted(context))for(profile in listOf(BluetoothProfile.A2DP,BluetoothProfile.HEADSET)) {
            if(profile !in proxies && requesting.add(profile))try{if(adapter?.getProfileProxy(context,listener,profile)!=true)requesting.remove(profile)}catch(_:SecurityException){requesting.remove(profile)}
        }
        evaluate()
    }
    private fun evaluate(){
        if(!projected || closed || manual)return
        val keys=if(store.automaticCars && permitted(context))proxies.values.toList().flatMap{try{it.connectedDevices.map{d->key(d.address)}}catch(_:SecurityException){emptyList()}}.toSet() else emptySet()
        val route=CarQueues.route(projected,store.automaticCars,permitted(context),store.cars,keys) ?: return
        coordinator.switchProfile(route.profileId)
        coordinator.routingNotice(route.notice)
    }
    fun close(){
        if(closed)return
        closed=true
        connection.type.removeObserver(observer)
        if(registered){context.unregisterReceiver(receiver);registered=false}
        requesting.clear()
        // Detach first: closeProfileProxy can synchronously invoke disconnect callbacks.
        closeProfileProxies(proxies){id,p->try{adapter?.closeProfileProxy(id,p)}catch(_:SecurityException){}}
    }
    companion object {
        fun permitted(context:Context)=Build.VERSION.SDK_INT<31 || context.checkSelfPermission(Manifest.permission.BLUETOOTH_CONNECT)==PackageManager.PERMISSION_GRANTED
        fun key(address:String)=MessageDigest.getInstance("SHA-256").digest(address.uppercase().toByteArray()).joinToString(""){"%02x".format(it)}
        fun paired(context:Context):List<Pair<String,String>> {
            if(!permitted(context))return emptyList()
            return try{(context.getSystemService(Context.BLUETOOTH_SERVICE) as? BluetoothManager)?.adapter?.bondedDevices?.map{key(it.address) to (it.name ?: "Dispositivo Bluetooth")}?.sortedBy{it.second} ?: emptyList()}catch(_:SecurityException){emptyList()}
        }
    }
}
