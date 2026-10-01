from pathlib import Path
import copy,json,subprocess,hashlib
root=Path('/workspace/cache/member-owned-current4e-20261001'); inp=root/'corpus'; inp.mkdir(exist_ok=True)
base={'format':'ournotes-deck.recommendation-request/1','execution':{'kind':'live','scoreId':1004,'gekisou':False,'play':{'kind':'theoreticalBest'}},'scenario':{'kind':'free','musicId':10},'context':json.loads((inp/'context-played.json').read_text()),'metric':{'kind':'score'},'goal':'dailyHighScore','seedLaw':{'atoms':[[1,1],[-1,3]],'provenance':'manual synthetic UTF-8 transport corpus; not native seed population'},'constraints':{'leader':1,'includeMembers':[1,2,3,4,5],'excludeMembers':[6,7],'noSnaps':True},'k':3,'strategy':{'kind':'exhaustive'},'limits':{'timeLimitMs':None,'maxCandidates':None,'cacheEntries':32}}
cases=[]
def add(name,q=None,**meta):
 q=copy.deepcopy(base if q is None else q)
 path=inp/('request-'+name+'.json'); path.write_text(json.dumps(q,ensure_ascii=False,indent=2)+'\n',encoding='utf-8'); cases.append({'name':name,'request':path.name,**meta})
add('ordinary-exhaustive',completion='Complete')
q=copy.deepcopy(base); q['execution']['gekisou']=True; q['goal']='gekisouScore'; q['seedLaw']['atoms']=[[1,9007199254740993],[-1,3],[1,2]]; add('gekisou-large-integer',q,completion='Complete',denominator='9007199254740998')
q=copy.deepcopy(base); q['execution']['gekisou']=True; q['goal']='stableTarget'; q['metric']={'kind':'scoreAtLeast','threshold':500000}; add('stable-target',q,completion='Complete')
q=copy.deepcopy(base); q['metric']={'kind':'cappedScore','threshold':500000}; q['goal']='stableTarget'; add('capped-target',q,completion='Complete')
q=copy.deepcopy(base); q['metric']={'kind':'scoreAndLifeAtLeast','threshold':500000,'minFinalLife':1}; q['goal']='stableTarget'; q['execution']['play']={'kind':'stream','stream':json.loads((inp/'play-ordinary.json').read_text())}; add('life-target',q,completion='Complete')
q=copy.deepcopy(base); q['metric']={'kind':'clientEventPoints','eventId':7}; q['goal']='eventFarming'; add('event-farming',q,completion='Complete')
q=copy.deepcopy(base); q['metric']={'kind':'conditionalClientEventItems','eventId':7,'resourceType':11,'resourceId':9}; q['goal']='eventFarming'; add('conditional-items',q,completion='Complete')
q=copy.deepcopy(base); q['execution']={'kind':'skip','scoreId':1004}; q['context']=json.loads((inp/'context-skip.json').read_text()); q['seedLaw']=None; q['goal']='skipFarming'; add('skip-farming',q,completion='Complete')
q=copy.deepcopy(base); q['execution']={'kind':'power','musicId':10,'eventParameter':False}; q['seedLaw']=None; q['metric']={'kind':'power'}; q['goal']='power'; q['constraints']['noSnaps']=False; add('power-paired-snaps',q,completion='Complete',requireSnap=True)
q=copy.deepcopy(base); q['execution']['gekisou']=True; q['goal']='gekisouScore'; q['constraints']['noSnaps']=False; q['limits']['maxCandidates']=200; add('live-paired-bounded',q,completion='TimedOut',requireSnap=True)
q=copy.deepcopy(base); q['strategy']={'kind':'candidate','powerSeeds':1,'proposals':10,'proposalSeed':9007199254740993}; q['limits']['maxCandidates']=8; add('candidate-budget',q,completion='TimedOut')
q=copy.deepcopy(base); q['limits']['timeLimitMs']=1; q['simulation']={'musicLengthMs':600000,'scoreMusicLengthMs':600000}; add('deadline-partial',q,completion='TimedOut',requirePartial=True)
q['limits']['timeLimitMs']=None; q['k']=24; add('deadline-same-input-oracle',q,completion='Complete')
for name,value in [('network-empty',[]),('network-null',None),('network-packet',[{'frame':1,'range':0,'rank':1,'percent':10}])]:
 q=copy.deepcopy(base); q['networkConfirmations']=value; add(name,q,error='Unsupported')
