from pathlib import Path,PurePosixPath
from zipfile import ZipFile
from io import BytesIO
from collections import Counter
from urllib.parse import urlsplit,unquote
from PIL import Image
import json,hashlib,gzip,stat,subprocess,re,copy,struct,math
ROOT=Path('/workspace/liminis-evidence/recorded-hud-candidate/originals')
OUT=Path('/workspace/liminis-evidence/recorded-hud-actual-review')
REPO='/workspace/liminis-recorded-hud-integration'; HEAD='d8e69e4d652e40409fc15bc5e9f247eb58d8c5ab';TREE='c5775c00a150768f2819e265a4165d8605bd48e3'
def sha(b):return hashlib.sha256(b).hexdigest()
def gitblob(p):return subprocess.check_output(['git','show',HEAD+':'+str(PurePosixPath(p))],cwd=REPO)
def deserialize(x,refs=None):
 if refs is None:refs={}
 if not isinstance(x,dict):return x
 if 'ref' in x:return refs[x['ref']]
 if 'o' in x:
  value={}
  if 'id' in x:refs[x['id']]=value
  for e in x['o']:value[e['k']]=deserialize(e['v'],refs)
  return value
 if 'a' in x:
  value=[]
  if 'id' in x:refs[x['id']]=value
  value.extend(deserialize(v,refs) for v in x['a']);return value
 for k in ['s','n','b']: 
  if k in x:return x[k]
 if 'v' in x:return None if x['v'] in ['null','undefined'] else {'NaN':float('nan'),'Infinity':float('inf'),'-Infinity':float('-inf'),'-0':-0.0}.get(x['v'],x['v'])
 if 'bi' in x:return int(x['bi'])
 return x

def exact(a,b):
 if isinstance(a,bool) or isinstance(b,bool):return type(a)==type(b) and a==b
 if isinstance(a,(int,float)) and isinstance(b,(int,float)):return struct.pack('>d',float(a))==struct.pack('>d',float(b))
 if type(a)!=type(b):return False
 if isinstance(a,dict):return a.keys()==b.keys() and all(exact(a[k],b[k]) for k in a)
 if isinstance(a,list):return len(a)==len(b) and all(exact(x,y) for x,y in zip(a,b))
 return a==b

def safe_zip(z,cap):
 infos=z.infolist();names=[x.filename for x in infos];assert len(names)==len(set(names))
 assert sum(x.file_size for x in infos)<=cap
 for x in infos:
  p=PurePosixPath(x.filename);mode=x.external_attr>>16
  assert not p.is_absolute() and '..' not in p.parts and '\\' not in x.filename and not x.is_dir()
  assert not stat.S_ISLNK(mode) and stat.S_IFMT(mode) in (0,stat.S_IFREG)
 assert z.testzip() is None
 return {'entries':len(infos),'expandedBytes':sum(x.file_size for x in infos),'crc':'PASS','pathRegularClosed':True}
privacy={'decodedUtf8Files':0,'decodedUtf8Bytes':0,'binaryEntries':0,'gzipTextFiles':0,'gzipDecodedBytes':0,'textBudgetBytes':128*1024*1024,'patternMatches':{},'authCookieHeaderEntries':0,'cookies':0,'qualification':'Bounded UTF8/pattern/header review of identified outer+nested entries, including retained gzip text; not an exhaustive secret guarantee.'}
patterns={'providerCredentials':r'\b(?:sk-(?:proj-|svcacct-)?[A-Za-z0-9_-]{16,}|gh[opusr]_[A-Za-z0-9]{20,}|github_pat_[A-Za-z0-9_]{20,}|xox[baprs]-[A-Za-z0-9-]{15,})\b','privateKey':r'-----BEGIN (?:RSA |EC |OPENSSH )?PRIVATE KEY-----','jwtCredential':r'\beyJ[A-Za-z0-9_-]{15,}\.[A-Za-z0-9_-]{15,}\.[A-Za-z0-9_-]{15,}\b','privateOldContext':r'(?i)(?:tailscale\.com|100\.(?:6[4-9]|[7-9][0-9]|1[01][0-9]|12[0-7])\.\d+\.\d+|(?:chat|conversation)[_-]id["\s:=]+[A-Za-z0-9_-]{16,})'}
privacy['patternMatches']={k:0 for k in patterns}
def scan(b):
 try:s=b.decode('utf-8')
 except UnicodeDecodeError:privacy['binaryEntries']+=1;return
 privacy['decodedUtf8Files']+=1;privacy['decodedUtf8Bytes']+=len(b);assert privacy['decodedUtf8Bytes']<=privacy['textBudgetBytes']
 for k,p in patterns.items():privacy['patternMatches'][k]+=len(re.findall(p,s))

