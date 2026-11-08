package it.ckiikc.musiccraft

import android.app.*
import android.content.*
import android.content.pm.ServiceInfo
import android.net.*
import android.net.nsd.*
import android.os.*
import java.net.Inet4Address
import java.net.InetAddress
import java.util.concurrent.Executors

/** User-started device communication service; it never pretends to play audio. */
class SharingService:Service() {
    private val worker=Executors.newSingleThreadExecutor()
    private val handler=Handler(Looper.getMainLooper())
    private var server:SharingServer?=null
    private var identity:SharingIdentity?=null
    private var address:String?=null
    private var registration:NsdManager.RegistrationListener?=null
    private var busy=false
    private var destroyed=false
    private var building=false
    private val ticker=object:Runnable {override fun run(){update();handler.postDelayed(this,1500)}}
    override fun onBind(intent:Intent?)=null
    override fun onCreate() {
        super.onCreate();SharingGate.running=true
        getSystemService(NotificationManager::class.java).createNotificationChannel(NotificationChannel("musiccraft_sharing","Condivisione musicale",NotificationManager.IMPORTANCE_LOW))
        foreground("Avvio della condivisione…");handler.post(ticker)
    }
    override fun onStartCommand(intent:Intent?,flags:Int,startId:Int):Int {if(intent?.action=="stop") stopSelf();return START_NOT_STICKY}
    private fun foreground(text:String) {
        SharingGate.state=text
        val open=PendingIntent.getActivity(this,0,Intent(this,MainActivity::class.java),PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)
        val stop=PendingIntent.getService(this,1,Intent(this,SharingService::class.java).setAction("stop"),PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)
        val notification=Notification.Builder(this,"musiccraft_sharing").setSmallIcon(R.drawable.ic_notification).setContentTitle("MusicCraft · condivisione").setContentText(text).setContentIntent(open).setOngoing(true).addAction(Notification.Action.Builder(null,"Interrompi",stop).build()).build()
        if(Build.VERSION.SDK_INT>=29) startForeground(2001,notification,ServiceInfo.FOREGROUND_SERVICE_TYPE_CONNECTED_DEVICE) else startForeground(2001,notification)
    }
    private fun wifiAddress():InetAddress? {
        val manager=getSystemService(ConnectivityManager::class.java)
        @Suppress("DEPRECATION") val networks=manager.allNetworks
        for(network in networks) {
            val caps=manager.getNetworkCapabilities(network) ?: continue
            if(!caps.hasTransport(NetworkCapabilities.TRANSPORT_WIFI) && !caps.hasTransport(NetworkCapabilities.TRANSPORT_ETHERNET)) continue
            val link=manager.getLinkProperties(network) ?: continue
            link.linkAddresses.map{it.address}.firstOrNull{it is Inet4Address && it.isSiteLocalAddress}?.let{return it}
        }
        return null
    }
    private fun update() {
        if(destroyed) return
        val host=wifiAddress()
        val nowBusy=SharingGate.busy(this)
        if(nowBusy && !busy) server?.suspendTransfers()
        if(nowBusy!=busy) {busy=nowBusy;if(server!=null) foreground(if(busy)"Sospesa: telefono in riproduzione" else "Disponibile sulla rete locale")}
        if(host==null) {if(server!=null) stopServer();if(SharingGate.state!="Connetti il telefono al Wi-Fi") foreground("Connetti il telefono al Wi-Fi");return}
        if(server!=null && address==host.hostAddress || building) return
        stopServer();building=true;foreground("Preparazione del collegamento sicuro…")
        worker.execute {
            try {
                val peer=identity ?: SharingIdentity(this).also {identity=it}
                val created=SharingServer(this,peer,host)
                handler.post {
                    building=false
                    if(destroyed){created.close();return@post}
                    server=created;address=host.hostAddress;SharingGate.pairLink=peer.pairing(address!!,created.port)
                    register(peer.id,created.port)
                    foreground(if(SharingGate.busy(this))"Sospesa: telefono in riproduzione" else "Disponibile sulla rete locale")
                }
            } catch(_:Exception) {handler.post {building=false;if(!destroyed) foreground("Condivisione non disponibile. Interrompi e riprova.")}}
        }
    }
    private fun register(id:String,port:Int) {
        val manager=getSystemService(NsdManager::class.java)
        val listener=object:NsdManager.RegistrationListener {
            override fun onServiceRegistered(info:NsdServiceInfo){}
            override fun onRegistrationFailed(info:NsdServiceInfo,errorCode:Int){}
            override fun onServiceUnregistered(info:NsdServiceInfo){}
            override fun onUnregistrationFailed(info:NsdServiceInfo,errorCode:Int){}
        }
        try {
            manager.registerService(NsdServiceInfo().apply {serviceName="MusicCraft-"+id.take(8);serviceType="_musiccraft._tcp.";setPort(port);setAttribute("id",id);setAttribute("protocol","1")},NsdManager.PROTOCOL_DNS_SD,listener);registration=listener
        } catch(_:Exception){}
    }
    private fun stopServer() {
        registration?.let {try{getSystemService(NsdManager::class.java).unregisterService(it)}catch(_:Exception){}};registration=null
        server?.close();server=null;address=null;SharingGate.pairLink=null
    }
    override fun onDestroy() {destroyed=true;handler.removeCallbacksAndMessages(null);stopServer();worker.shutdownNow();SharingGate.running=false;SharingGate.state="Condivisione disattivata";super.onDestroy()}
}
