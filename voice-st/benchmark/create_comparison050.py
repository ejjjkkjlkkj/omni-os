from pathlib import Path
import base64, hashlib, io, json, random, statistics, wave
import numpy as np
import soundfile as sf
root=Path(r'C:\st'); bench=root/'benchmark'; out=root/'release/st-0.5.0-windows-x64'
out.mkdir(parents=True,exist_ok=True)
base=json.loads((bench/'runs/baseline041-comparison/metrics.json').read_text())
new=json.loads((bench/'runs/candidate050-comparison/metrics.json').read_text())
groups={'ST 0.4.1':[r for r in base if r['engine']=='st'],'ST 0.5.0':new,'eSpeak NG 1.52.0':[r for r in base if r['engine']=='espeak'],'Microsoft Hortense / Zira':[r for r in base if r['engine']=='sapi5']}
summary=[]
for name,rows in groups.items():
 assert len(rows)==22 and all(r['status']=='PASS' for r in rows)
 summary.append({'engine':name,'phrases':len(rows),'hnr_mean_db':round(statistics.mean(r['hnr_db'] for r in rows),2),'duration_mean_s':round(statistics.mean(r['duration_s'] for r in rows),2),'max_clipping_ratio':max(r['clipping_ratio'] for r in rows)})
report={'summary':summary,'tests_passed':29,'source_archive_sha256':hashlib.sha256(Path(r'C:\Users\adm\Downloads\espeak-ng-1.52.0.zip').read_bytes()).hexdigest(),'reference_espeak_version':'1.52.0 (runtime espeak_Info)','reference_note':'Existing local binary, not rebuilt from supplied source archive. SHA256 captured by benchmark.','limitations':['Historical corpus contains French text without accents.','HNR is not an intelligibility or naturalness score.','Process timings include startup and PowerShell COM initialization: no engine speed ranking.','No listener scores collected.','SAPI desktop voices only; does not represent all Microsoft voices.'],'records':base+new}
(out/'COMPARISON.json').write_text(json.dumps(report,ensure_ascii=False,indent=2),encoding='utf-8')
items=[]
rng=random.Random(50926)
for idx,row in enumerate(new):
 recordings=[]
 for name,rows in groups.items():
  rec=next(r for r in rows if r['phrase_id']==row['phrase_id'])
  x,sr=sf.read(bench/rec['path']); x=np.asarray(x,dtype=float)
  if x.ndim>1: x=x.mean(axis=1)
  active=x[np.abs(x)>max(.001,np.max(np.abs(x))*.03)]
  rms=np.sqrt(np.mean(active*active)); gain=min(.12/max(rms,1e-8), .89/max(np.max(np.abs(x)),1e-8))
  pcm=np.round(np.clip(x*gain,-1,1)*32767).astype('<i2').tobytes()
  buf=io.BytesIO()
  with wave.open(buf,'wb') as w:
   w.setnchannels(1);w.setsampwidth(2);w.setframerate(sr);w.writeframes(pcm)
  recordings.append({'name':name,'audio':'data:audio/wav;base64,'+base64.b64encode(buf.getvalue()).decode()})
 rng.shuffle(recordings)
 items.append({'text':row['text'],'lang':row['lang'],'recordings':recordings})