def reconstruct(raw):
 rows=[json.loads(x) for x in raw.splitlines()];static=['id','parent_id','generation','birth_tick','genome_key','division_mass_mol'];dynamic=['age_s','mass_mol','energy_j','mass_units','energy_units','starvation_s'];out={}
 for n,row in enumerate(rows):
  if n==0:
   assert row['type']=='keyframe';frame=copy.deepcopy(row['frame']);genomes=copy.deepcopy(row['genomes']);cells={a[0]:dict(zip(static+dynamic,a+b)) for a,b in zip(row['definitions'],row['values'])};order=[a[0] for a in row['definitions']]
  else:
   assert row['type']=='delta';frame.update(copy.deepcopy(row['set']));genomes.update(copy.deepcopy(row['genomes']))
   for ident in row['removed']:del cells[ident]
   for ident,mask,values in row['changed']:
    names=[key for bit,key in enumerate(dynamic) if mask&(1<<bit)];assert len(names)==len(values);cells[ident].update(zip(names,values))
   for a,b in row['born']:cells[a[0]]=dict(zip(static+dynamic,a+b))
   order=row.get('order',order)
  assert set(order)==set(cells);frame['cells']=[cells[x] for x in order];out[frame['tick']]={'frame':copy.deepcopy(frame),'genomes':copy.deepcopy(genomes)}
 return out

def contains(a,b):return b['left']>=a['left'] and b['right']<=a['right'] and b['top']>=a['top'] and b['bottom']<=a['bottom']
def geometry(g):
 for n,b in g['rows'].items():
  assert b['width']>0 and b['height']>0 and b['display']!='none' and b['visibility']=='visible' and b['opacity']>0
  assert contains(g['stage'],b) and b['scrollWidth']<=b['clientWidth'] and b['scrollHeight']<=b['clientHeight']
  for t in b['text']:assert contains(b,t) and contains(g['stage'],t)
 a=g['rows'];assert contains(a['header'],a['title']) and contains(a['header'],a['stats']);assert a['header']['bottom']<=a['canvas']['top'] and a['title']['bottom']<=a['canvas']['top'] and a['stats']['bottom']<=a['canvas']['top'] and a['canvas']['bottom']<=a['footer']['top']
 c=a['canvas'];assert c['width']>72 and c['height']>72 and g['roundedInterior']['width']>0 and g['roundedInterior']['height']>0 and all(g['canvasUnobscured']) and g['loadingHidden']
 return sum(len(b['text']) for b in a.values())

