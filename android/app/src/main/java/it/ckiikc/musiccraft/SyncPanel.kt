package it.ckiikc.musiccraft

import android.app.Activity
import android.app.AlertDialog
import android.content.Context
import android.content.Intent
import android.graphics.Color
import android.net.Uri
import android.os.Handler
import android.os.Looper
import android.text.InputType
import android.view.Gravity
import android.view.View
import android.widget.*
import org.json.JSONObject
import java.io.File

object SyncPanel {
    const val PICK=8460
    private val main=Handler(Looper.getMainLooper())
    @Volatile var busy=false
    @Volatile private var message="Scegli le cartelle, poi genera un’anteprima. Nessun file viene modificato prima della conferma."
    private var plan:JSONObject?=null
    private var planConfig:String?=null
    private var task:(()->Unit)?=null
    private var remoteDialog:AlertDialog?=null
    private var remoteOwner:Activity?=null
    private var remotePlanId:String?=null
    @Synchronized fun takeTask()=task.also{task=null}
    private fun prefs(c:Context)=c.getSharedPreferences("folder-sync-v1",Context.MODE_PRIVATE)
    fun result(activity:Activity,request:Int,result:Int,data:Intent?) {
        if(request!=PICK || result!=Activity.RESULT_OK)return
        try{val uri=data?.data ?: return;require(uri.authority=="com.android.externalstorage.documents"){"Scegli una cartella sul telefono o sulla scheda SD"};activity.contentResolver.takePersistableUriPermission(uri,Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION);FolderSync.setFolder(activity,uri);plan=null;toast(activity,"Cartella autorizzata")}catch(e:Exception){toast(activity,e.message ?: "Cartella non disponibile")}
    }
    private fun toast(c:Context,s:String)=Toast.makeText(c,s,Toast.LENGTH_LONG).show()
    private fun column(c:Context)=LinearLayout(c).apply{orientation=LinearLayout.VERTICAL}
    private fun text(c:Context,value:String,size:Float=15f)=TextView(c).apply{text=value;textSize=size;setTextColor(if(size<16)Color.rgb(174,181,185)else Color.WHITE);val pad=(8*c.resources.displayMetrics.density).toInt();setPadding(0,pad,0,pad)}
    private fun button(c:Context,value:String,action:()->Unit)=Button(c).apply{text=value;isAllCaps=false;setTextColor(Color.rgb(229,187,122));minimumHeight=(48*c.resources.displayMetrics.density).toInt();background=android.graphics.drawable.GradientDrawable().apply{setColor(Color.rgb(32,38,42));cornerRadius=10*c.resources.displayMetrics.density};setOnClickListener{action()}}
    fun view(activity:Activity):View {
        val c=activity;val content=column(c);val scroll=ScrollView(c).apply{addView(content)}
        content.addView(text(c,"Sincronizzazione",22f));content.addView(text(c,"La tua musica, sul NAS e sul telefono. Controlla l’anteprima prima di trasferire i file.",14f))
        if(android.os.Build.VERSION.SDK_INT<29){content.addView(text(c,"La sincronizzazione richiede Android 10 o successivo."));return scroll}
        fun invalidate(){plan=null}
        content.addView(text(c,"CARTELLA SUL TELEFONO",12f));val folder=text(c,FolderSync.folder(c)?.let{Uri.decode(Uri.parse(it).lastPathSegment.orEmpty())} ?: "Nessuna cartella autorizzata");content.addView(folder)
        content.addView(button(c,"Scegli cartella musicale…"){if(!busy)c.startActivityForResult(Intent(Intent.ACTION_OPEN_DOCUMENT_TREE).addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION or Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION),PICK)})
        content.addView(Switch(c).apply{text="Consenti sincronizzazione dal computer";setTextColor(Color.WHITE);isChecked=FolderSync.computer(c);minimumHeight=(56*c.resources.displayMetrics.density).toInt();isEnabled=!busy;setOnCheckedChangeListener{_,enabled->try{FolderSync.computer(c,enabled)}catch(e:Exception){isChecked=false;toast(c,e.message.orEmpty())}}})
        content.addView(text(c,"Richiede anche Condividi → Attiva e l’abbinamento esistente. Ogni anteprima del computer richiede la conferma su questo telefono.",13f))
        content.addView(text(c,"ACCESSO DIRETTO AL NAS",12f))
        val fields=linkedMapOf<String,EditText>();val settingsForm=column(c)
        for((key,label)in listOf("host" to "Server NAS (nome o IP)","share" to "Condivisione SMB", "subfolder" to "Sottocartella (facoltativa)","user" to "Nome utente", "domain" to "Dominio (facoltativo)","password" to "Password (non salvata)")) {
            val field=EditText(c).apply{hint=label;contentDescription=label;setSingleLine();setTextColor(Color.WHITE);setHintTextColor(Color.LTGRAY);textSize=16f;minimumHeight=(52*c.resources.displayMetrics.density).toInt();if(key=="password")inputType=InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_VARIATION_PASSWORD else setText(prefs(c).getString(key,""))};fields[key]=field;settingsForm.addView(field)
        }
        val server=text(c,listOf(prefs(c).getString("host","").orEmpty(),prefs(c).getString("share","").orEmpty()).filter{it.isNotBlank()}.joinToString(" / ").ifEmpty{"Nessun NAS configurato"});content.addView(server)
        content.addView(button(c,"Configura accesso SMB…"){if(!busy){(settingsForm.parent as? android.view.ViewGroup)?.removeView(settingsForm);AlertDialog.Builder(c).setTitle("Accesso al NAS · SMB2/3").setView(ScrollView(c).apply{setPadding(24,0,24,0);addView(settingsForm)}).setNegativeButton("Annulla",null).setPositiveButton("Usa queste impostazioni"){_,_->invalidate();server.text=listOf(fields.getValue("host").text.toString(),fields.getValue("share").text.toString()).filter{it.isNotBlank()}.joinToString(" / ")}.show()}})
        val direction=Spinner(c).apply{adapter=ArrayAdapter(c,android.R.layout.simple_spinner_dropdown_item,listOf("NAS → telefono · unidirezionale","Telefono → NAS · unidirezionale","NAS ↔ telefono · bidirezionale"));setSelection(prefs(c).getInt("direction",0).coerceIn(0,2));minimumHeight=(52*c.resources.displayMetrics.density).toInt()};content.addView(direction)
        val status=text(c,message,14f);content.addView(status)
        fun config():JSONObject {val result=JSONObject();for((key,field)in fields)if(key!="password")result.put(key,field.text.toString().trim());result.put("direction",direction.selectedItemPosition);result.put("tree",FolderSync.folder(c) ?: error("Scegli una cartella sul telefono"));return result}
        fun run(apply:Boolean) {
            if(busy)return
            try {
                val settings=config();require(settings.getString("host").isNotEmpty() && settings.getString("share").isNotEmpty()){"Inserisci server e condivisione SMB"}
                val password=fields.getValue("password").text.toString().toCharArray();val context=c.applicationContext
                val edit=prefs(c).edit();for(key in listOf("host","share","subfolder","user","domain"))edit.putString(key,settings.getString(key));edit.putInt("direction",settings.getInt("direction")).apply()
                val selected=plan?.let{JSONObject(it.toString())};if(apply)check(selected!=null && planConfig==settings.toString()){"Le impostazioni sono cambiate: genera una nuova anteprima"}
                busy=true;FolderSync.cancel.set(false);message=if(apply)"Sincronizzazione in corso…" else "Lettura e confronto dei file…";status.text=message
                task={try {
                    SmbSyncStorage(settings.getString("host"),settings.getString("share"),settings.getString("subfolder"),settings.getString("user"),password,settings.getString("domain"),fingerprintCache=File(context.filesDir,"sync-index/smb-${java.security.MessageDigest.getInstance("SHA-256").digest(JSONObject(settings.toString()).apply{remove("direction");remove("user");remove("domain")}.toString().toByteArray()).joinToString(""){"%02x".format(it)}}.json")).use{nas->
                        TreeSyncStorage(context,Uri.parse(settings.getString("tree"))).use{phone->
                            val key=java.security.MessageDigest.getInstance("SHA-256").digest(JSONObject(settings.toString()).apply{remove("direction");remove("user");remove("domain")}.toString().toByteArray()).joinToString(""){"%02x".format(it)};val history=File(context.filesDir,"sync-history/$key.json")
                            if(apply){val after=FolderSync.execute(context,selected!!,nas,phone){n,total->message="$n / $total"};FolderSync.save(history,after);plan=null;Catalog.get(context).refresh(force=true).get(20,java.util.concurrent.TimeUnit.SECONDS);message="Sincronizzazione completata e verificata"}
                            else {val baseline=if(history.exists())JSONObject(history.readText())else null;val d=listOf("Forward","Reverse","Both")[settings.getInt("direction")];plan=FolderSync.preview(SyncFiles.snapshot(nas,SyncFiles.root(nas),FolderSync.cancel),SyncFiles.snapshot(phone,SyncFiles.root(phone),FolderSync.cancel),d,baseline);planConfig=settings.toString();message="Anteprima pronta: ${plan!!.getJSONArray("operations").length()} operazioni"}
                        }
                    }
                }catch(e:Exception){plan=null;message=e.message ?: "Sincronizzazione non disponibile"}finally{password.fill('\u0000');busy=false;main.post{if(!c.isDestroyed){status.text=message;if(!apply)plan?.let{preview(c,it){run(true)}}}}}}
                c.startForegroundService(Intent(c,SyncService::class.java))
            }catch(e:Exception){busy=false;toast(c,e.message.orEmpty())}
        }
        content.addView(button(c,"Genera anteprima"){invalidate();run(false)})
        content.addView(button(c,"Rivedi e applica…"){val p=plan;if(p==null)toast(c,"Genera prima l’anteprima")else preview(c,p){run(true)}})
        content.addView(button(c,"Annulla operazione"){FolderSync.cancel.set(true);status.text="Annullamento richiesto; i file originali sono conservati"})
        content.addView(text(c,"Una direzione allinea anche le cancellazioni nella destinazione. In entrambe le direzioni i conflitti richiedono una scelta. File nascosti esclusi; recupero in .musiccraft-sync/recovery.",13f))
        val update=object:Runnable{override fun run(){if(!scroll.isAttachedToWindow)return;status.text=message;folder.text=FolderSync.folder(c)?.let{Uri.decode(Uri.parse(it).lastPathSegment.orEmpty())} ?: "Nessuna cartella autorizzata";main.postDelayed(this,700)}}
        scroll.addOnAttachStateChangeListener(object:View.OnAttachStateChangeListener{override fun onViewAttachedToWindow(v:View){main.post(update)};override fun onViewDetachedFromWindow(v:View){main.removeCallbacks(update)}})
        return scroll
    }
    fun preview(activity:Activity,plan:JSONObject,onCancel:()->Unit={},apply:()->Unit):AlertDialog {
        val ops=plan.getJSONArray("operations");val rows=mutableListOf<String>()
        fun label(i:Int):String {val op=ops.getJSONObject(i);val action=op.optJSONObject("action");val summary=if(action==null)"Conflitto · scegli una versione" else if(action.has("Copy"))"Copia ${if(action.getString("Copy")=="B")"A → B" else "B → A"}" else "Sposta nel recupero · ${action.getString("Delete")}";return "$summary\n${op.getString("path")}"}
        for(i in 0 until ops.length())rows+=label(i)
        val adapter=ArrayAdapter(activity,android.R.layout.simple_list_item_1,rows)
        val list=ListView(activity).apply{this.adapter=adapter;setOnItemClickListener{_,_,i,_->val op=ops.getJSONObject(i);if(op.optString("action")=="Conflict")AlertDialog.Builder(activity).setTitle(op.getString("path")).setItems(arrayOf("Mantieni NAS / A","Mantieni telefono / B")){_,side->val source=op.optJSONObject(if(side==0)"a" else "b");op.put("action",JSONObject().put(if(source==null)"Delete" else "Copy",if(side==0)"B" else "A"));rows[i]=label(i);adapter.notifyDataSetChanged()}.show()}}
        val dialog=AlertDialog.Builder(activity).setTitle("Anteprima · ${ops.length()} operazioni").setView(list).setNegativeButton("Annulla"){_,_->onCancel()}.setPositiveButton("Conferma e sincronizza",null).create()
        dialog.setOnCancelListener{onCancel()}
        dialog.setOnShowListener{dialog.getButton(AlertDialog.BUTTON_POSITIVE).setOnClickListener{
            if((0 until ops.length()).any{ops.getJSONObject(it).optString("action")=="Conflict"}){toast(activity,"Tocca ogni conflitto e scegli la versione da conservare");return@setOnClickListener}
            apply();dialog.dismiss()
        }};dialog.show()
        return dialog
    }
    fun background(activity:Activity){if(remoteOwner===activity){remoteDialog?.dismiss();remoteDialog=null;remoteOwner=null;remotePlanId=null}}
    fun present(activity:Activity) {
        val pending=FolderSync.pending() ?: return
        if(activity.isFinishing || activity.isDestroyed)return
        if(remoteOwner===activity && remoteDialog?.isShowing==true && remotePlanId==pending.first)return
        remoteDialog?.dismiss();remoteOwner=activity;remotePlanId=pending.first
        fun clear(){remoteDialog=null;remoteOwner=null;remotePlanId=null}
        fun refuse(){try{FolderSync.approve(pending.first,false)}catch(_:Exception){};clear()}
        val ops=pending.second.getJSONArray("operations");val deletions=(0 until ops.length()).count{ops.getJSONObject(it).optJSONObject("action")?.has("Delete")==true}
        remoteDialog=AlertDialog.Builder(activity).setTitle("Sincronizzazione dal computer").setMessage("Operazioni: ${ops.length()} · Cancellazioni: $deletions (recuperabili).\n\nRivedi l’elenco completo prima di autorizzare.").setNegativeButton("Rifiuta"){_,_->refuse()}.setPositiveButton("Rivedi anteprima"){_,_->
            remoteDialog=preview(activity,pending.second,onCancel={refuse()}){try{FolderSync.approve(pending.first,true)}catch(_:Exception){toast(activity,"Anteprima scaduta: riprova dal computer")};clear()}
        }.setOnCancelListener{refuse()}.create().also{it.show()}
    }
}
