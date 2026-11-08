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
    private val bg=Color.rgb(16,21,25);private val panel=Color.rgb(27,36,41);private val gold=Color.rgb(224,179,107);private val muted=Color.rgb(159,173,182)
    private val handler=Handler(Looper.getMainLooper());private val images=Executors.newSingleThreadExecutor();private val searches=Executors.newSingleThreadExecutor()
    private lateinit var catalog:Catalog;private lateinit var store:ListeningStore
    private var browser:MediaBrowser?=null;private var connection:ListenableFuture<MediaBrowser>?=null
    private lateinit var root:LinearLayout;private var list:ListView?=null;private var status:TextView?=null;private var grant:Button?=null;private var mixLabel:TextView?=null
    private var titleView:TextView?=null;private var subtitle:TextView?=null;private var clock:TextView?=null;private var cover:ImageView?=null;private var seek:SeekBar?=null
    private val playButtons=mutableListOf<Button>();private var previous:Button?=null;private var next:Button?=null
    private var scene=0;private var parentId="songs";private var query="";private var rows=emptyList<MediaItem>();private var adapter:Rows?=null
    private var dragging=false;private var artGeneration=0;private var artKey:String?=null;private var searchGeneration=0;private var voiceQuery:String?=null
    private var renderedQueue="";private var renderedRevision=-1L;private var searching=false
    private val searchTask=Runnable{showRows()}
    private val listener=object:Player.Listener {
        override fun onEvents(player:Player,events:Player.Events){renderPlayer();if(scene==3)showQueue()}
        override fun onPlayerError(error:PlaybackException){status?.text="File non disponibile. Il player prova il brano successivo."}
    }
    private val ticker=object:Runnable {override fun run(){renderProgress();renderMix();if(scene==1 && renderedRevision!=store.libraryRevision){renderedRevision=store.libraryRevision;buildContent()};handler.postDelayed(this,1000)}}
    private fun dp(n:Int)=(n*resources.displayMetrics.density).toInt()
    private fun text(value:String,size:Float,color:Int=Color.WHITE)=TextView(this).apply{text=value;textSize=size;setTextColor(color)}
    private fun button(value:String,action:()->Unit)=Button(this).apply{text=value;isAllCaps=false;setTextColor(gold);setBackgroundColor(Color.TRANSPARENT);minHeight=dp(48);setOnClickListener{action()}}
    private fun column()=LinearLayout(this).apply{orientation=LinearLayout.VERTICAL}
    private fun row()=LinearLayout(this).apply{orientation=LinearLayout.HORIZONTAL;gravity=Gravity.CENTER_VERTICAL}
    private fun heading(value:String)=text(value,22f).apply{setTypeface(null,Typeface.BOLD);setPadding(0,dp(10),0,dp(8))}
    override fun onCreate(savedInstanceState:Bundle?){
        super.onCreate(savedInstanceState);catalog=Catalog.get(this);store=ListeningStore.get(this)
        scene=savedInstanceState?.getInt("scene") ?: getPreferences(MODE_PRIVATE).getInt("scene",0);scene=scene.coerceIn(0,3)
        parentId=savedInstanceState?.getString("parent") ?: "songs";query=savedInstanceState?.getString("query") ?: ""
        if(intent.action=="android.media.action.MEDIA_PLAY_FROM_SEARCH")voiceQuery=intent.getStringExtra(android.app.SearchManager.QUERY).orEmpty()
        buildContent()
        if(Build.VERSION.SDK_INT>=33)onBackInvokedDispatcher.registerOnBackInvokedCallback(android.window.OnBackInvokedDispatcher.PRIORITY_DEFAULT){navigateBack()}
        scan()
    }
    private fun navigate(to:Int){(getSystemService(INPUT_METHOD_SERVICE) as InputMethodManager).hideSoftInputFromWindow(root.windowToken,0);scene=to;buildContent()}
    private fun buildContent(){
        searchGeneration++;list=null;status=null;grant=null;mixLabel=null;titleView=null;subtitle=null;clock=null;cover=null;seek=null;previous=null;next=null;playButtons.clear();artKey=null;artGeneration++;renderedQueue=""
        root=column().apply{setBackgroundColor(bg);setPadding(dp(16),0,dp(16),0)}
        root.setOnApplyWindowInsetsListener{v,insets->
            if(Build.VERSION.SDK_INT>=30){val bars=insets.getInsets(WindowInsets.Type.systemBars());v.setPadding(dp(16)+bars.left,bars.top,dp(16)+bars.right,bars.bottom)}
            else @Suppress("DEPRECATION") v.setPadding(dp(16)+insets.systemWindowInsetLeft,insets.systemWindowInsetTop,dp(16)+insets.systemWindowInsetRight,insets.systemWindowInsetBottom)
            insets
        }
        val header=row();header.addView(text("MUSICCRAFT",15f,gold).apply{gravity=Gravity.CENTER_VERTICAL;letterSpacing=.14f;setTypeface(null,Typeface.BOLD)},LinearLayout.LayoutParams(0,dp(44),1f))
        header.addView(button("⋮"){viewMenu(header)}.apply{contentDescription="Opzioni MusicCraft"},LinearLayout.LayoutParams(dp(48),dp(44)));root.addView(header)
        when(scene){0->buildSearch();1->buildPlaylists();2->buildPlayer();3->buildQueue()}
        if(scene!=2 && scene!=3)buildMiniPlayer()
        val tabs=row()
        for((index,label)in listOf(0 to "Cerca",1 to "Playlist",2 to "Player"))tabs.addView(button(label){navigate(index)}.apply{id=when(index){0->R.id.nav_search;1->R.id.nav_playlists;else->R.id.nav_player};contentDescription="Apri $label";if(scene==index || index==2 && scene==3)setBackgroundColor(panel)},LinearLayout.LayoutParams(0,dp(52),1f))
        root.addView(tabs);setContentView(root);root.requestApplyInsets();renderPlayer()
    }
    private fun viewMenu(anchor:View){val menu=PopupMenu(this,anchor);listOf("Aggiorna libreria","Condividi con il computer","Informazioni").forEach{menu.menu.add(it)};menu.setOnMenuItemClickListener{when(it.title.toString()){"Aggiorna libreria"->scan();"Condividi con il computer"->showSharing();else->android.app.AlertDialog.Builder(this).setTitle("MusicCraft 0.3.0").setMessage("Ricerca, playlist e player indipendenti.\n\nStatistiche e valutazioni rimangono sul dispositivo. Il server musicale condivide la libreria in sola lettura.").setPositiveButton("Chiudi",null).show()};true};menu.show()}
    private fun buildSearch(){
        root.addView(heading("Cerca nella tua musica"))
        val tabs=row();for((id,label)in listOf("songs" to "Brani","albums" to "Album","artists" to "Artisti"))tabs.addView(button(label){parentId=id;query="";buildContent()}.apply{if(parentId==id)setBackgroundColor(panel)},LinearLayout.LayoutParams(0,dp(44),1f));root.addView(tabs)
        val field=EditText(this).apply{id=R.id.library_search;setSingleLine(true);hint="Titolo, artista o album";contentDescription="Cerca nella libreria";setHintTextColor(muted);setTextColor(Color.WHITE);textSize=16f;setText(query);setPadding(dp(12),0,dp(12),0);background=GradientDrawable().apply{setColor(panel);cornerRadius=dp(10).toFloat()}}
        field.addTextChangedListener(object:TextWatcher{override fun beforeTextChanged(s:CharSequence?,start:Int,count:Int,after:Int){};override fun afterTextChanged(s:Editable?){};override fun onTextChanged(s:CharSequence?,start:Int,before:Int,count:Int){query=s?.toString().orEmpty();handler.removeCallbacks(searchTask);handler.postDelayed(searchTask,180)}});root.addView(field,LinearLayout.LayoutParams(-1,dp(48)))
        if(parentId !in listOf("songs","albums","artists")){root.addView(button("‹ Torna alla libreria"){parentId="songs";buildContent()})}
        status=text("Lettura della libreria…",12f,muted).apply{setPadding(0,dp(6),0,dp(4))};root.addView(status)
        val actions=row();actions.addView(button("Riproduci risultati"){replaceQueue(rows.filter{it.mediaMetadata.isPlayable==true}.map{it.mediaId})},LinearLayout.LayoutParams(0,dp(44),1f));actions.addView(button("+ In coda"){sendIds(rows.filter{it.mediaMetadata.isPlayable==true}.map{it.mediaId},"append")},LinearLayout.LayoutParams(0,dp(44),1f));root.addView(actions)
        grant=button("Consenti accesso alla musica"){requestPermissions(arrayOf(if(Build.VERSION.SDK_INT>=33)Manifest.permission.READ_MEDIA_AUDIO else Manifest.permission.READ_EXTERNAL_STORAGE),10)}.apply{visibility=if(catalog.permitted())View.GONE else View.VISIBLE};root.addView(grant)
        adapter=Rows();list=ListView(this).apply{id=R.id.library_list;dividerHeight=0;setAdapter(this@MainActivity.adapter);setOnItemClickListener{_,_,position,_->rows.getOrNull(position)?.let{select(it)}}};root.addView(list,LinearLayout.LayoutParams(-1,0,1f));showRows()
    }
    private fun showRows(){
        if(scene!=0 || isDestroyed)return
        if(!catalog.permitted()){rows=emptyList();adapter?.notifyDataSetChanged();status?.text="Autorizza l’accesso alla musica nelle opzioni Android.";return}
        val generation=++searchGeneration;val input=query;val parent=parentId;val songs=catalog.songs;searching=true
        searches.execute {
            val found=if(input.isNotBlank())songs.filter{LibraryRules.matches(it.title,it.artist,it.album,input)}.map{it.item()} else catalog.children(parent)
            handler.post{if(!isDestroyed && scene==0 && searchGeneration==generation){searching=false;rows=found;adapter?.notifyDataSetChanged();status?.text=if(found.isEmpty())"Nessun risultato · la coda non cambia" else "${found.size} risultati · cercare non modifica la coda"}}
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
    private fun command(args:Bundle){
        val b=browser ?: run{toast("Player in collegamento, riprova tra un momento");return}
        val future=b.sendCustomCommand(SessionCommand(PlaybackCommands.ACTION,Bundle.EMPTY),args)
        future.addListener({try{val r=future.get();if(r.resultCode!=SessionResult.RESULT_SUCCESS)toast(r.extras.getString("message") ?: "Operazione non riuscita");renderPlayer();if(scene==3)showQueue()}catch(_:Exception){toast("Comando non disponibile. Riapri MusicCraft.")}}, {handler.post(it)})
    }
    private fun toast(value:String){Toast.makeText(this,value,Toast.LENGTH_LONG).show()}
    private fun buildMiniPlayer(){
        val bar=row().apply{setBackgroundColor(panel);setPadding(dp(10),0,0,0)};titleView=text("Scegli un brano",14f).apply{gravity=Gravity.CENTER_VERTICAL;maxLines=1;ellipsize=android.text.TextUtils.TruncateAt.END;setOnClickListener{navigate(2)}};bar.addView(titleView,LinearLayout.LayoutParams(0,dp(50),1f));val play=button("▶"){toggle()}.apply{contentDescription="Riproduci"};playButtons+=play;bar.addView(play,LinearLayout.LayoutParams(dp(54),dp(50)));root.addView(bar)
    }
    private fun buildPlayer(){
        val landscape=resources.configuration.screenWidthDp>resources.configuration.screenHeightDp
        val stage=if(landscape)row() else column();stage.id=R.id.player_stage
        val artBox=column().apply{gravity=Gravity.CENTER;setPadding(dp(8),dp(8),dp(8),dp(8))}
        cover=ImageView(this).apply{id=R.id.player_cover;scaleType=ImageView.ScaleType.FIT_CENTER;contentDescription="Copertina dell’album";setImageResource(R.mipmap.ic_launcher)};artBox.addView(cover,LinearLayout.LayoutParams(-1,-1))
        if(landscape)stage.addView(artBox,LinearLayout.LayoutParams(0,-1,.43f)) else stage.addView(artBox,LinearLayout.LayoutParams(-1,0,1f))
        val controls=column().apply{gravity=Gravity.CENTER_VERTICAL;setPadding(dp(8),dp(4),dp(8),dp(4))}
        titleView=text("Scegli un brano",if(landscape)22f else 25f).apply{id=R.id.player_title;maxLines=if(landscape)1 else 2;ellipsize=android.text.TextUtils.TruncateAt.END;setTypeface(null,Typeface.BOLD)};controls.addView(titleView)
        subtitle=text("La musica continua anche uscendo da questa vista",13f,muted).apply{maxLines=if(landscape)1 else 2};controls.addView(subtitle)
        seek=SeekBar(this).apply{id=R.id.player_seek;max=1000;contentDescription="Posizione di riproduzione";setOnSeekBarChangeListener(object:SeekBar.OnSeekBarChangeListener{override fun onProgressChanged(v:SeekBar?,p:Int,user:Boolean){};override fun onStartTrackingTouch(v:SeekBar?){dragging=true};override fun onStopTrackingTouch(v:SeekBar?){browser?.let{if(it.duration>0)it.seekTo(it.duration*progress/1000)};dragging=false}})};controls.addView(seek)
        clock=text("0:00 / 0:00",12f,muted);controls.addView(clock)
        val transport=row();previous=button("|◀"){browser?.seekToPreviousMediaItem()}.apply{contentDescription="Brano precedente"};val play=button("▶"){toggle()}.apply{id=R.id.player_toggle;textSize=30f;contentDescription="Riproduci"};playButtons+=play;next=button("▶|"){browser?.seekToNextMediaItem()}.apply{contentDescription="Brano successivo"}
        for(v in listOf(previous,play,next))transport.addView(v,LinearLayout.LayoutParams(0,dp(58),1f));controls.addView(transport)
        mixLabel=text("Coda manuale",12f,gold).apply{maxLines=if(landscape)1 else 2};controls.addView(mixLabel)
        val options=row();options.addView(button("Coda"){navigate(3)}.apply{id=R.id.open_queue},LinearLayout.LayoutParams(0,dp(48),1f));options.addView(button("★ Valuta"){browser?.currentMediaItem?.let{rate(it.mediaId)}},LinearLayout.LayoutParams(0,dp(48),1f));controls.addView(options)
        if(landscape){val scroll=ScrollView(this).apply{addView(controls)};stage.addView(scroll,LinearLayout.LayoutParams(0,-1,.57f))} else stage.addView(controls,LinearLayout.LayoutParams(-1,-2))
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
        root.addView(heading("Le tue playlist"));val content=column();val scroll=ScrollView(this).apply{addView(content)};root.addView(scroll,LinearLayout.LayoutParams(-1,0,1f))
        content.addView(text("DINAMICHE",12f,gold));content.addView(text("Scelgono e aggiungono musica secondo i criteri. Ascolti e voti sono locali a MusicCraft.",13f,muted))
        content.addView(button("+ Nuova playlist dinamica"){editRule(null)}.apply{id=R.id.create_dynamic})
        for(rule in store.rules){val item=row();item.addView(button(rule.name){previewRule(rule)},LinearLayout.LayoutParams(0,-2,1f));item.addView(button("⋮"){ruleActions(rule)}.apply{contentDescription="Opzioni ${rule.name}"},LinearLayout.LayoutParams(dp(48),-2));content.addView(item);content.addView(text(ruleSummary(rule),12f,muted))}
        content.addView(heading("Playlist salvate"));content.addView(button("+ Nuova playlist vuota"){nameDialog("Nuova playlist"){store.putPlaylist(it,emptyList());buildContent()}})
        if(store.playlists.isEmpty())content.addView(text("Aggiungi i brani dai risultati di ricerca oppure salva la coda.",13f,muted))
        for(playlist in store.playlists)content.addView(button("${playlist.name} · ${playlist.ids.size} brani"){savedActions(playlist)})
        store.error?.let{content.addView(text(it,13f,gold))}
    }
    private fun ruleSummary(r:SmartRule)=listOf(unitLabels[r.unit.ordinal],orderLabels[r.order.ordinal],playedLabels[r.played.ordinal],r.genre.takeIf{it.isNotBlank()},if(r.fromYear>0 || r.toYear>0)"${r.fromYear.takeIf{it>0} ?: "…"}–${r.toYear.takeIf{it>0} ?: "…"}" else null,if(r.minimumRating>0 || r.maximumRating<5)"★ ${r.minimumRating}–${r.maximumRating}" else null,r.query.takeIf{it.isNotBlank()}).filterNotNull().joinToString(" · ")
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
    private fun savedActions(p:NamedPlaylist){android.app.AlertDialog.Builder(this).setTitle(p.name).setItems(arrayOf("Riproduci playlist","Aggiungi in coda","Rinomina","Elimina")){_,i->when(i){0->replaceQueue(p.ids);1->sendIds(p.ids,"append");2->nameDialog("Rinomina playlist",p.name){store.putPlaylist(it,p.ids,p.id);buildContent()};else->{store.removePlaylist(p.id);buildContent()}}}.show()}
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
        fields.addView(text("Un ascolto conta dopo 30 secondi o metà brano, se più breve; pause e salti non aggiungono tempo. I criteri modificati si applicano al prossimo avvio del mix.",12f,muted))
        val error=text("",13f,gold);fields.addView(error);val dialog=android.app.AlertDialog.Builder(this).setTitle(if(existing==null)"Nuova dinamica" else "Modifica dinamica").setView(ScrollView(this).apply{addView(fields)}).setNegativeButton("Annulla",null).setPositiveButton("Salva",null).create();dialog.show();dialog.getButton(-1).setOnClickListener{
            val result=SmartRule(r.id,name.text.toString().trim(),MixUnit.entries[unit.selectedItemPosition],MixOrder.entries[order.selectedItemPosition],PlayedFilter.entries[played.selectedItemPosition],if(genre.selectedItemPosition==0)"" else genres[genre.selectedItemPosition],from.text.toString().toIntOrNull() ?: 0,to.text.toString().toIntOrNull() ?: 0,min.selectedItemPosition,max.selectedItemPosition,query.text.toString().trim())
            if(!result.valid())error.text="Controlla nome, anni e voti. Ascoltati in parte è disponibile per album e artisti." else{store.put(result);dialog.dismiss();buildContent()}
        }
    }
    private fun showSharing() {
        val info=android.app.AlertDialog.Builder(this).setTitle("Condivisione con il computer")
        val link=SharingGate.pairLink
        val message=if(SharingGate.running) SharingGate.state+"\n\n"+(if(link!=null)"Sul computer apri Libreria → Aggiungi sorgente → MusicCraft Android. Premi Abbina computer per mostrare il codice di 4 cifre, poi conferma la richiesta qui sul telefono." else "Collega il telefono alla stessa rete Wi-Fi del computer.") else "Condividi la libreria in sola lettura sulla rete Wi-Fi. La condivisione si sospende durante la riproduzione sul telefono e riprende quando il telefono torna inattivo. Rimane visibile una notifica per interromperla."
        info.setMessage(message).setNegativeButton("Chiudi",null)
        if(!SharingGate.running) {
            info.setPositiveButton("Attiva") {_,_->if(catalog.permitted()) {startForegroundService(android.content.Intent(this,SharingService::class.java))} else {requestPermissions(arrayOf(if(Build.VERSION.SDK_INT>=33)Manifest.permission.READ_MEDIA_AUDIO else Manifest.permission.READ_EXTERNAL_STORAGE),10);Unit}}
            info.setNeutralButton("Revoca abbinamenti") {_,_->android.app.AlertDialog.Builder(this).setTitle("Revocare tutti gli abbinamenti?").setMessage("I computer già abbinati dovranno ricevere un nuovo collegamento dal telefono.").setNegativeButton("Annulla",null).setPositiveButton("Revoca") {_,_->getSharedPreferences("sharing_identity",MODE_PRIVATE).edit().remove("token").commit();Toast.makeText(this,"Abbinamenti revocati",Toast.LENGTH_SHORT).show()}.show()}
        }
        else {
            info.setNeutralButton("Interrompi") {_,_->stopService(android.content.Intent(this,SharingService::class.java));Unit}
            if(link!=null) info.setPositiveButton("Abbina computer") {_,_->showPairing()}
        }
        info.show()
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
    override fun onStart(){super.onStart();val future=MediaBrowser.Builder(this,SessionToken(this,ComponentName(this,PlaybackService::class.java))).buildAsync();connection=future;future.addListener({if(connection===future && !isFinishing)try{browser=future.get();browser?.addListener(listener);renderPlayer();tryVoicePlayback();if(scene==3)showQueue()}catch(_:Exception){toast("Servizio audio non disponibile. Riapri MusicCraft.")}}, {handler.post(it)});handler.post(ticker)}
    override fun onStop(){handler.removeCallbacks(ticker);browser?.removeListener(listener);browser=null;connection?.let{androidx.media3.session.MediaController.releaseFuture(it)};connection=null;getPreferences(MODE_PRIVATE).edit().putInt("scene",scene).apply();super.onStop()}
    override fun onDestroy(){artGeneration++;searchGeneration++;handler.removeCallbacks(searchTask);images.shutdownNow();searches.shutdownNow();super.onDestroy()}
    override fun onSaveInstanceState(outState:Bundle){outState.putInt("scene",scene);outState.putString("parent",parentId);outState.putString("query",query);super.onSaveInstanceState(outState)}
    override fun onRequestPermissionsResult(requestCode:Int,permissions:Array<out String>,grantResults:IntArray){super.onRequestPermissionsResult(requestCode,permissions,grantResults);if(requestCode==10){buildContent();scan();if(Build.VERSION.SDK_INT>=33 && catalog.permitted() && checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS)!=PackageManager.PERMISSION_GRANTED)requestPermissions(arrayOf(Manifest.permission.POST_NOTIFICATIONS),11)}}
    override fun onNewIntent(intent:android.content.Intent){super.onNewIntent(intent);setIntent(intent);if(intent.action=="android.media.action.MEDIA_PLAY_FROM_SEARCH"){voiceQuery=intent.getStringExtra(android.app.SearchManager.QUERY).orEmpty();scan()}}
    private fun tryVoicePlayback(){val q=voiceQuery ?: return;if(browser==null || !catalog.permitted())return;voiceQuery=null;val items=catalog.songs.filter{LibraryRules.matches(it.title,it.artist,it.album,q)};if(items.isEmpty()){toast("Nessun risultato per la ricerca vocale");return};sendIds(items.map{it.id},"replace");navigate(2)}
    private fun navigateBack(){when{scene==3->navigate(2);scene!=0->navigate(0);parentId !in listOf("songs","albums","artists")-> {parentId="songs";query="";buildContent()};else->finish()}}
    @Deprecated("Legacy back dispatch") override fun onBackPressed(){navigateBack()}
    private fun scan(){if(!catalog.permitted()){status?.text="Consenti l’accesso alla musica per iniziare.";return};status?.text="Aggiornamento…";val future=catalog.refresh(true);future.addListener({if(!isDestroyed)try{future.get();if(scene==0)showRows();tryVoicePlayback()}catch(_:Exception){toast(catalog.lastError ?: "Lettura non riuscita")}}, {handler.post(it)})}
    private fun renderMix(){val b=browser ?: return;val name=b.sessionExtras.getString("mix_name");mixLabel?.text=if(name.isNullOrBlank())"Coda manuale · ${b.mediaItemCount} brani" else "Dinamica attiva: $name";findViewById<Button>(R.id.stop_dynamic)?.isEnabled=!name.isNullOrBlank()}
    private fun renderPlayer(){
        val b=browser ?: return;val meta=b.currentMediaItem?.mediaMetadata;titleView?.text=meta?.title ?: "Scegli un brano";subtitle?.text=listOfNotNull(meta?.artist,meta?.albumTitle).joinToString(" · ").ifEmpty{"La musica continua anche uscendo dal Player"}
        for(button in playButtons){button.text=if(b.isPlaying)"Ⅱ" else "▶";button.contentDescription=if(b.isPlaying)"Pausa" else "Riproduci";button.isEnabled=b.mediaItemCount>0};previous?.isEnabled=b.hasPreviousMediaItem();next?.isEnabled=b.hasNextMediaItem();seek?.isEnabled=b.currentMediaItem!=null
        val key=meta?.artworkUri?.toString();if(cover!=null && key!=artKey){artKey=key;val generation=++artGeneration;cover?.setImageResource(R.mipmap.ic_launcher);if(key!=null)images.execute{val bitmap=try{contentResolver.openInputStream(android.net.Uri.parse(key))?.use{BitmapFactory.decodeStream(it,null,BitmapFactory.Options().apply{inSampleSize=2})}}catch(_:Exception){null};handler.post{if(generation==artGeneration && !isDestroyed && bitmap!=null)cover?.setImageBitmap(bitmap)}}};renderProgress();renderMix()
    }
    private fun renderProgress(){val b=browser ?: return;val duration=b.duration.takeIf{it>0} ?: 0;if(!dragging)seek?.progress=if(duration>0)(b.currentPosition*1000/duration).toInt() else 0
        fun time(ms:Long)="${ms.coerceAtLeast(0)/60000}:${((ms.coerceAtLeast(0)/1000)%60).toString().padStart(2,'0')}"
        clock?.text="${time(b.currentPosition)} / ${time(duration)}"}
    private inner class Rows:BaseAdapter(){override fun getCount()=rows.size;override fun getItem(position:Int)=rows[position];override fun getItemId(position:Int)=position.toLong();override fun getView(position:Int,convertView:View?,parent:ViewGroup?):View{
        val box=convertView as? LinearLayout ?: column().apply{setPadding(dp(10),dp(10),dp(10),dp(10));minimumHeight=dp(66);addView(text("",16f));addView(text("",12f,muted))};val item=rows[position];val m=item.mediaMetadata
        (box.getChildAt(0) as TextView).apply{text=(if(scene==3)"${position+1}. " else "")+m.title;maxLines=1;ellipsize=android.text.TextUtils.TruncateAt.END}
        (box.getChildAt(1) as TextView).apply{text=if(m.isBrowsable==true)"Apri raccolta ›" else listOfNotNull(m.artist,m.albumTitle).joinToString(" · ");maxLines=1;ellipsize=android.text.TextUtils.TruncateAt.END};box.setBackgroundColor(if(scene==3 && browser?.currentMediaItemIndex==position)panel else Color.TRANSPARENT);return box
    }}
}