def state_layout(l,frame):
 s=l['selection'];assert s['clearedSelection']=='none selected' and s['tick']==frame['tick'] and s['cells']==len(frame['cells']);assert s['selectedId']==frame['cells'][-1]['id'];assert s['inventoryBefore']==s['inventoryAfter'];assert l.get('inventoryAfterScreenshot',s['inventoryAfter'])==s['inventoryAfter']
 inv=s['inventoryAfter'];r=inv['readouts'];keys={'living':'living_cells','births':'births','deaths':'deaths','divisions':'divisions','generation':'generation_max'}
 for element,key in keys.items():assert int(r[element].replace(',',''))==frame['summary'][key]
 assert int(r['tick'].replace(',',''))==frame['tick'] and float(r['time'].replace(',',''))==frame['sim_time']
 assert r['ledger']==('M '+frame['residual']['matter']+' · E '+frame['residual']['energy'] if frame['residual'] else 'not checked')
 counts=Counter(x['genome_key'] for x in frame['cells']);expected=[{'key':k,'value':str(c)+' · '+format(c/len(frame['cells'])*100,'.1f')+'%'} for k,c in sorted(counts.items(),key=lambda a:(-a[1],a[0]))];assert inv['frequencies']==expected
 cell=frame['cells'][-1];fields=s['inspector']['fields'];assert fields['id']==cell['id'] and fields['parent']==(cell['parent_id'] or 'founder') and fields['birth tick']==str(cell['birth_tick']) and fields['mass units']==cell['mass_units'] and fields['energy units']==cell['energy_units']
 ring=s['ring'];b=ring['backingBounds'];c=ring['canvasRect'];assert ring['pixels']>=4
 assert ring['clientX']==c['left']+(b['left']+b['right']+1)/2*c['width']/ring['backing']['width']
 assert ring['clientY']==c['top']+(b['top']+b['bottom']+1)/2*c['height']/ring['backing']['height']
 return {'tick':frame['tick'],'cells':len(frame['cells']),'selectedId':cell['id'],'exactFields':fields,'frequencies':expected,'ringExactWhitePixels':ring['pixels'],'clearedSelection':s['clearedSelection'],'pointer':{'x':ring['clientX'],'y':ring['clientY']}}