q=copy.deepcopy(base); q['simulation']={'liveFinishedFromFrame':0}; add('explicit-finished',q,error='Unsupported')
q=copy.deepcopy(base); q['simulation']={'liveFinishedFromFrame':None}; add('explicit-finished-null',q,error='Unsupported')
q=copy.deepcopy(base); q['constraints']['noSnap']=True; add('constraints-unknown-key',q,error='Input')
q=copy.deepcopy(base); q['newUnknownKey']=1; add('unknown-key',q,error='Input')
q=copy.deepcopy(base); q['execution']['unusedUnknown']=1; add('nested-unknown-key',q,error='Input')
q=copy.deepcopy(base); q['execution']['play']={'kind':'stream','stream':json.loads((inp/'play-ordinary.json').read_text())}; q['execution']['play']['stream']['assistt']=True; add('stream-unknown-key',q,error='Input')
q=copy.deepcopy(base); q['goal']='power'; add('goal-mismatch',q,error='Input')
q=copy.deepcopy(base); q['constraints']['leader']=999; add('unknown-member',q,error='Input')
# Raw duplicate tokens must never pass through JS JSON.parse/stringify.
p=inp/'request-duplicate-field.json'; txt=json.dumps(base,ensure_ascii=False).replace('"k": 3','"k": 3, "k": 2'); p.write_text(txt,encoding='utf-8'); cases.append({'name':'duplicate-field','request':p.name,'error':'Input'})
roster=json.loads((inp/'roster.json').read_text()); bad=copy.deepcopy(roster); bad['members'].append(copy.deepcopy(bad['members'][0])); (inp/'roster-duplicate-member.json').write_text(json.dumps(bad)+'\n'); cases.append({'name':'duplicate-member','request':'request-ordinary-exhaustive.json','roster':'roster-duplicate-member.json','error':'Input'})
bad=copy.deepcopy(roster); bad['snaps'].append(copy.deepcopy(bad['snaps'][0])); (inp/'roster-duplicate-snap.json').write_text(json.dumps(bad)+'\n'); cases.append({'name':'duplicate-snap','request':'request-ordinary-exhaustive.json','roster':'roster-duplicate-snap.json','error':'Input'})
# Separate master identity with two cards sharing one character: required cards cannot form a legal five-character deck.
data=json.loads((inp/'DeckData.json').read_text()); table=data['master']['MasterMemberCard']; idcol=table['columns'].index('_id'); charcol=table['columns'].index('_characterID'); next(r for r in table['rows'] if r[idcol]==2)[charcol]=1
(inp/'DeckData-duplicate-character.json').write_text(json.dumps(data)+'\n'); cases.append({'name':'duplicate-character-required','request':'request-ordinary-exhaustive.json','data':'DeckData-duplicate-character.json','completion':'Complete','requireNoResults':True,'requireZeroEvaluated':True})
for c in cases:
 c['requestSha256']=hashlib.sha256((inp/c['request']).read_bytes()).hexdigest()
(inp/'cases.json').write_text(json.dumps({'synthetic':True,'latestNativeCertified':False,'cases':cases},indent=2)+'\n')
out=root/'cli-results'; out.mkdir(exist_ok=True)
summary=[]
for c in cases:
 command=[str(root/'target-j4/release/ournotes-recommend'),'--data',str(inp/c.get('data','DeckData.json')),'--roster',str(inp/c.get('roster','roster.json')),'--request',str(inp/c['request'])]
 p=subprocess.run(command,capture_output=True,text=True,encoding='utf-8',timeout=180)
 (out/(c['name']+'.stdout')).write_text(p.stdout,encoding='utf-8'); (out/(c['name']+'.stderr')).write_text(p.stderr,encoding='utf-8')
 summary.append({'name':c['name'],'exit':p.returncode,'stdout':c['name']+'.stdout','stderr':c['name']+'.stderr'})
 print(c['name'],p.returncode,flush=True)
(out/'manifest.json').write_text(json.dumps(summary,indent=2)+'\n')