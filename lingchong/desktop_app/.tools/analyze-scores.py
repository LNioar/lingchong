import sqlite3,os,json,subprocess,struct,math,time,datetime
from pathlib import Path
p=Path(os.environ['APPDATA'])/'deskpet'/'deskpet.db'
c=sqlite3.connect(p.as_uri()+'?mode=ro',uri=True); c.row_factory=sqlite3.Row
profile=json.loads(c.execute("select value from profile where key='profile'").fetchone()[0])
vecs=[json.loads(v) for v in subprocess.check_output([str(Path('.tools/score-probe.exe').resolve())]+[i['topic'] for i in profile['interests']],encoding='utf-8').splitlines()]
now=int(time.time()); feedback={}
for r in c.execute('select hot_id,action from feedback where at>=?',(now-3*86400,)):feedback.setdefault(r['hot_id'],set()).add(r['action'])
rows=[]
for r in c.execute('select h.*,e.embedding from hot_items h join hot_embeddings e on h.id=e.id'):
    actions=feedback.get(r['id'],set())
    if 'dislike' in actions:continue
    emb=struct.unpack('<'+'f'*(len(r['embedding'])//4),r['embedding'])
    def cosine(a,b):return sum(x*y for x,y in zip(a,b))/math.sqrt(sum(x*x for x in a)*sum(y*y for y in b)) if len(a)==len(b) else 0
    sim=max([0]+[cosine(emb,i.get('embedding') or v)*i['weight'] for i,v in zip(profile['interests'],vecs)])
    heat=.5 if r['hot_value'] is None else (0 if r['hot_value']==0 else min(math.log(r['hot_value'])/20,1))
    fresh=1/(1+(now-r['first_seen'])/3600*.1)
    penalty=.3 if actions & {'ignore','dismiss','dislike'} else 1
    parts=[.6*sim,.2*heat,.15*fresh,.05*penalty]
    rows.append({'title':r['title'],'platform':r['platform'],'score':round(sum(parts),4),'parts':[round(x,4) for x in parts],'first_seen':r['first_seen']})
cut=int(datetime.datetime(2026,10,22,17,29,22,tzinfo=datetime.timezone(datetime.timedelta(hours=8))).timestamp())
recent=sorted([r for r in rows if r['first_seen']>=cut],key=lambda r:r['score'],reverse=True)
allrows=sorted(rows,key=lambda r:r['score'],reverse=True)
result={'time':datetime.datetime.now().isoformat(),'profile_interests':profile['interests'],'all_eligible':len(rows),'all_above_0_6':sum(r['score']>=.6 for r in rows),'since_last_scored_cycle':len(recent),'recent_above_0_6':sum(r['score']>=.6 for r in recent),'recent_top5':recent[:5],'all_top3':allrows[:3]}
Path('diagnostics/current-score-analysis.json').write_text(json.dumps(result,ensure_ascii=False,indent=2),encoding='utf-8')