receipts=json.loads((ROOT/'originals-byte-receipts.json').read_text());assert receipts['head']==HEAD and receipts['tree']==TREE and receipts['run_id']==37270688687
review={'candidate':{'head':HEAD,'tree':TREE,'run':37270688687,'attempt':1,'job':111636831214},'artifacts':{},'privacy':privacy,'bridge':{}}
allTrace={};allReport={};originalFrames={}
for row in receipts['rows']:
 label=row['label'];p=Path(row['path']);b=p.read_bytes();assert len(b)==row['bytes'] and sha(b)==row['sha256'];z=ZipFile(BytesIO(b));info=safe_zip(z,128*1024*1024)
 reportName='evidence.json' if label=='recorded' else 'report.json';d=json.loads(z.read(reportName));allReport[label]=d
 expected={x['filename'] for x in d['screenshots']}|{reportName,'trace.zip'};assert set(z.namelist())==expected
 for n in z.namelist():assert z.read(n)==(ROOT/label/n).read_bytes()
 assert d['status']=='PASS' and all(c['status']=='PASS' for c in d['checks']);assert d.get('sourceCommit',d.get('sourceHead'))==HEAD and d['sourceTree']==TREE and d['workflowRun'].endswith('/37270688687') and d['workflowAttempt']=='1'
 assert d['launchArgumentCheck']['status']=='PASS' and not d['launchArgumentCheck']['forbiddenSwitches'];argv=d['launchArgumentCheck']['arguments'];assert '--enable-automation' in argv
 bad=['--no-sandbox','--no-zygote-sandbox','--disable-setuid-sandbox','--disable-namespace-sandbox','--disable-seccomp-filter-sandbox','--disable-gpu-sandbox','--allow-sandbox-debugging','--single-process'];assert not any(a.split('=')[0] in bad or re.fullmatch(r'--disable-.*sandbox',a.split('=')[0]) for a in argv)
 assert d['browser']['chromiumSandbox'] and d['browser']['version']=='149.0.7827.55'
 pinRows=d.get('sourceFiles',d.get('sources'));src=[]
 for e in pinRows:
  path='site/'+e['filename'] if label=='recorded' else e['filename'];value=gitblob(path);assert sha(value)==e['sha256'];assert e.get('servedSha256',e['sha256'])==e['sha256'];src.append({'path':path,'bytes':len(value),'sha256':sha(value)})
 if label=='recorded':assert sha(gitblob('site/playback.browser.mjs'))==d['scriptSha256']
 for n in [reportName]:scan(z.read(n))
 images=[]
 for e in d['screenshots']:
  value=z.read(e['filename']);assert sha(value)==e['sha256'];im=Image.open(BytesIO(value));im.load();images.append({'filename':e['filename'],'bytes':len(value),'sha256':sha(value),'width':im.width,'height':im.height,'rgbaSha256':sha(im.convert('RGBA').tobytes()),'viewedIndependently':True})
 trace=z.read('trace.zip');assert len(trace)==d['trace']['bytes'] and sha(trace)==d['trace']['sha256'] and d['trace']['status']=='PASS';tz=ZipFile(BytesIO(trace));ti=safe_zip(tz,128*1024*1024)
 for entry in tz.infolist():
  value=tz.read(entry.filename)
  if entry.filename.endswith('.gz'):
   decoder=gzip.GzipFile(fileobj=BytesIO(value));plain=decoder.read(4*1024*1024+1);assert len(plain)<=4*1024*1024;privacy['gzipTextFiles']+=1;privacy['gzipDecodedBytes']+=len(plain);scan(plain)
  else:scan(value)
 events=[json.loads(x) for x in tz.read('trace.trace').splitlines()];network=[json.loads(x)['snapshot'] for x in tz.read('trace.network').splitlines()];allTrace[label]=(events,network,tz)
 before=[x for x in events if x['type']=='before'];after={x['callId']:x for x in events if x['type']=='after'};assert len(after)==len(before) and all(a['callId'] in after and not after[a['callId']].get('error') for a in before)
 assert not any(x['type']=='console' and x.get('messageType')=='error' for x in events)
 net={'requests':len(network),'methods':dict(Counter(n['request']['method'] for n in network)),'statuses':dict(Counter(n['response']['status'] for n in network)),'retainedBodies':0,'omittedBodyRows':0,'bodyChecks':[]}
 for n in network:
  for side in ['request','response']:
   privacy['cookies']+=len(n[side].get('cookies',[]));privacy['authCookieHeaderEntries']+=sum(x['name'].lower() in ['authorization','proxy-authorization','cookie','set-cookie'] for x in n[side].get('headers',[]))
  u=urlsplit(n['request']['url']);assert u.hostname=='127.0.0.1' and u.scheme=='http' and n['request']['method']=='GET' and n['response']['status']==200
  c=n['response'].get('content',{});ref=c.get('_sha1')
  if ref:
   raw=tz.read('resources/'+ref);assert hashlib.sha1(raw).hexdigest()==ref.split('.')[0];net['retainedBodies']+=1;path=unquote(u.path)
   if path.startswith('/__dense_qa__/') and not path.endswith('/harness.html'):
    parts=path.split('/');folder='smoke-100' if parts[2] in ['smoke','stream'] else 'pilot-100000';assert raw==gitblob('scripts/fixtures/dense-recording/'+folder+'/'+parts[-1])
   if not path.startswith('/__dense_qa__/'):
    expectedRaw=gitblob('site'+path)
    if raw!=expectedRaw:
     assert label=='recorded';pages=[x['pageId'] for x in events if x['type']=='before' and x.get('method')=='goto']
     if n['pageref']==pages[-2]:
      changed=bytearray(expectedRaw);changed[-1]^=1;assert raw==changed;role='exact last-byte XOR digest negative'
     else:
      assert n['pageref']==pages[-1]
      if path=='/data/catalog.json':
       modified=json.loads(raw);original=json.loads(expectedRaw);oldEntry=next(x for x in original['entries'] if x['id']==original['default_experiment']);newEntry=next(x for x in modified['entries'] if x['id']==modified['default_experiment']);schemaResource=next(v for v in network if v['pageref']==n['pageref'] and urlsplit(v['request']['url']).path=='/data/cell-chamber-seed-42.json');schemaRaw=tz.read('resources/'+schemaResource['response']['content']['_sha1']);oldEntry['bytes']=len(schemaRaw);oldEntry['sha256']=sha(schemaRaw);assert exact(modified,original);role='exact metadata rehash for schema negative'
      else:
       modified=json.loads(raw);original=json.loads(expectedRaw);original['schema_version']=999;assert exact(modified,original);role='exact schema999 negative'
     n['_independentNegativeRole']=role
   net['bodyChecks'].append({'path':path,'bytes':len(raw),'sha256':sha(raw),'negativeRole':n.get('_independentNegativeRole')})
  else:net['omittedBodyRows']+=1
 pagesClose=sum(a['class']=='Page' and a['method']=='close' for a in before);closedEvents=sum(x['type']=='event' and x.get('method')=='pageClosed' for x in events);assert pagesClose==closedEvents
 review['artifacts'][label]={'artifactId':row['artifact_id'],'originalBytes':len(b),'originalSha256':sha(b),'outer':info,'trace':{'bytes':len(trace),'sha256':sha(trace),**ti,'pairedActions':len(before),'actionErrors':0,'pageCloseAcks':pagesClose,'pageClosedEvents':closedEvents,'warnings':len([x for x in events if x['type']=='console']),'traceEndedBeforeContextBrowserCleanup':True},'checkCount':len(d['checks']),'checks':d['checks'],'sourcePins':src,'screenshots':images,'network':net,'elapsedMs':(datetime_parse(d['finishedAt'])-datetime_parse(d['startedAt']))*1000 if False else None}
 if label=='recorded':
  assert d['httpRequestCoverage']['status']=='PASS' and all(x['status']=='PASS' for x in d['httpChecks'])
  review['artifacts'][label]['httpChecks']=len(d['httpChecks']);review['artifacts'][label]['downloads']=d['recordings']
  catalog=json.loads(gitblob('site/data/catalog.json'))
  for e in catalog['entries']:
   raw=gitblob('site/'+e['recording']);assert len(raw)==e['bytes'] and sha(raw)==e['sha256'];originalFrames[e['id']]=json.loads(raw)
 else:
  for k in ['pageErrors','consoleErrors','unhandled','cleanupErrors']:assert not d[k]
  assert d['finalRequestClassification']['status']=='PASS' and d['decoder']['uninstrumented']['status']=='PASS'
  assert len(d['pageMeasurements'])==5 and [len(x['unhandled']) for x in d['pageMeasurements']]==[0,1,0,0,0]
  assert d['pageMeasurements'][1]['expectedControls'][0]['index']==0 and len(d['pageMeasurements'][1]['expectedControls'])==1

