package it.ckiikc.musiccraft

import android.app.*
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.IBinder
import java.util.concurrent.Executors

/** An explicit user action starts a bounded data-sync job, independent of playback. */
class SyncService:Service() {
    private val worker=Executors.newSingleThreadExecutor()
    override fun onBind(intent:Intent?):IBinder?=null
    override fun onCreate(){super.onCreate();getSystemService(NotificationManager::class.java).createNotificationChannel(NotificationChannel("musiccraft_sync","Sincronizzazione musicale",NotificationManager.IMPORTANCE_LOW))
        val open=PendingIntent.getActivity(this,0,Intent(this,MainActivity::class.java),PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)
        val notification=Notification.Builder(this,"musiccraft_sync").setSmallIcon(R.drawable.ic_notification).setContentTitle("MusicCraft · sincronizzazione").setContentText("Controllo e trasferimento dei file in corso").setContentIntent(open).setOngoing(true).build()
        if(Build.VERSION.SDK_INT>=29)startForeground(2002,notification,ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC)else startForeground(2002,notification)
    }
    override fun onStartCommand(intent:Intent?,flags:Int,startId:Int):Int {val task=SyncPanel.takeTask();if(task==null){stopSelf(startId);return START_NOT_STICKY};worker.execute{try{task()}finally{stopSelf(startId)}};return START_NOT_STICKY}
    override fun onTimeout(startId:Int,fgsType:Int){FolderSync.cancel.set(true);stopSelf(startId)}
    override fun onDestroy(){FolderSync.cancel.set(true);worker.shutdown();super.onDestroy()}
}
