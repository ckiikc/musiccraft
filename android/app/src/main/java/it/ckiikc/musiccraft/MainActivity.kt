package it.ckiikc.musiccraft

import android.Manifest
import android.app.Activity
import android.content.ComponentName
import android.content.pm.PackageManager
import android.graphics.BitmapFactory
import android.graphics.Color
import android.graphics.Typeface
import android.graphics.drawable.GradientDrawable
import android.os.*
import android.text.Editable
import android.text.TextWatcher
import android.view.*
import android.view.inputmethod.InputMethodManager
import android.widget.*
import androidx.media3.common.*
import androidx.media3.session.*
import com.google.common.util.concurrent.ListenableFuture
import java.util.UUID
import java.util.concurrent.Executors

@androidx.annotation.OptIn(androidx.media3.common.util.UnstableApi::class)
class MainActivity:Activity() {
    private val bg=Color.rgb(18,19,19);private val panel=Color.rgb(34,35,34);private val gold=Color.rgb(224,179,107);private val muted=Color.rgb(177,177,169)
    private val handler=Handler(Looper.getMainLooper());private val images=java.util.concurrent.ThreadPoolExecutor(2,2,0,java.util.concurrent.TimeUnit.SECONDS,java.util.concurrent.ArrayBlockingQueue(48),java.util.concurrent.ThreadPoolExecutor.DiscardOldestPolicy());private val searches=Executors.newSingleThreadExecutor()
    private lateinit var catalog:Catalog;private lateinit var store:ListeningStore
    private var browser:MediaBrowser?=null;private var connection:ListenableFuture<MediaBrowser>?=null
    private lateinit var root:LinearLayout;private var list:ListView?=null;private var status:TextView?=null;private var grant:Button?=null;private var mixLabel:TextView?=null
    private var titleView:TextView?=null;private var subtitle:TextView?=null;private var clock:TextView?=null;private var cover:ImageView?=null;private var seek:SeekBar?=null
    private val playButtons=mutableListOf<ImageButton>();private var previous:ImageButton?=null;private var next:ImageButton?=null
    private var bluetoothAction:(()->Unit)?=null
    private var sharingStatus:TextView?=null;private var sharingDialog:android.app.AlertDialog?=null
    private var scene=0;private var parentId="songs";private var query="";private var rows=emptyList<MediaItem>();private var adapter:Rows?=null
    private var dragging=false;private var artGeneration=0;private var artKey:String?=null;private var searchGeneration=0;private var voiceQuery:String?=null
    private var resumeTitle:TextView?=null;private var resumeArtist:TextView?=null;private var resumeName:TextView?=null;private var resumeImage:ImageView?=null;private var resumeButton:Button?=null
    private var playlistCatalog=false;private var savedTab=false;private var upcoming:LinearLayout?=null;private var upcomingKey="";private var queueButton:Button?=null;private var contextLabel:TextView?=null
    private var renderedQueue="";private var renderedRevision=-1L;private var searching=false
    private val searchTask=Runnable{showRows()}
    private val listener=object:Player.Listener {
        override fun onEvents(player:Player,events:Player.Events){renderPlayer();if(scene==3)showQueue()}
        override fun onPlayerError(error:PlaybackException){status?.text="File non disponibile. Il player prova il brano successivo."}
    }
    private val ticker=object:Runnable {override fun run(){renderProgress();renderMix();renderSharing();RatingTags.present(this@MainActivity);if(scene==1 && renderedRevision!=store.libraryRevision){renderedRevision=store.libraryRevision;buildContent()};handler.postDelayed(this,1000)}}
    private fun dp(n:Int)=(n*resources.displayMetrics.density).toInt()
    private fun text(value:String,size:Float,color:Int=Color.WHITE)=TextView(this).apply{text=value;textSize=size;setTextColor(color)}
    private fun button(value:String,action:()->Unit)=Button(this).apply{text=value;isAllCaps=false;setTextColor(gold);setBackgroundColor(Color.TRANSPARENT);minHeight=dp(48);setOnClickListener{action()}}
    private fun column()=LinearLayout(this).apply{orientation=LinearLayout.VERTICAL}
    private fun row()=LinearLayout(this).apply{orientation=LinearLayout.HORIZONTAL;gravity=Gravity.CENTER_VERTICAL}
    private fun heading(value:String)=text(value,22f).apply{setTypeface(null,Typeface.BOLD);setPadding(0,dp(10),0,dp(8))}
    private fun surface(view:View,radius:Int=14,fill:Int=panel){view.background=GradientDrawable().apply{setColor(fill);cornerRadius=dp(radius).toFloat()}}
    private fun icon(resource:Int,label:String,action:()->Unit)=ImageButton(this).apply{setImageResource(resource);imageTintList=android.content.res.ColorStateList.valueOf(gold);setPadding(dp(12),dp(12),dp(12),dp(12));setBackgroundColor(Color.TRANSPARENT);minimumWidth=dp(48);minimumHeight=dp(48);contentDescription=label;setOnClickListener{action()}}
    private fun primary(label:String,action:()->Unit)=button(label,action).apply{setTextColor(bg);textSize=16f;setTypeface(null,Typeface.BOLD);surface(this,10,gold)}
    private fun art(size:Int)=ImageView(this).apply{scaleType=ImageView.ScaleType.CENTER_CROP;setImageResource(R.drawable.ic_album);imageTintList=android.content.res.ColorStateList.valueOf(muted);setPadding(dp(4),dp(4),dp(4),dp(4));surface(this,10);clipToOutline=true;layoutParams=LinearLayout.LayoutParams(dp(size),dp(size))}
    private fun bindArt(view:ImageView,item:MediaItem?){val uri=item?.mediaMetadata?.artworkUri;view.tag=uri;view.setImageResource(R.drawable.ic_album);view.imageTintList=android.content.res.ColorStateList.valueOf(muted)
        if(uri==null)return
        images.execute{val bitmap=try{contentResolver.openInputStream(uri)?.use{BitmapFactory.decodeStream(it,null,BitmapFactory.Options().apply{inSampleSize=2})}}catch(_:Exception){null};handler.post{if(!isDestroyed && view.tag==uri && bitmap!=null){view.imageTintList=null;view.setPadding(0,0,0,0);view.setImageBitmap(bitmap)}}}
    }
    private fun contextRow():View {val box=row();surface(box,12);box.addView(icon(if(store.activeProfile==CarQueues.PHONE)R.drawable.ic_phone else R.drawable.ic_car,"Auto e code"){showCarProfiles()}.apply{id=R.id.car_profiles_open},LinearLayout.LayoutParams(dp(48),dp(48)));contextLabel=text("Telefono · Coda manuale",14f).apply{minimumHeight=dp(48);gravity=Gravity.CENTER_VERTICAL;maxLines=1;ellipsize=android.text.TextUtils.TruncateAt.END;setOnClickListener{showCarProfiles()}};box.addView(contextLabel,LinearLayout.LayoutParams(0,-2,1f));box.addView(icon(R.drawable.ic_chevron,"Scegli la coda"){showCarProfiles()},LinearLayout.LayoutParams(dp(48),dp(48)));return box}
    private fun entry(title:String,detail:String,resource:Int=R.drawable.ic_playlist,action:()->Unit):View {val box=row().apply{minimumHeight=dp(70);setPadding(0,dp(5),0,dp(5));isClickable=true;isFocusable=true;contentDescription="$title. $detail";setOnClickListener{action()}};box.addView(icon(resource,title,action),LinearLayout.LayoutParams(dp(48),dp(48)));val labels=column();labels.addView(text(title,16f).apply{maxLines=2});if(detail.isNotBlank())labels.addView(text(detail,13f,muted).apply{maxLines=2});box.addView(labels,LinearLayout.LayoutParams(0,-2,1f));box.addView(icon(R.drawable.ic_chevron,"Apri $title",action),LinearLayout.LayoutParams(dp(48),dp(48)));return box}
    override fun onCreate(savedInstanceState:Bundle?){
        super.onCreate(savedInstanceState);catalog=Catalog.get(this);store=ListeningStore.get(this)
        scene=savedInstanceState?.getInt("scene") ?: getPreferences(MODE_PRIVATE).getInt("scene",0);scene=scene.coerceIn(0,4)
        parentId=savedInstanceState?.getString("parent") ?: "songs";query=savedInstanceState?.getString("query") ?: ""
        if(intent.action=="android.media.action.MEDIA_PLAY_FROM_SEARCH")voiceQuery=intent.getStringExtra(android.app.SearchManager.QUERY).orEmpty()
        playlistCatalog=savedInstanceState?.getBoolean("playlistCatalog") ?: false;savedTab=savedInstanceState?.getBoolean("savedTab") ?: false
        buildContent()
        if(Build.VERSION.SDK_INT>=33)onBackInvokedDispatcher.registerOnBackInvokedCallback(android.window.OnBackInvokedDispatcher.PRIORITY_DEFAULT){navigateBack()}
        scan()
    }
    override fun onActivityResult(requestCode:Int,resultCode:Int,data:android.content.Intent?){
        super.onActivityResult(requestCode,resultCode,data)
        RatingTags.result(this,requestCode,resultCode)
    }
    private fun navigate(to:Int){(getSystemService(INPUT_METHOD_SERVICE) as InputMethodManager).hideSoftInputFromWindow(root.windowToken,0);scene=to;buildContent()}
    private fun buildContent(){
        images.queue.clear()
        searchGeneration++;resumeTitle=null;resumeArtist=null;resumeName=null;resumeImage=null;resumeButton=null;list=null;status=null;grant=null;mixLabel=null;titleView=null;subtitle=null;clock=null;cover=null;seek=null;previous=null;next=null;playButtons.clear();artKey=null;artGeneration++;renderedQueue="";upcoming=null;upcomingKey="";queueButton=null;contextLabel=null
        root=column().apply{setBackgroundColor(bg);setPadding(dp(16),0,dp(16),0)}
        root.setOnApplyWindowInsetsListener{v,insets->
            if(Build.VERSION.SDK_INT>=30){val bars=insets.getInsets(WindowInsets.Type.systemBars());v.setPadding(dp(16)+bars.left,bars.top,dp(16)+bars.right,bars.bottom)}
            else @Suppress("DEPRECATION") v.setPadding(dp(16)+insets.systemWindowInsetLeft,insets.systemWindowInsetTop,dp(16)+insets.systemWindowInsetRight,insets.systemWindowInsetBottom)
            insets
        }
        val header=row();header.addView(text("MUSICCRAFT",15f,gold).apply{setSingleLine();ellipsize=android.text.TextUtils.TruncateAt.END;gravity=Gravity.CENTER_VERTICAL;letterSpacing=.10f;setTypeface(null,Typeface.BOLD)},LinearLayout.LayoutParams(0,dp(56),1f))
        val share=column().apply{gravity=Gravity.CENTER;isClickable=true;isFocusable=true;contentDescription="Condividi con il computer";id=R.id.sharing_open;setOnClickListener{showSharing()};addView(text("Condividi",13f));sharingStatus=text("Spenta",10f,muted).apply{id=R.id.sharing_status};addView(sharingStatus)}
        header.addView(share,LinearLayout.LayoutParams(dp(78),dp(56)));header.addView(icon(R.drawable.ic_more,"Opzioni MusicCraft"){viewMenu(header)},LinearLayout.LayoutParams(dp(48),dp(56)));root.addView(header)
        when(scene){0->buildSearch();1->buildPlaylists();2->buildPlayer();3->buildQueue();4->buildCarProfiles()}
        if(scene==0 || scene==1)buildMiniPlayer()
        val tabs=row().apply{setPadding(0,dp(4),0,0)}
        for((index,label)in listOf(0 to "Cerca",1 to "Playlist",2 to "Player")){
            val selected=scene==index || index==2 && scene>=3
            val tab=column().apply{id=when(index){0->R.id.nav_search;1->R.id.nav_playlists;else->R.id.nav_player};gravity=Gravity.CENTER;isSelected=selected;isClickable=true;isFocusable=true;contentDescription="$label${if(selected)", selezionato" else ""}";setOnClickListener{if(index==1){playlistCatalog=false};navigate(index)}}
            val image=ImageView(this).apply{setImageResource(when(index){0->R.drawable.ic_search;1->R.drawable.ic_playlist;else->R.drawable.ic_player});imageTintList=android.content.res.ColorStateList.valueOf(if(selected)gold else muted);importantForAccessibility=View.IMPORTANT_FOR_ACCESSIBILITY_NO}
            tab.addView(image,LinearLayout.LayoutParams(dp(24),dp(24)));tab.addView(text(label,12f,if(selected)gold else muted).apply{gravity=Gravity.CENTER});val line=View(this).apply{setBackgroundColor(if(selected)gold else Color.TRANSPARENT)};tab.addView(line,LinearLayout.LayoutParams(dp(32),dp(2)).apply{topMargin=dp(4)});tabs.addView(tab,LinearLayout.LayoutParams(0,dp(60),1f))
        }
        root.addView(tabs);setContentView(root);root.requestApplyInsets();renderPlayer();renderSharing()
    }
    private fun viewMenu(anchor:View){val menu=PopupMenu(this,anchor);listOf("Aggiorna libreria","Condividi con il computer","Auto e code","Copertine","Voti nei tag","Informazioni").forEach{menu.menu.add(it)};menu.setOnMenuItemClickListener{when(it.title.toString()){"Aggiorna libreria"->scan();"Condividi con il computer"->showSharing();"Auto e code"->showCarProfiles();"Copertine"->showArtworkSettings();"Voti nei tag"->RatingTags.settings(this);else->android.app.AlertDialog.Builder(this).setTitle("MusicCraft ${BuildConfig.VERSION_NAME}").setMessage("Ricerca, playlist e player indipendenti.\n\nStatistiche e valutazioni rimangono sul dispositivo. Il server musicale condivide la libreria in sola lettura.").setPositiveButton("Chiudi",null).show()};true};menu.show()}
    private fun buildSearch(){
        root.addView(heading("Cerca nella tua musica"))
        val tabs=row();for((id,label)in listOf("songs" to "Brani","albums" to "Album","artists" to "Artisti"))tabs.addView(button(label){parentId=id;query="";buildContent()}.apply{isSelected=parentId==id;if(isSelected){surface(this,10);setTypeface(null,Typeface.BOLD)}},LinearLayout.LayoutParams(0,dp(48),1f));root.addView(tabs)
        val field=EditText(this).apply{id=R.id.library_search;setSingleLine(true);hint="Titolo, artista o album";contentDescription="Cerca nella libreria";setHintTextColor(muted);setTextColor(Color.WHITE);textSize=16f;setText(query);setPadding(dp(12),0,dp(12),0);background=GradientDrawable().apply{setColor(panel);cornerRadius=dp(10).toFloat()}}
        field.addTextChangedListener(object:TextWatcher{override fun beforeTextChanged(s:CharSequence?,start:Int,count:Int,after:Int){};override fun afterTextChanged(s:Editable?){};override fun onTextChanged(s:CharSequence?,start:Int,before:Int,count:Int){query=s?.toString().orEmpty();handler.removeCallbacks(searchTask);handler.postDelayed(searchTask,180)}});root.addView(field,LinearLayout.LayoutParams(-1,dp(48)))
        if(parentId !in listOf("songs","albums","artists")){root.addView(button("‹ Torna alla libreria"){parentId="songs";buildContent()})}
        status=text("Lettura della libreria…",12f,muted).apply{setPadding(0,dp(6),0,dp(4))};root.addView(status)
        val actions=row();actions.addView(button("Riproduci risultati"){replaceQueue(rows.filter{it.mediaMetadata.isPlayable==true}.map{it.mediaId})},LinearLayout.LayoutParams(0,dp(48),1f));actions.addView(button("+ In coda"){sendIds(rows.filter{it.mediaMetadata.isPlayable==true}.map{it.mediaId},"append")},LinearLayout.LayoutParams(0,dp(48),1f));root.addView(actions)
        grant=button("Consenti accesso alla musica"){requestPermissions(arrayOf(if(Build.VERSION.SDK_INT>=33)Manifest.permission.READ_MEDIA_AUDIO else Manifest.permission.READ_EXTERNAL_STORAGE),10)}.apply{visibility=if(catalog.permitted())View.GONE else View.VISIBLE};root.addView(grant)
        adapter=Rows();list=ListView(this).apply{id=R.id.library_list;dividerHeight=0;setAdapter(this@MainActivity.adapter);setOnItemClickListener{_,_,position,_->rows.getOrNull(position)?.let{select(it)}}};root.addView(list,LinearLayout.LayoutParams(-1,0,1f));showRows()
    }
    private fun showRows(){
        if(scene!=0 || isDestroyed)return
        if(!catalog.permitted()){rows=emptyList();adapter?.notifyDataSetChanged();status?.text="Autorizza l’accesso alla musica nelle opzioni Android.";return}
        val generation=++searchGeneration;val input=query;val parent=parentId;val songs=catalog.songs;searching=true
        searches.execute {
            val found=if(input.isNotBlank())songs.filter{LibraryRules.matches(it.title,it.artist,it.album,input)}.map{it.item()} else catalog.children(parent)
            handler.post{if(!isDestroyed && scene==0 && searchGeneration==generation){searching=false;rows=found;adapter?.notifyDataSetChanged();status?.text=if(found.isEmpty())"Nessun risultato" else "${found.size} risultati"}}
        }
    }
    private fun select(item:MediaItem){if(item.mediaMetadata.isBrowsable==true){parentId=item.mediaId;query="";buildContent();return};songActions(item)}
    private fun songActions(item:MediaItem){
        val actions=arrayOf("Riproduci ora","Riproduci dopo","Aggiungi in coda","Aggiungi a una playlist…","Valuta il brano…")
        android.app.AlertDialog.Builder(this).setTitle(item.mediaMetadata.title).setItems(actions){_,i->when(i){0->replaceQueue(listOf(item.mediaId));1->sendIds(listOf(item.mediaId),"next");2->sendIds(listOf(item.mediaId),"append");3->addToPlaylist(item.mediaId);4->rate(item.mediaId)}}.setNegativeButton("Chiudi",null).show()
    }
    private fun replaceQueue(ids:List<String>){if(ids.isEmpty()){toast("Nessun brano da riprodurre");return};confirmReplace{sendIds(ids,"replace");navigate(2)}}
    private fun confirmReplace(action:()->Unit){if((browser?.mediaItemCount ?: 0)>0)android.app.AlertDialog.Builder(this).setTitle("Sostituire la coda?").setMessage("Questa azione avvia la nuova selezione e sostituisce la coda attuale.").setNegativeButton("Annulla",null).setPositiveButton("Riproduci"){_,_->action()}.show() else action()}
    private fun sendIds(ids:List<String>,mode:String){if(ids.isEmpty()){toast("Nessun brano da aggiungere");return};command(Bundle().apply{putString("op","play_ids");putString("mode",mode);putStringArrayList("ids",ArrayList(ids))})}
    private fun command(args:Bundle,done:()->Unit={}){
        val b=browser ?: run{toast("Player in collegamento, riprova tra un momento");return}
        val future=b.sendCustomCommand(SessionCommand(PlaybackCommands.ACTION,Bundle.EMPTY),args)
        future.addListener({try{val r=future.get();if(r.resultCode!=SessionResult.RESULT_SUCCESS)toast(r.extras.getString("message") ?: "Operazione non riuscita") else done();renderPlayer();if(scene==3)showQueue()}catch(_:Exception){toast("Comando non disponibile. Riapri MusicCraft.")}}, {handler.post(it)})
    }
    private fun toast(value:String){Toast.makeText(this,value,Toast.LENGTH_LONG).show()}
    private fun buildMiniPlayer(){
        val bar=row().apply{setPadding(dp(8),dp(7),dp(4),dp(7));surface(this,12);setOnClickListener{navigate(2)};contentDescription="Apri il Player"}
        cover=art(44);bar.addView(cover)
        val labels=column().apply{setPadding(dp(10),0,0,0);setOnClickListener{navigate(2)}};titleView=text("Scegli un brano",14f).apply{maxLines=1;ellipsize=android.text.TextUtils.TruncateAt.END};subtitle=text("La tua musica",12f,muted).apply{maxLines=1;ellipsize=android.text.TextUtils.TruncateAt.END};labels.addView(titleView);labels.addView(subtitle);bar.addView(labels,LinearLayout.LayoutParams(0,-2,1f))
        val play=icon(R.drawable.ic_play,"Riproduci"){toggle()};playButtons+=play;bar.addView(play,LinearLayout.LayoutParams(dp(48),dp(48)));root.addView(bar)
    }
    private fun buildPlayer(){
        val landscape=resources.configuration.screenWidthDp>resources.configuration.screenHeightDp
        if(!landscape)root.addView(contextRow())
        val stage=if(landscape)row() else column();stage.id=R.id.player_stage
        val scroll=ScrollView(this).apply{isFillViewport=true}
        val content=column().apply{setPadding(0,dp(18),0,dp(10))}
        val summary=if(landscape)column() else row()
        val coverSize=if(landscape)160 else if(resources.configuration.screenWidthDp<360)116 else 148
        cover=art(coverSize).apply{id=R.id.player_cover;contentDescription="Copertina del brano"};summary.addView(cover)
        val labels=column().apply{setPadding(if(landscape)0 else dp(16),if(landscape)dp(12) else 0,0,0)}
        if(landscape){contextLabel=text("Telefono · Coda manuale",12f,gold).apply{maxLines=2};labels.addView(contextLabel)}
        titleView=text("Scegli un brano",22f).apply{id=R.id.player_title;maxLines=3;ellipsize=android.text.TextUtils.TruncateAt.END;setTypeface(null,Typeface.BOLD)};subtitle=text("Cerca nella tua libreria",14f,muted).apply{maxLines=3};labels.addView(titleView);labels.addView(subtitle)
        summary.addView(labels,if(landscape)LinearLayout.LayoutParams(-1,-2) else LinearLayout.LayoutParams(0,-2,1f))
        val controls=column().apply{setPadding(0,dp(14),0,0)}
        seek=SeekBar(this).apply{id=R.id.player_seek;max=1000;minimumHeight=dp(48);progressTintList=android.content.res.ColorStateList.valueOf(gold);thumbTintList=android.content.res.ColorStateList.valueOf(gold);contentDescription="Posizione di riproduzione";setOnSeekBarChangeListener(object:SeekBar.OnSeekBarChangeListener{override fun onProgressChanged(v:SeekBar?,p:Int,user:Boolean){};override fun onStartTrackingTouch(v:SeekBar?){dragging=true};override fun onStopTrackingTouch(v:SeekBar?){browser?.let{if(it.duration>0)it.seekTo(it.duration*progress/1000)};dragging=false}})};controls.addView(seek,LinearLayout.LayoutParams(-1,dp(48)))
        clock=text("0:00 / 0:00",12f,muted);controls.addView(clock)
        val transport=row().apply{gravity=Gravity.CENTER;setPadding(0,dp(8),0,dp(14))}
        previous=icon(R.drawable.ic_previous,"Brano precedente"){browser?.seekToPreviousMediaItem()};val play=icon(R.drawable.ic_play,"Riproduci"){toggle()}.apply{id=R.id.player_toggle;imageTintList=android.content.res.ColorStateList.valueOf(bg);surface(this,40,gold);setPadding(dp(18),dp(18),dp(18),dp(18))};playButtons+=play;next=icon(R.drawable.ic_next,"Brano successivo"){browser?.seekToNextMediaItem()}
        transport.addView(previous,LinearLayout.LayoutParams(dp(64),dp(64)));transport.addView(play,LinearLayout.LayoutParams(dp(72),dp(72)).apply{leftMargin=dp(24);rightMargin=dp(24)});transport.addView(next,LinearLayout.LayoutParams(dp(64),dp(64)));if(landscape)controls.addView(transport,0) else controls.addView(transport)
        upcoming=column();controls.addView(upcoming)
        queueButton=button("Coda · 0 brani"){navigate(3)}.apply{id=R.id.open_queue;gravity=Gravity.START or Gravity.CENTER_VERTICAL;textSize=16f;setCompoundDrawablesWithIntrinsicBounds(R.drawable.ic_playlist,0,R.drawable.ic_chevron,0);compoundDrawablePadding=dp(10)};controls.addView(queueButton,LinearLayout.LayoutParams(-1,dp(56)))
        controls.addView(button("Valuta il brano…"){browser?.currentMediaItem?.let{rate(it.mediaId)}}.apply{gravity=Gravity.START;textSize=13f})
        if(landscape){val left=ScrollView(this).apply{addView(summary)};stage.addView(left,LinearLayout.LayoutParams(0,-1,.42f));scroll.addView(controls);stage.addView(scroll,LinearLayout.LayoutParams(0,-1,.58f).apply{leftMargin=dp(22)})}else{content.addView(summary);content.addView(controls);scroll.addView(content);stage.addView(scroll,LinearLayout.LayoutParams(-1,-1))}
        root.addView(stage,LinearLayout.LayoutParams(-1,0,1f))
    }
    private fun toggle(){val b=browser ?: return;if(b.isPlaying)b.pause() else{if(b.playbackState==Player.STATE_ENDED)b.seekToDefaultPosition(0);b.prepare();b.play()}}
    private fun buildQueue(){
        root.addView(heading("Coda di ascolto"));mixLabel=text("Coda manuale",13f,gold);root.addView(mixLabel)
        val actions=row();actions.addView(button("‹ Player"){navigate(2)},LinearLayout.LayoutParams(0,dp(48),1f));actions.addView(button("Salva…"){nameDialog("Salva la coda"){name->command(Bundle().apply{putString("op","save_queue");putString("name",name)})}},LinearLayout.LayoutParams(0,dp(48),1f));actions.addView(button("Svuota"){android.app.AlertDialog.Builder(this).setTitle("Svuotare la coda?").setMessage("Ferma la riproduzione e la playlist dinamica.").setNegativeButton("Annulla",null).setPositiveButton("Svuota"){_,_->command(Bundle().apply{putString("op","clear")})}.show()},LinearLayout.LayoutParams(0,dp(48),1f));root.addView(actions)
        root.addView(button("Interrompi aggiunta automatica"){command(Bundle().apply{putString("op","stop_mix")})}.apply{id=R.id.stop_dynamic})
        status=text("I brani già aggiunti restano in coda",12f,muted);root.addView(status)
        adapter=Rows();list=ListView(this).apply{id=R.id.queue_list;dividerHeight=0;setAdapter(this@MainActivity.adapter);setOnItemClickListener{_,_,position,_->queueActions(position)}};root.addView(list,LinearLayout.LayoutParams(-1,0,1f));showQueue()
    }
    private fun showQueue(){if(scene!=3)return;val b=browser ?: return;val signature="${b.currentMediaItemIndex}:"+(0 until b.mediaItemCount).joinToString{b.getMediaItemAt(it).mediaId};if(signature!=renderedQueue){renderedQueue=signature;rows=(0 until b.mediaItemCount).map{b.getMediaItemAt(it)};adapter?.notifyDataSetChanged();status?.text="${rows.size} brani · tocca per spostare, rimuovere o riprodurre"}}
    private fun queueActions(index:Int){
        val b=browser ?: return;if(index !in 0 until b.mediaItemCount)return;val selected=b.getMediaItemAt(index);val ids=(0 until b.mediaItemCount).map{b.getMediaItemAt(it).mediaId}
        android.app.AlertDialog.Builder(this).setTitle(selected.mediaMetadata.title).setItems(arrayOf("Riproduci","Sposta prima","Sposta dopo","Rimuovi")){_,which->
            val current=browser ?: return@setItems
            if((0 until current.mediaItemCount).map{current.getMediaItemAt(it).mediaId}!=ids){toast("La coda è cambiata: scegli di nuovo il brano");return@setItems}
            command(Bundle().apply{when(which){0->{putString("op","jump");putInt("index",index)};1,2->{val to=if(which==1)index-1 else index+1;if(to !in ids.indices)return@setItems;putString("op","move");putInt("from",index);putInt("to",to)};else->{putString("op","remove");putInt("index",index)}}})
        }.setNegativeButton("Chiudi",null).show()
    }
    private fun buildPlaylists(){
        renderedRevision=store.libraryRevision
        if(playlistCatalog){buildPlaylistCatalog();return}
        root.addView(contextRow());val content=column();root.addView(ScrollView(this).apply{addView(content)},LinearLayout.LayoutParams(-1,0,1f))
        content.addView(heading("Riprendi l’ascolto"))
        val card=column().apply{setPadding(dp(14),dp(14),dp(14),dp(14));surface(this)};val summary=row();val image=art(88);resumeImage=image;bindArt(image,browser?.currentMediaItem);summary.addView(image)
        val labels=column().apply{setPadding(dp(12),0,0,0)};resumeName=text(browser?.sessionExtras?.getString("queue_name") ?: store.queue.name,17f).apply{setTypeface(null,Typeface.BOLD);maxLines=2};labels.addView(resumeName);resumeTitle=text("Nessun brano in coda",14f).apply{maxLines=1;ellipsize=android.text.TextUtils.TruncateAt.END};resumeArtist=text("Scegli un mix o una playlist",13f,muted).apply{maxLines=1};clock=text("",12f,muted);labels.addView(resumeTitle);labels.addView(resumeArtist);labels.addView(clock);summary.addView(labels,LinearLayout.LayoutParams(0,-2,1f));card.addView(summary)
        resumeButton=primary("Riprendi"){browser?.let{if(it.mediaItemCount>0){if(!it.isPlaying)toggle();navigate(2)}}}.apply{isEnabled=(browser?.mediaItemCount ?: 0)>0};card.addView(resumeButton,LinearLayout.LayoutParams(-1,dp(48)).apply{topMargin=dp(12)});content.addView(card)
        content.addView(heading("Scegli un mix"));val suggestions=(store.rules.filter{it.id=="unplayed"}+store.rules.filter{it.id!="unplayed"}).take(2)
        for(rule in suggestions)content.addView(entry(rule.name,ruleSummary(rule),R.drawable.ic_shuffle){previewRule(rule)})
        if(suggestions.isEmpty())content.addView(text("Crea una playlist dinamica con i tuoi criteri.",14f,muted))
        content.addView(entry("Tutte le playlist","Dinamiche e salvate"){playlistCatalog=true;buildContent()})
    }
    private fun buildPlaylistCatalog(){
        root.addView(button("‹ Playlist"){playlistCatalog=false;buildContent()}.apply{gravity=Gravity.START});root.addView(heading("Tutte le playlist"))
        val tabs=row();for((saved,label)in listOf(false to "Dinamiche",true to "Salvate"))tabs.addView(button(label){savedTab=saved;buildContent()}.apply{isSelected=saved==savedTab;if(isSelected)surface(this,10)},LinearLayout.LayoutParams(0,dp(48),1f));root.addView(tabs)
        val content=column();root.addView(ScrollView(this).apply{addView(content)},LinearLayout.LayoutParams(-1,0,1f))
        if(!savedTab){content.addView(text("Mix che si aggiornano durante l’ascolto, secondo i tuoi criteri.",14f,muted).apply{setPadding(0,dp(12),0,dp(8))});content.addView(button("+ Nuova playlist dinamica"){editRule(null)}.apply{id=R.id.create_dynamic;gravity=Gravity.START})
            for(rule in store.rules){val item=row();item.addView(entry(rule.name,ruleSummary(rule),R.drawable.ic_shuffle){previewRule(rule)},LinearLayout.LayoutParams(0,-2,1f));item.addView(icon(R.drawable.ic_more,"Opzioni ${rule.name}"){ruleActions(rule)},LinearLayout.LayoutParams(dp(48),dp(48)));content.addView(item)}
        }else{content.addView(button("+ Nuova playlist"){nameDialog("Nuova playlist"){store.putPlaylist(it,emptyList());buildContent()}}.apply{gravity=Gravity.START});if(store.playlists.isEmpty())content.addView(text("Salva la coda oppure aggiungi brani dalla ricerca.",14f,muted));for(playlist in store.playlists)content.addView(entry(playlist.name,"${playlist.ids.size} brani"){savedActions(playlist)})}
        store.error?.let{content.addView(text(it,13f,gold))}
    }
    private fun ruleSummary(r:SmartRule)=listOf(unitLabels[r.unit.ordinal],orderLabels[r.order.ordinal],playedLabels[r.played.ordinal],r.genre.takeIf{it.isNotBlank()},if(r.fromYear>0 || r.toYear>0)"${r.fromYear.takeIf{it>0} ?: "…"}–${r.toYear.takeIf{it>0} ?: "…"}" else null,if(r.minimumRating>0 || r.maximumRating<5)"★ ${r.minimumRating}–${r.maximumRating}" else null,if(r.minimumPlays>0 || r.maximumPlays!=null)"Riproduzioni ${r.minimumPlays}–${r.maximumPlays ?: "∞"}" else null,r.query.takeIf{it.isNotBlank()}).filterNotNull().joinToString(" · ")
    private val unitLabels=arrayOf("Brani","Album","Artisti")
    private val playedLabels=arrayOf("Tutti","Mai ascoltati","Ascoltati","Ascoltati in parte")
    private val orderLabels=arrayOf("Casuale","Più ascoltati (totale)","Meno ascoltati (totale)","Più ascoltati (media)","Meno ascoltati (media)","Voto medio più alto","Voto totale più alto","Più brani votati","Ascoltati di recente")
    private fun previewRule(rule:SmartRule){
        val songs=catalog.songs;val stats=store.stats
        searches.execute{val selected=SmartRules.select(songs.map{it.facts()},stats,rule);val byId=songs.associateBy{it.id};handler.post{if(isDestroyed)return@post
            val names=selected.take(100).map{byId[it.id]?.let{s->"${s.title} · ${s.artist}"} ?: it.title}.toTypedArray()
            android.app.AlertDialog.Builder(this).setTitle("${rule.name} · ${selected.size} brani").setItems(names,null).setNeutralButton("Modifica"){_,_->editRule(rule)}.setNegativeButton("Chiudi",null).setPositiveButton("Avvia mix"){_,_->confirmReplace{command(Bundle().apply{putString("op","start_mix");putString("id",rule.id)});navigate(2)}}.show()
        }}
    }
    private fun ruleActions(rule:SmartRule){android.app.AlertDialog.Builder(this).setTitle(rule.name).setItems(arrayOf("Anteprima e avvio","Modifica criteri","Elimina playlist")){_,i->when(i){0->previewRule(rule);1->editRule(rule);else->android.app.AlertDialog.Builder(this).setTitle("Eliminare ${rule.name}?").setNegativeButton("Annulla",null).setPositiveButton("Elimina"){_,_->if(browser?.sessionExtras?.getString("mix_id")==rule.id)command(Bundle().apply{putString("op","stop_mix")});store.removeRule(rule.id);buildContent()}.show()}}.show()}
    private fun savedActions(p:NamedPlaylist){android.app.AlertDialog.Builder(this).setTitle(p.name).setItems(arrayOf("Riproduci playlist","Aggiungi in coda","Rinomina","Elimina")){_,i->when(i){0->confirmReplace{command(Bundle().apply{putString("op","play_playlist");putString("id",p.id)});navigate(2)};1->sendIds(p.ids,"append");2->nameDialog("Rinomina playlist",p.name){store.putPlaylist(it,p.ids,p.id);buildContent()};else->{store.removePlaylist(p.id);buildContent()}}}.show()}
    private fun addToPlaylist(id:String){val existing=store.playlists;android.app.AlertDialog.Builder(this).setTitle("Aggiungi a playlist").setItems((existing.map{it.name}+"+ Nuova playlist").toTypedArray()){_,i->if(i==existing.size)nameDialog("Nuova playlist"){store.putPlaylist(it,listOf(id));toast("Brano aggiunto")} else{val p=existing[i];store.putPlaylist(p.name,p.ids+id,p.id);toast("Brano aggiunto")}}.show()}
    private fun nameDialog(label:String,initial:String="",save:(String)->Unit){val input=EditText(this).apply{setSingleLine();setText(initial);filters=arrayOf(android.text.InputFilter.LengthFilter(100))};val dialog=android.app.AlertDialog.Builder(this).setTitle(label).setView(input).setNegativeButton("Annulla",null).setPositiveButton("Salva",null).create();dialog.show();dialog.getButton(-1).setOnClickListener{val name=input.text.toString().trim();if(name.isBlank())input.error="Inserisci un nome" else{save(name);dialog.dismiss()}}}
    private fun rate(id:String){val song=catalog.songs.find{it.id==id} ?: return;val current=store.stats[song.statKey]?.rating ?: 0;android.app.AlertDialog.Builder(this).setTitle(song.title).setSingleChoiceItems(arrayOf("Senza voto","★","★★","★★★","★★★★","★★★★★"),current){dialog,value->command(Bundle().apply{putString("op","rate");putString("id",id);putInt("rating",value)});dialog.dismiss()}.setNegativeButton("Chiudi",null).show()}
    private fun editRule(existing:SmartRule?){
        val r=existing ?: SmartRule(UUID.randomUUID().toString(),"");val fields=column().apply{setPadding(dp(16),0,dp(16),0)}
        fun entry(label:String,value:String,numeric:Boolean=false):EditText{fields.addView(text(label,13f,muted));return EditText(this).apply{setSingleLine();setText(value);setTextColor(Color.WHITE);if(numeric)inputType=android.text.InputType.TYPE_CLASS_NUMBER;fields.addView(this)}}
        fun choice(label:String,values:Array<String>,selected:Int):Spinner{fields.addView(text(label,13f,muted));return Spinner(this).apply{adapter=ArrayAdapter(this@MainActivity,android.R.layout.simple_spinner_dropdown_item,values);setSelection(selected);fields.addView(this)}}
        val name=entry("Nome",r.name);val unit=choice("Scegli per",unitLabels,r.unit.ordinal);val order=choice("Ordine",orderLabels,r.order.ordinal);val played=choice("Ascolti",playedLabels,r.played.ordinal)
        val genres=listOf("Tutti i generi")+catalog.songs.map{it.genre}.filter{it.isNotBlank()}.distinct().sorted();val genre=choice("Genere",genres.toTypedArray(),genres.indexOf(r.genre).coerceAtLeast(0))
        val from=entry("Dall’anno (vuoto = qualsiasi)",r.fromYear.takeIf{it>0}?.toString().orEmpty(),true);val to=entry("All’anno (vuoto = qualsiasi)",r.toYear.takeIf{it>0}?.toString().orEmpty(),true)
        val decades=listOf("Nessun decennio preimpostato")+catalog.songs.map{it.year/10*10}.filter{it>0}.distinct().sorted().map{it.toString()};val decade=choice("Oppure scegli un decennio",decades.toTypedArray(),0);decade.onItemSelectedListener=object:AdapterView.OnItemSelectedListener{override fun onNothingSelected(parent:AdapterView<*>?){};override fun onItemSelected(parent:AdapterView<*>?,view:View?,position:Int,id:Long){decades[position].toIntOrNull()?.let{from.setText(it.toString());to.setText((it+9).toString())}}}
        val min=choice("Voto minimo",arrayOf("Senza limite","★","★★","★★★","★★★★","★★★★★"),r.minimumRating);val max=choice("Voto massimo",arrayOf("Solo senza voto","★","★★","★★★","★★★★","★★★★★"),r.maximumRating);val query=entry("Titolo, artista o album contiene",r.query)
        val minPlays=entry("Riproduzioni minime (vuoto = 0)",r.minimumPlays.takeIf{it>0}?.toString().orEmpty(),true)
        val maxPlays=entry("Riproduzioni massime (vuoto = nessun limite)",r.maximumPlays?.toString().orEmpty(),true)
        fields.addView(text("Un ascolto conta dopo 30 secondi o metà brano, se più breve; pause e salti non aggiungono tempo. I criteri modificati si applicano al prossimo avvio del mix.",12f,muted))
        val error=text("",13f,gold);fields.addView(error);val dialog=android.app.AlertDialog.Builder(this).setTitle(if(existing==null)"Nuova dinamica" else "Modifica dinamica").setView(ScrollView(this).apply{addView(fields)}).setNegativeButton("Annulla",null).setPositiveButton("Salva",null).create();dialog.show();dialog.getButton(-1).setOnClickListener{
            val minimumText=minPlays.text.toString().trim();val maximumText=maxPlays.text.toString().trim()
            val minimum=if(minimumText.isBlank())0L else minimumText.toLongOrNull()
            val maximum=if(maximumText.isBlank())null else maximumText.toLongOrNull()
            if(minimum==null || (maximumText.isNotBlank() && maximum==null)){error.text="Inserisci numeri di riproduzioni validi, senza decimali.";return@setOnClickListener}
            val result=SmartRule(r.id,name.text.toString().trim(),MixUnit.entries[unit.selectedItemPosition],MixOrder.entries[order.selectedItemPosition],PlayedFilter.entries[played.selectedItemPosition],if(genre.selectedItemPosition==0)"" else genres[genre.selectedItemPosition],from.text.toString().toIntOrNull() ?: 0,to.text.toString().toIntOrNull() ?: 0,min.selectedItemPosition,max.selectedItemPosition,query.text.toString().trim(),minimum,maximum)
            if(!result.valid())error.text="Controlla nome, anni, voti e intervallo di riproduzioni. Ascoltati in parte è disponibile per album e artisti." else{store.put(result);dialog.dismiss();buildContent()}
        }
    }
    private fun bluetoothPermission(action:()->Unit){
        if(CarRouteMonitor.permitted(this)){action();return}
        android.app.AlertDialog.Builder(this).setTitle("Riconosci l’auto tramite Bluetooth")
            .setMessage("MusicCraft leggerà i dispositivi Bluetooth già abbinati e collegati per richiamare la coda dell’auto che scegli. Nessuna scansione, posizione o invio dei nomi a server esterni. Puoi usare i profili manualmente senza questo permesso.")
            .setNegativeButton("Non ora",null).setPositiveButton("Continua"){_,_->bluetoothAction=action;requestPermissions(arrayOf(Manifest.permission.BLUETOOTH_CONNECT),12)}.show()
    }
    private fun showCarProfiles(){navigate(4)}
    private fun buildCarProfiles(){
        root.addView(button("‹ Player"){navigate(2)}.apply{gravity=Gravity.START});root.addView(heading("Auto e code"));val content=column();root.addView(ScrollView(this).apply{addView(content)},LinearLayout.LayoutParams(-1,0,1f))
        content.addView(text("Ogni auto conserva playlist, brano e posizione propri. Il cambio di coda non avvia la musica.",14f,muted))
        for(profile in listOf(CarProfile(CarQueues.PHONE,"Telefono"))+store.cars){val active=profile.id==store.activeProfile;val q=store.queues[profile.id] ?: QueueSnapshot();val count=if(active)browser?.mediaItemCount ?: q.ids.size else q.ids.size
            content.addView(entry(profile.name+(if(active)" · In uso" else ""),"${q.name} · $count brani",if(profile.id==CarQueues.PHONE)R.drawable.ic_phone else R.drawable.ic_car){carActions(profile)}.apply{isSelected=active;if(active)surface(this,12)})}
        content.addView(button("+ Nuova auto"){nameDialog("Nome dell’auto"){name->command(Bundle().apply{putString("op","put_car");putString("name",name)},::showCarProfiles)}}.apply{gravity=Gravity.START})
        content.addView(entry("Riconoscimento Bluetooth: "+if(store.automaticCars)"attivo" else "spento","Facoltativo · usa i dispositivi già abbinati",R.drawable.ic_car){if(store.automaticCars)command(Bundle().apply{putString("op","automatic_cars");putBoolean("enabled",false)},::showCarProfiles) else bluetoothPermission{command(Bundle().apply{putString("op","automatic_cars");putBoolean("enabled",true)},::showCarProfiles)}})
    }
    private fun showArtworkSettings(){
        val art=ArtworkLibrary.get(this);val layout=column().apply{setPadding(dp(24),dp(8),dp(24),dp(12))};val toggle=Switch(this).apply{text="Recupera le copertine mancanti";textSize=16f;isChecked=art.online;minimumHeight=dp(56);setOnCheckedChangeListener{_,enabled->art.online=enabled;artKey=null;renderPlayer()}};layout.addView(toggle)
        layout.addView(text("Prima le immagini locali. Se attivi il recupero, titolo, artista e album vengono cercati su MusicBrainz e Cover Art Archive. Solo per i brani visualizzati; nessun file musicale viene modificato.",14f,muted))
        val info=text("Archivio permanente…",13f,gold).apply{setPadding(0,dp(16),0,dp(8))};layout.addView(info);art.summary{info.text=it+"\nLe immagini sono conservate nei dati dell’app, anche svuotando la cache. Un’immagine per brano; album e immagini identiche vengono riutilizzati."}
        android.app.AlertDialog.Builder(this).setTitle("Copertine").setView(layout).setPositiveButton("Chiudi",null).show()
    }
    private fun carActions(car:CarProfile){
        val phone=car.id==CarQueues.PHONE || car.id==CarQueues.DEFAULT_CAR
        val actions=if(phone)arrayOf("Usa questa coda") else (listOf("Usa questa coda","Associa Bluetooth…","Rimuovi associazione Bluetooth","Rinomina")+if(store.activeProfile==car.id)emptyList() else listOf("Elimina profilo")).toTypedArray()
        android.app.AlertDialog.Builder(this).setTitle(car.name).setItems(actions){_,index->when(index){
            0->command(Bundle().apply{putString("op","select_car");putString("id",car.id)},::showCarProfiles)
            1->bluetoothPermission{
                val devices=CarRouteMonitor.paired(this)
                if(devices.isEmpty())toast("Nessun dispositivo abbinato. Abbina prima l’auto nelle impostazioni Bluetooth di Android.")
                else android.app.AlertDialog.Builder(this).setTitle("Bluetooth di ${car.name}").setItems(devices.map{it.second}.toTypedArray()){_,i->command(Bundle().apply{putString("op","put_car");putString("id",car.id);putString("name",car.name);putString("bluetooth",devices[i].first)},::showCarProfiles)}.setNegativeButton("Annulla",null).show()
            }
            2->command(Bundle().apply{putString("op","put_car");putString("id",car.id);putString("name",car.name)},::showCarProfiles)
            3->nameDialog("Nome dell’auto",car.name){name->command(Bundle().apply{putString("op","put_car");putString("id",car.id);putString("name",name);putString("bluetooth",car.bluetoothKey)},::showCarProfiles)}
            4->android.app.AlertDialog.Builder(this).setTitle("Eliminare ${car.name}?").setMessage("Viene rimossa la sua coda, senza cancellare brani o playlist. Il profilo attivo e quello Auto predefinito vanno conservati.").setNegativeButton("Annulla",null).setPositiveButton("Elimina"){_,_->command(Bundle().apply{putString("op","remove_car");putString("id",car.id)},::showCarProfiles)}.show()
        }}.setNegativeButton("Chiudi",null).show()
    }
    private fun renderSharing() {
        val value=if(!SharingGate.running)"Spenta" else if(SharingGate.localPlayback)"Sospesa" else "Attiva"
        findViewById<View>(R.id.sharing_open)?.contentDescription="Condividi con il computer, $value"
        sharingStatus?.let{if(it.text.toString()!=value)it.text=value;it.setTextColor(if(SharingGate.running)gold else muted)}
    }
    private fun showSharing() {
        sharingDialog?.takeIf{it.isShowing}?.let{return}
        val message=text("",15f).apply{id=R.id.sharing_panel;setPadding(dp(22),dp(12),dp(22),dp(8));accessibilityLiveRegion=View.ACCESSIBILITY_LIVE_REGION_POLITE}
        val dialog=android.app.AlertDialog.Builder(this).setTitle("Condivisione con il computer").setView(message)
            .setNegativeButton("Chiudi",null).setPositiveButton("Attiva",null).setNeutralButton("Revoca abbinamenti",null).create()
        sharingDialog=dialog
        var startingUntil=0L
        val refresh=object:Runnable {override fun run(){
            if(!dialog.isShowing || isDestroyed)return
            val running=SharingGate.running;val ready=SharingGate.pairLink!=null
            if(running)startingUntil=0L
            val starting=SystemClock.elapsedRealtime()<startingUntil
            val description=when {
                running && ready->SharingGate.state+"\n\nIl computer può ora trovare il telefono. Se è già abbinato si ricollega automaticamente. Per il primo collegamento premi Abbina computer, inserisci il codice sul Mac e autorizza qui la richiesta."
                running->SharingGate.state+"\n\nIl collegamento apparirà qui appena disponibile. Non occorre riaprire il menu."
                starting->"Avvio della condivisione…"
                else->"Condivisione spenta. Premi Attiva per rendere la musica disponibile al computer sulla stessa rete Wi-Fi.\n\nAccesso in sola lettura, sospeso mentre il telefono riproduce audio. Gli abbinamenti esistenti vengono conservati."
            }
            if(message.text.toString()!=description)message.text=description
            dialog.getButton(-1).apply{val label=if(running)"Abbina computer" else if(starting)"Avvio…" else "Attiva";if(text.toString()!=label)text=label;isEnabled=if(running)ready else !starting}
            dialog.getButton(-3).apply{val label=if(running)"Interrompi" else "Revoca abbinamenti";if(text.toString()!=label)text=label}
            renderSharing();handler.postDelayed(this,500)
        }}
        dialog.setOnDismissListener{handler.removeCallbacks(refresh);if(sharingDialog===dialog)sharingDialog=null}
        dialog.show()
        dialog.getButton(-1).setOnClickListener {
            if(SharingGate.running){if(SharingGate.pairLink!=null){dialog.dismiss();showPairing()}}
            else if(!catalog.permitted()){
                dialog.dismiss();requestPermissions(arrayOf(if(Build.VERSION.SDK_INT>=33)Manifest.permission.READ_MEDIA_AUDIO else Manifest.permission.READ_EXTERNAL_STORAGE),10)
            } else {
                startingUntil=SystemClock.elapsedRealtime()+5000
                try{startForegroundService(android.content.Intent(this,SharingService::class.java))}
                catch(_:RuntimeException){startingUntil=0;toast("Impossibile avviare la condivisione. Riapri MusicCraft e riprova.")}
                handler.removeCallbacks(refresh);handler.post(refresh)
            }
        }
        dialog.getButton(-3).setOnClickListener {
            if(SharingGate.running)stopService(android.content.Intent(this,SharingService::class.java))
            else android.app.AlertDialog.Builder(this).setTitle("Revocare tutti gli abbinamenti?")
                .setMessage("I computer già abbinati dovranno essere autorizzati nuovamente sul telefono.")
                .setNegativeButton("Annulla",null).setPositiveButton("Revoca"){_,_->getSharedPreferences("sharing_identity",MODE_PRIVATE).edit().remove("token").commit();toast("Abbinamenti revocati")}.show()
            handler.removeCallbacks(refresh);handler.post(refresh)
        }
        handler.post(refresh)
    }
    private fun showPairing() {
        PairingWindow.open()
        val code=TextView(this).apply {id=R.id.pairing_code;textSize=36f;gravity=Gravity.CENTER;setPadding(dp(12),dp(18),dp(12),dp(8));typeface=android.graphics.Typeface.MONOSPACE}
        val state=TextView(this).apply {textSize=15f;setPadding(dp(20),dp(8),dp(20),dp(16))}
        val layout=column().apply {addView(code);addView(state)}
        val dialog=android.app.AlertDialog.Builder(this).setTitle("Abbina un computer").setView(layout).setNegativeButton("Annulla",null).setPositiveButton("Autorizza",null).create()
        val refresh=object:Runnable {override fun run(){
            if(!dialog.isShowing) return
            code.text=PairingWindow.visibleCode() ?: "— — — —"
            state.text=when {PairingWindow.completed()->"Computer abbinato. Puoi chiudere questa finestra.";PairingWindow.pending()->"Richiesta ricevuta da MusicCraft sul computer. Premi Autorizza solo se hai inserito tu questo codice.";PairingWindow.visibleCode()==null->"Codice scaduto. Chiudi e riapri l’abbinamento.";else->"Scegli questo telefono sul computer e inserisci il codice. Il codice scade dopo due minuti."}
            dialog.getButton(android.app.AlertDialog.BUTTON_POSITIVE).apply {isEnabled=PairingWindow.pending() || PairingWindow.completed();text=if(PairingWindow.completed()) "Chiudi" else "Autorizza"}
            handler.postDelayed(this,500)
        }}
        dialog.setOnDismissListener {handler.removeCallbacks(refresh);PairingWindow.close()}
        dialog.show()
        dialog.getButton(android.app.AlertDialog.BUTTON_POSITIVE).setOnClickListener {if(PairingWindow.completed()) dialog.dismiss() else PairingWindow.approve()}
        handler.post(refresh)
    }
    override fun onStart(){super.onStart();val future=MediaBrowser.Builder(this,SessionToken(this,ComponentName(this,PlaybackService::class.java))).buildAsync();connection=future;future.addListener({if(connection===future && !isFinishing)try{browser=future.get();browser?.addListener(listener);if(scene==1)buildContent() else renderPlayer();tryVoicePlayback();if(scene==3)showQueue()}catch(_:Exception){toast("Servizio audio non disponibile. Riapri MusicCraft.")}}, {handler.post(it)});handler.post(ticker)}
    override fun onStop(){sharingDialog?.dismiss();handler.removeCallbacks(ticker);browser?.removeListener(listener);browser=null;connection?.let{androidx.media3.session.MediaController.releaseFuture(it)};connection=null;getPreferences(MODE_PRIVATE).edit().putInt("scene",scene).apply();super.onStop()}
    override fun onDestroy(){sharingDialog?.dismiss();artGeneration++;searchGeneration++;handler.removeCallbacks(searchTask);images.shutdownNow();searches.shutdownNow();super.onDestroy()}
    override fun onSaveInstanceState(outState:Bundle){outState.putBoolean("playlistCatalog",playlistCatalog);outState.putBoolean("savedTab",savedTab);outState.putInt("scene",scene);outState.putString("parent",parentId);outState.putString("query",query);super.onSaveInstanceState(outState)}
    override fun onRequestPermissionsResult(requestCode:Int,permissions:Array<out String>,grantResults:IntArray){super.onRequestPermissionsResult(requestCode,permissions,grantResults);if(requestCode==12){val action=bluetoothAction;bluetoothAction=null;if(CarRouteMonitor.permitted(this))action?.invoke() else toast("Permesso Bluetooth negato: puoi scegliere manualmente il profilo auto.")} else if(requestCode==10){buildContent();scan();if(Build.VERSION.SDK_INT>=33 && catalog.permitted() && checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS)!=PackageManager.PERMISSION_GRANTED)requestPermissions(arrayOf(Manifest.permission.POST_NOTIFICATIONS),11)}}
    override fun onNewIntent(intent:android.content.Intent){super.onNewIntent(intent);setIntent(intent);if(intent.action=="android.media.action.MEDIA_PLAY_FROM_SEARCH"){voiceQuery=intent.getStringExtra(android.app.SearchManager.QUERY).orEmpty();scan()}}
    private fun tryVoicePlayback(){val q=voiceQuery ?: return;if(browser==null || !catalog.permitted())return;voiceQuery=null;val items=catalog.songs.filter{LibraryRules.matches(it.title,it.artist,it.album,q)};if(items.isEmpty()){toast("Nessun risultato per la ricerca vocale");return};sendIds(items.map{it.id},"replace");navigate(2)}
    private fun navigateBack(){when{scene>=3->navigate(2);scene==1 && playlistCatalog->{playlistCatalog=false;buildContent()};scene!=0->navigate(0);parentId !in listOf("songs","albums","artists")-> {parentId="songs";query="";buildContent()};else->finish()}}
    @Deprecated("Legacy back dispatch") override fun onBackPressed(){navigateBack()}
    private fun scan(){if(!catalog.permitted()){status?.text="Consenti l’accesso alla musica per iniziare.";return};status?.text="Aggiornamento…";val future=catalog.refresh(true);future.addListener({if(!isDestroyed)try{future.get();if(scene==0)showRows();tryVoicePlayback()}catch(_:Exception){toast(catalog.lastError ?: "Lettura non riuscita")}}, {handler.post(it)})}
    private fun renderMix(){val b=browser ?: return;val name=b.sessionExtras.getString("mix_name");mixLabel?.text=listOf(b.sessionExtras.getString("queue_profile_name","Telefono"),b.sessionExtras.getString("queue_name","Coda manuale"),if(name.isNullOrBlank())"${b.mediaItemCount} brani" else "Dinamica attiva").joinToString(" · ");contextLabel?.text=listOf(b.sessionExtras.getString("queue_profile_name","Telefono"),b.sessionExtras.getString("queue_name","Coda manuale")).joinToString(" · ");findViewById<Button>(R.id.stop_dynamic)?.visibility=if(name.isNullOrBlank())View.GONE else View.VISIBLE}
    private fun renderPlayer(){
        val b=browser ?: return;val meta=b.currentMediaItem?.mediaMetadata;titleView?.text=meta?.title ?: "Scegli un brano";subtitle?.text=listOfNotNull(meta?.artist,meta?.albumTitle).joinToString(" · ").ifEmpty{"La musica continua anche uscendo dal Player"}
        for(button in playButtons){button.setImageResource(if(b.isPlaying)R.drawable.ic_pause else R.drawable.ic_play);button.contentDescription=if(b.isPlaying)"Pausa" else "Riproduci";button.isEnabled=b.mediaItemCount>0;button.alpha=if(button.isEnabled)1f else .35f};previous?.apply{isEnabled=b.hasPreviousMediaItem();alpha=if(isEnabled)1f else .35f};next?.apply{isEnabled=b.hasNextMediaItem();alpha=if(isEnabled)1f else .35f};seek?.isEnabled=b.currentMediaItem!=null
        resumeTitle?.text=meta?.title ?: "Nessun brano in coda";resumeArtist?.text=meta?.artist ?: "Scegli un mix o una playlist";resumeName?.text=b.sessionExtras.getString("queue_name","Coda manuale");resumeButton?.let{it.isEnabled=b.mediaItemCount>0;it.alpha=if(it.isEnabled)1f else .45f;it.text=if(b.isPlaying)"Apri il Player" else "Riprendi"}
        val key=meta?.artworkUri?.toString();if(key!=artKey){artKey=key;cover?.let{bindArt(it,b.currentMediaItem)};resumeImage?.let{bindArt(it,b.currentMediaItem)}}
        queueButton?.text="Coda · ${b.mediaItemCount} brani"
        val upcomingSignature="${b.currentMediaItemIndex}:${b.mediaItemCount}:${b.currentMediaItem?.mediaId}"
        if(upcoming!=null && upcomingKey!=upcomingSignature){upcomingKey=upcomingSignature;upcoming?.removeAllViews();upcoming?.addView(heading("A seguire"));val from=(b.currentMediaItemIndex+1).coerceAtLeast(0);for(i in from until minOf(from+2,b.mediaItemCount)){val item=b.getMediaItemAt(i);val row=row();val image=art(44);bindArt(image,item);row.addView(image);val labels=column().apply{setPadding(dp(12),0,0,0)};labels.addView(text(item.mediaMetadata.title.toString(),15f).apply{maxLines=1;ellipsize=android.text.TextUtils.TruncateAt.END});labels.addView(text(item.mediaMetadata.artist?.toString().orEmpty(),13f,muted).apply{maxLines=1});row.addView(labels,LinearLayout.LayoutParams(0,-2,1f));row.addView(icon(R.drawable.ic_more,"Opzioni brano ${i+1}"){queueActions(i)},LinearLayout.LayoutParams(dp(48),dp(48)));upcoming?.addView(row,LinearLayout.LayoutParams(-1,dp(64)))};if(from>=b.mediaItemCount)upcoming?.addView(text("Fine della coda",14f,muted))}
        renderProgress();renderMix()
    }
    private fun renderProgress(){val b=browser ?: return;val duration=b.duration.takeIf{it>0} ?: 0;if(!dragging)seek?.progress=if(duration>0)(b.currentPosition*1000/duration).toInt() else 0
        fun time(ms:Long)="${ms.coerceAtLeast(0)/60000}:${((ms.coerceAtLeast(0)/1000)%60).toString().padStart(2,'0')}"
        clock?.text="${time(b.currentPosition)} / ${time(duration)}"}
    private inner class Rows:BaseAdapter(){
        override fun getCount()=rows.size
        override fun getItem(position:Int)=rows[position]
        override fun getItemId(position:Int)=position.toLong()
        override fun getView(position:Int,convertView:View?,parent:ViewGroup?):View {
            val box=convertView as? LinearLayout ?: row().apply{descendantFocusability=ViewGroup.FOCUS_BLOCK_DESCENDANTS;setPadding(dp(4),dp(6),0,dp(6));minimumHeight=dp(72);addView(art(48));addView(column().apply{setPadding(dp(12),0,0,0);addView(text("",16f));addView(text("",13f,muted))},LinearLayout.LayoutParams(0,-2,1f));addView(icon(R.drawable.ic_more,"Opzioni brano"){},LinearLayout.LayoutParams(dp(48),dp(48)))}
            val item=rows[position];val meta=item.mediaMetadata;val current=scene==3 && browser?.currentMediaItemIndex==position;val labels=box.getChildAt(1) as LinearLayout
            (labels.getChildAt(0) as TextView).apply{text=(if(current)"In ascolto · " else if(scene==3)"${position+1}. " else "")+meta.title;maxLines=1;ellipsize=android.text.TextUtils.TruncateAt.END;setTextColor(if(current)gold else Color.WHITE)}
            (labels.getChildAt(1) as TextView).apply{text=if(meta.isBrowsable==true)"Apri raccolta" else listOfNotNull(meta.artist,meta.albumTitle).joinToString(" · ");maxLines=1;ellipsize=android.text.TextUtils.TruncateAt.END}
            val image=box.getChildAt(0) as ImageView
            if(image.tag!=meta.artworkUri || image.tag==null)bindArt(image,item)
            (box.getChildAt(2) as ImageButton).apply{contentDescription="Opzioni ${meta.title}";setOnClickListener{if(scene==3)queueActions(position) else select(item)}}
            box.isSelected=current;box.setBackgroundColor(if(current)panel else Color.TRANSPARENT);return box
        }
    }
}