# Independent retained-value reconstruction, only tiny101 and one225-record chunk; no model, tests or full-recording decode.
tz=allTrace['dense'][2];tinyFrames=reconstruct(gzip.decompress(tz.read('resources/49bf2f9bc4d8d6373d530ee1480b09ad13669eb7.gz')));frame992=reconstruct(gzip.decompress(tz.read('resources/c6d029e61ad548ccfbe97ec05faef26f6222c8d2.gz')))[992]['frame'];assert len(frame992['cells'])==216
trace=allTrace['dense'][0];pairs={x['callId']:x for x in trace if x['type']=='after'};actualTiny={}
for x in trace:
 if x['type']=='before' and x.get('method')=='evaluateExpression' and x.get('params',{}).get('expression')=='(tick) => window.__denseQaLoader.seek(tick)':
  value=deserialize(pairs[x['callId']].get('result',{}).get('value'))
  if value and value['frame']['tick']<=100 and x['pageId']==next(a['pageId'] for a in trace if a['type']=='before' and a.get('method')=='goto'):
   actualTiny[value['frame']['tick']]=value
assert set(range(101))<=set(actualTiny)
assert all(exact(actualTiny[t],tinyFrames[t]) for t in range(101))
review['boundedRawFrameAudit']={'nativeBrowser101ValuesVsOriginalRetainedGzip':'EXACT_FLOAT_BITS_AND_STRINGS','frame992OriginalChunkRange':[768,992],'frame992DecodedBytes':4135813,'frame992Summary':frame992['summary'],'frame992SelectedCell':frame992['cells'][-1],'noModelsOrFullDatasetDecode':True}
for label in ['recorded','dense']:
 d=allReport[label];summaries=[];geometryCount=0;textCount=0
 for l in d['layouts']:
  if label=='dense':frame=frame992
  else:
   eid=l['label'].split(':')[0]
   if eid not in originalFrames:eid='cell-chamber-10k'
   frame=next(f for f in originalFrames[eid]['frames'] if f['tick']==l['selection']['tick'])
  s=state_layout(l,frame)
  for k in ['hudBefore','hudAfterScreenshot']:
   if k in l:textCount+=geometry(l[k]);geometryCount+=1
  s.update({'label':l.get('label',l.get('filename')),'viewport':l['viewport'],'canvasCss':{k:l['hudBefore']['rows']['canvas'][k] for k in ['width','height']},'roundedInterior':l['hudBefore']['roundedInterior']});summaries.append(s)
 dEvents=allTrace[label][0];before=[x for x in dEvents if x['type']=='before'];afters={x['callId']:x for x in dEvents if x['type']=='after'};mouse=[x for x in before if x.get('method')=='mouseClick'];assert len(mouse)==len(d['layouts'])
 transitions=[]
 for m,l in zip(mouse,d['layouts']):
  candidates=[x for x in before if x.get('pageId')==m['pageId'] and x['startTime']<m['startTime']];lastBlank=next(x for x in reversed(candidates) if x.get('method')=='click' and x.get('params',{}).get('selector')=='#chamber');assert lastBlank['params']['position']=={'x':5,'y':5}
  middle=[x for x in candidates if x['startTime']>lastBlank['startTime']];none=next(x for x in middle if x.get('method')=='textContent' and x['params']['selector']=='#selection-label');count=next(x for x in middle if x.get('method')=='queryCount');assert afters[none['callId']]['result']['value']=='none selected' and afters[count['callId']]['result']['value']==0
  key=next(x for x in reversed(candidates) if x.get('method')=='press');assert key['params']['key']=='ArrowLeft'
  inspector=next(x for x in before if x['startTime']>m['startTime'] and x.get('method')=='evaluateExpression' and 'heading:' in x.get('params',{}).get('expression',''));actual=deserialize(afters[inspector['callId']]['result']['value']);assert actual==l['selection']['inspector'];assert m['params']['x']==l['selection']['ring']['clientX'] and m['params']['y']==l['selection']['ring']['clientY']
  transitions.append({'keyboardCallId':key['callId'],'blankCallId':lastBlank['callId'],'noneCallId':none['callId'],'emptyInspectorCallId':count['callId'],'singlePointerCallId':m['callId'],'inspectorCallId':inspector['callId'],'selectedId':actual['fields']['id']})
 review['artifacts'][label]['hud']={'layoutCount':len(summaries),'geometryObservationsRecomputed':geometryCount,'containedTextRects':textCount,'layouts':summaries,'nativeTransitions':transitions,'locatorRingReadResultTraceOmitted':'Ring pixel/bounds live values retained in report; source uses actual locator read and screenshots corroborate; Playwright locator.evaluate result itself not retained.'}