html='''<!doctype html><html lang="fr"><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>ST 0.5 — Comparaison vocale</title>
<style>body{font:18px system-ui;background:#111827;color:#eef2ff;max-width:1100px;margin:40px auto;padding:20px}button,select{font:inherit;padding:10px;margin:6px;border-radius:6px}article{background:#1f2937;padding:20px;border-radius:12px;margin:18px 0}audio{width:100%}.grid{display:grid;grid-template-columns:repeat(auto-fit,minmax(230px,1fr));gap:16px}table{border-collapse:collapse}td,th{padding:10px;border-bottom:1px solid #566}p{line-height:1.6}small{color:#bdc9de}</style>
<h1>ST 0.5.0 · Écouter avant de conclure</h1><p>22 phrases, quatre moteurs : ST 0.4.1, ST 0.5.0, eSpeak NG 1.52.0 et Microsoft Hortense/Zira. Ordre masqué par phrase. Volume rapproché par RMS actif avec limite de crête ; débit inchangé. Les sons sont intégrés, sans réseau.</p>
<p>Évaluez l’intelligibilité puis le naturel. La préférence sélectionnée reste dans cette page et peut être exportée. Révéler les moteurs met fin à l’écoute aveugle de la phrase.</p>
<div id="stats"></div><p><small>HNR : harmonicité estimée, pas une note de qualité. Aucun écrêtage détecté. Le corpus historique manque d’accents français. Les temps de lancement, particulièrement PowerShell/SAPI, ne permettent pas de classer la vitesse des moteurs. Aucune supériorité perceptive n’est démontrée.</small></p>
<label for="phrase">Phrase</label><select id="phrase"></select><button id="prev">Précédente</button><button id="next">Suivante</button><article><h2 id="text"></h2><div class="grid" id="clips"></div><button id="reveal">Révéler les moteurs</button></article><button id="export">Exporter mes préférences JSON</button><p id="state" aria-live="polite"></p>
<script>const data=DATA; const summary=SUMMARY; const votes={};let index=0;
const selector=document.getElementById('phrase');data.forEach((r,i)=>{const o=document.createElement('option');o.value=i;o.textContent=(i+1)+' · '+r.lang+' · '+r.text;selector.append(o)});
document.getElementById('stats').innerHTML='<table><caption>Mesures sur les fichiers originaux</caption><tr><th>Moteur</th><th>HNR moyen (dB)</th><th>Durée moyenne (s)</th></tr>'+summary.map(r=>'<tr><td>'+r.engine+'</td><td>'+r.hnr_mean_db+'</td><td>'+r.duration_mean_s+'</td></tr>').join('')+'</table>';
function show(){selector.value=index;document.getElementById('text').textContent=data[index].text;const box=document.getElementById('clips');box.replaceChildren();data[index].recordings.forEach((r,i)=>{const div=document.createElement('div');const h=document.createElement('h3');h.textContent='Voix '+String.fromCharCode(65+i);h.dataset.name=r.name;const a=document.createElement('audio');a.controls=true;a.preload='none';a.src=r.audio;a.setAttribute('aria-label',h.textContent);a.onplay=()=>document.querySelectorAll('audio').forEach(other=>{if(other!==a)other.pause()});const b=document.createElement('button');b.textContent='Je préfère '+String.fromCharCode(65+i);b.onclick=()=>{votes[index]={text:data[index].text,preferred:r.name};document.getElementById('state').textContent='Préférence enregistrée pour la phrase '+(index+1)};div.append(h,a,b);box.append(div)});}
selector.onchange=()=>{index=+selector.value;show()};document.getElementById('prev').onclick=()=>{index=(index+data.length-1)%data.length;show()};document.getElementById('next').onclick=()=>{index=(index+1)%data.length;show()};document.getElementById('reveal').onclick=()=>document.querySelectorAll('h3').forEach(h=>h.textContent=h.dataset.name);document.getElementById('export').onclick=()=>{const blob=new Blob([JSON.stringify(votes,null,2)],{type:'application/json'});const url=URL.createObjectURL(blob);const a=document.createElement('a');a.href=url;a.download='preferences-st050.json';a.click();setTimeout(()=>URL.revokeObjectURL(url),1000)};show();</script></html>'''
html=html.replace('DATA',json.dumps(items,ensure_ascii=False).replace('</','<\\/')).replace('SUMMARY',json.dumps(summary,ensure_ascii=False))
(out/'ECOUTER.html').write_text(html,encoding='utf-8')
print(json.dumps(summary,ensure_ascii=False,indent=2))
print('Comparison HTML bytes:',len(html.encode()))