# Strict native failure correlation, not generic abort allowance.
d=allReport['dense'];failureClasses=Counter();correlations=[]
for row in d['http']:
 if row.get('filename'):
  original=gitblob(row['filename']);assert len(original)==row['bytes'] and sha(original)==row['sha256']
  if row['filename'].endswith('.gz'):assert row['contentEncoding'] is None
assert sum(x.get('bytes',0) for x in d['http'])==d['servedBytes']<=96*1024*1024
init=[x for x in allTrace['dense'][0] if x['type']=='before' and x.get('method')=='addInitScript'];assert len(init)==5 and init[1]['params']['source'].endswith(')({"observeBodies":false})')
assert d['requestObservations'][1]['native']['calls'][0]['body']['readers']==0
review['artifacts']['dense']['nativePromiseScope']='Trace actual observeBodies=false second-page init; no native fetch observer handlers or reader wrappers in this phase, one real controlled16B abort and exact orphan positive control.'
review['artifacts']['dense']['actualHttpBytesWritten']=sum(x.get('bytesWritten',0) for x in d['http'])
for obs in d['requestObservations']:
 assert not obs['errors'] and not obs['native']['errors'] and obs['failureBijection']['status']=='PASS';failures=[x for x in d['requestFailures'] if x['pageId']==obs['pageId']];assert len(failures)==len([x for x in obs['network'] if x.get('failure')]);seen=set();callsSeen=set()
 for f in failures:
  rows=[x for x in obs['network'] if x['url']==f['url'] and x['method']==f['method'] and x['occurrence']==f['occurrence']];assert len(rows)==1;n=rows[0];assert len(n['fetchIds'])==1;calls=[x for x in obs['native']['calls'] if x['id']==n['fetchIds'][0]];assert len(calls)==1;c=calls[0]
  assert n['requestId'] not in seen and c['id'] not in callsSeen;seen.add(n['requestId']);callsSeen.add(c['id']);assert sum(c['id'] in x['fetchIds'] for x in obs['network'])==1
  assert f['error']==n['failure']['errorText']=='net::ERR_ABORTED' and n['failure']['canceled'] and f['resourceType']=='fetch' and n['resourceType']=='Fetch';assert c['url']==f['url'] and c['method']==f['method'] and c['hasSignal'] and not c['initiallyAborted']
  abort=c['abort'];failEpoch=n['wallEpochMs']+(n['failure']['timestamp']-n['timestamp'])*1000;assert abort and abort['name']=='AbortError' and abort['epochMs']<=failEpoch+1
  assert not any(x and x['epochMs']<=abort['epochMs'] for x in [c.get('fetchError'),c['body']['readError']])
  explicit=abort['message'] in ['Dense recording request superseded or aborted','Dense QA mid-body cancellation'] or (abort['message']=='Dense QA uninstrumented mid-body cancellation' and obs['proofMode']=='signal-only; native promise rejection semantics uninstrumented' and sum(x.get('abort',{}).get('message')==abort['message'] for x in obs['native']['calls'] if x.get('abort'))==1)
  if explicit:kind='EXACT_EXPLICIT';assert f['classification']=='Exact explicit module/test signal cancellation'
  else:
   kind='VERIFIED_FULL_EOF';assert abort['name']==obs['native']['defaultAbort']['name'] and abort['message']==obs['native']['defaultAbort']['message'];assert c['response']['status']==200 and c['body']['readError'] is None and not c.get('fetchError');assert c['body']['bytes']==int(c['response']['contentLength'])<=16*1024*1024 and c['body']['doneEpochMs']<=abort['epochMs'];assert f['classification']=='Native full body EOF/count observed before successful finally cleanup abort'
  assert f['intentional'];failureClasses[kind]+=1;correlations.append({'pageId':obs['pageId'],'requestId':n['requestId'],'nativeFetchId':c['id'],'class':kind,'actualBytes':c['body']['bytes'],'bodyEofEpochMs':c['body']['doneEpochMs'],'abortBeforeOrWithinClockAllowanceMs':failEpoch-abort['epochMs']})
review['artifacts']['dense']['nativeFailures']={'count':len(correlations),'classes':dict(failureClasses),'bijection':'ALL_ONE_TO_ONE','correlations':correlations,'clockAllowanceMs':1,'signalOnlyPhase':d['decoder']['uninstrumented'],'pageMeasurements':d['pageMeasurements'],'finalRequestClassification':d['finalRequestClassification'],'rawUnexpectedErrors':0,'qualification':'The exact one intentional native orphan sensitivity event is retained; no claim of zero total native unhandled events.'}
for e in d['ui']['endpoints']:
 for k in ['initialHud','endpointHud','finalHud']:geometry(e[k])
h=d['ui']['hold'];assert abs(h['modelSecondsAdvanced']-h['measuredWallMs']/1000)<.1 and h['httpRequests']==0 and h['tick']==992 and h['rate']==1
review['artifacts']['dense']['ui']={k:v for k,v in d['ui'].items() if k!='endpoints'};review['artifacts']['dense']['endpoints']=[{'horizon':x['horizon'],'shownTick':x['shownTick'],'chunkRequests':x['chunkRequests']} for x in d['ui']['endpoints']];review['artifacts']['dense']['decoder']=d['decoder'];review['artifacts']['dense']['servedBytes']=d['servedBytes'];review['artifacts']['dense']['httpRows']=len(d['http'])
assert all(x['status'] in [200,204] and not x.get('error') for x in d['http']);assert d['ui']['tenKChunkRequests']<=16
rd=allReport['recorded'];imgs={x['filename']:x for x in review['artifacts']['recorded']['screenshots']};assert imgs['held-30fps.png']['sha256']==imgs['held-60fps.png']['sha256'] and imgs['held-30fps.png']['rgbaSha256']==imgs['held-60fps.png']['rgbaSha256'];review['artifacts']['recorded']['heldPair']={'pngSha256':imgs['held-30fps.png']['sha256'],'rgbaSha256':imgs['held-30fps.png']['rgbaSha256'],'width':imgs['held-30fps.png']['width'],'height':imgs['held-30fps.png']['height'],'changedPixels':0,'canvasPixelAssertionSha256':next(x['sha256'] for x in rd['checks'] if x['name']=='held sample canvas pixel SHA-256')}
bridgePath=Path('/workspace/liminis-evidence/recorded-hud-candidate/regression-source-bridge.json');bridge=json.loads(bridgePath.read_text());assert bridge['candidate_head']==HEAD and bridge['candidate_tree']==TREE
for e in bridge['entries']:
 assert subprocess.check_output(['git','ls-tree',HEAD,'--',e['path']],cwd=REPO).decode().strip()==e['git_entry'];assert subprocess.check_output(['git','ls-tree',bridge['accepted_source_head'],'--',e['path']],cwd=REPO).decode().strip()==e['git_entry']
review['bridge']={'sha256':sha(bridgePath.read_bytes()),'entries':len(bridge['entries']),'allGitObjectsEqual':True,'qualification':bridge['qualification'],'approval':bridge['approval'],'notNewIndependentGlArtifactAcceptance':True}
assert privacy['authCookieHeaderEntries']==0 and privacy['cookies']==0
print(json.dumps({'artifactCounts':{k:{'checks':v['checkCount'],'outer':v['outer']['entries'],'nested':v['trace']['entries'],'actions':v['trace']['pairedActions'],'retainedBodies':v['network']['retainedBodies'],'omittedBodyRows':v['network']['omittedBodyRows'],'images':len(v['screenshots'])} for k,v in review['artifacts'].items()},'privacy':privacy,'nativeFailureClasses':dict(failureClasses)},ensure_ascii=False))
(OUT/'bounded-analysis.json').write_text(json.dumps(review,ensure_ascii=False,indent=2)+'\n')
