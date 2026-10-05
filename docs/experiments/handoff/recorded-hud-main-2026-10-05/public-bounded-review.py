# Independent, local-only inspection of fixed public evidence. No HTTP/browser/model.
from pathlib import Path, PurePosixPath
from zipfile import ZipFile
from io import BytesIO
from collections import Counter
from urllib.parse import urlsplit
from PIL import Image
import hashlib,json,stat,re,gzip,subprocess,copy,struct,datetime
OUT=Path('/workspace/liminis-evidence/recorded-hud-main-public-review')
ROOT=Path('/workspace/liminis-evidence/recorded-hud-main/originals')
REPO='/workspace/liminis-recorded-hud-main-acceptance'
HEAD='813be1efad29569a3011f00697bd5d41b7397dc6';TREE='c5775c00a150768f2819e265a4165d8605bd48e3'
sha=lambda b:hashlib.sha256(b).hexdigest()
def git(path):return subprocess.check_output(['git','show',HEAD+':'+str(PurePosixPath(path))],cwd=REPO)
def pin(p):
 b=p.read_bytes();return {'bytes':len(b),'sha256':sha(b)}
def safezip(z,cap):
 names=[a.filename for a in z.infolist()];assert len(names)==len(set(names))
 total=sum(a.file_size for a in z.infolist());assert total<=cap
 for a in z.infolist():
  p=PurePosixPath(a.filename);mode=a.external_attr>>16
  assert not p.is_absolute() and '..' not in p.parts and '\\' not in a.filename and not a.is_dir()
  assert not stat.S_ISLNK(mode) and stat.S_IFMT(mode) in [0,stat.S_IFREG]
 assert z.testzip() is None
 return {'entries':len(names),'expandedBytes':total,'crc':'PASS','closedRegularPaths':True}
def exact(a,b):
 if isinstance(a,bool) or isinstance(b,bool):return type(a)==type(b) and a==b
 if isinstance(a,(int,float)) and isinstance(b,(int,float)):return struct.pack('>d',float(a))==struct.pack('>d',float(b))
 if type(a)!=type(b):return False
 if isinstance(a,dict):return a.keys()==b.keys() and all(exact(a[k],b[k]) for k in a)
 if isinstance(a,list):return len(a)==len(b) and all(exact(x,y) for x,y in zip(a,b))
 return a==b
patterns={'providerCredential':r'\b(?:sk-(?:proj-|svcacct-)?[A-Za-z0-9_-]{16,}|gh[opusr]_[A-Za-z0-9]{20,}|github_pat_[A-Za-z0-9_]{20,}|xox[baprs]-[A-Za-z0-9-]{15,})\b','privateKey':r'-----BEGIN (?:RSA |EC |OPENSSH )?PRIVATE KEY-----','jwt':r'\beyJ[A-Za-z0-9_-]{15,}\.[A-Za-z0-9_-]{15,}\.[A-Za-z0-9_-]{15,}\b','privateOldContext':r'(?i)(?:tailscale\.com|100\.(?:6[4-9]|[7-9][0-9]|1[01][0-9]|12[0-7])\.\d+\.\d+|(?:chat|conversation)[_-]id["\s:=]+[A-Za-z0-9_-]{16,})'}
privacy={'utf8Entries':0,'utf8Bytes':0,'binaryEntries':0,'gzipDecodedEntries':0,'gzipDecodedBytes':0,'boundBytes':64*1024*1024,'matches':{k:0 for k in patterns},'authenticationCookieHeaderEntries':0,'cookies':0,'notExhaustiveSecretGuarantee':True}
def scan(b):
 try:s=b.decode('utf8')
 except UnicodeDecodeError:privacy['binaryEntries']+=1;return
 privacy['utf8Entries']+=1;privacy['utf8Bytes']+=len(b);assert privacy['utf8Bytes']<=privacy['boundBytes']
 for k,p in patterns.items():privacy['matches'][k]+=len(re.findall(p,s))
receipt=json.loads((ROOT.parent/'original-artifact-pins.json').read_text());assert receipt['head']==HEAD and receipt['tree']==TREE and receipt['run_id']==37273770904 and receipt['attempt']==1
row=next(r for r in receipt['rows'] if r['label']=='public')
b=(ROOT/row['name']).read_bytes();assert len(b)==row['bytes']==5598658 and sha(b)==row['sha256']=='d3225ea886c7fdbc605bd19e91c2830cd4b87aa131bca25dc984ddf0ea9be150'
z=ZipFile(BytesIO(b));outer=safezip(z,32*1024*1024)
assert set(z.namelist())=={'evidence.json','trace.zip','desktop-default.png','mobile-default.png','desktop-observe-raw.html','mobile-observe-raw.html'}
for n in z.namelist():assert z.read(n)==(ROOT/'public'/n).read_bytes()
d=json.loads(z.read('evidence.json'));assert d['status']==d['application_playback']=='PASS' and d['sourceCommit']==HEAD and d['sourceTree']==TREE and d['workflowRun'].endswith('/37273770904') and d['workflowAttempt']=='1' and not d['trackedWorkingTreeStatus']
assert sha(git('scripts/check_public_recording_browser.mjs'))==d['scriptSha256']=='0d41e009ac7c8fd8feca1fd398b016a4f5dee5e8f9119f10900d150dfb0e5f7b'
assert len(d['checks'])==4 and all(r['status']=='PASS' for r in d['checks'])
assert d['playwrightVersion']=='1.61.1' and d['browser']['version']=='149.0.7827.55' and d['browser']['chromiumSandbox'] and d['launchArgumentCheck']['status']=='PASS'
forbidden={'--no-sandbox','--no-zygote-sandbox','--disable-setuid-sandbox','--disable-namespace-sandbox','--disable-seccomp-filter-sandbox','--disable-gpu-sandbox','--allow-sandbox-debugging','--single-process'}
argv=d['launchArgumentCheck']['arguments'];assert '--enable-automation' in argv and not d['launchArgumentCheck']['forbiddenSwitches'] and not any(a.split('=')[0] in forbidden or re.fullmatch(r'--disable-.*sandbox',a.split('=')[0]) for a in argv)
assets={r['filename']:r for r in d['sourceFiles']};assert len(assets)==19 and len(d['browserSourceFiles'])==14
for p,r in assets.items():raw=git('site/'+p);assert len(raw)==r['bytes'] and sha(raw)==r['sha256']
node=d['publicAssets']['verifiedResponses'];assert len(node)==19 and len(d['publicAssetAttempts'])==1 and len(d['publicAssetAttempts'][0]['observations'])==19 and d['publicAssets']['status']=='PASS'
for r in node:
 exp=assets[r['filename']];assert r['status']=='PASS' and r['httpStatus']==200 and r['bytes']==r['expectedBytes']==exp['bytes'] and r['sha256']==r['expectedSha256']==exp['sha256']
 assert urlsplit(r['url']).scheme=='https' and urlsplit(r['url']).netloc=='liminis.dev';assert len(r['redirects'])<=4
 for hop in r['redirects']:assert hop['status'] in [301,302,303,307,308] and all(urlsplit(hop[k]).scheme=='https' and urlsplit(hop[k]).netloc=='liminis.dev' for k in ['from','to'])
 assert urlsplit(r['finalUrl']).scheme=='https' and urlsplit(r['finalUrl']).netloc=='liminis.dev'
index=json.loads(git('site/data/dense-cell-chamber/index.json'));indexByPath={m['path']:m for m in index['manifests']};endpoints=[]
for e in d['publicDenseEndpointPins']:
 assert e['horizon'] in [10000,100000,1000000] and len(e['filenames'])==3
 ip,mp,cp=e['filenames'];m=json.loads(git('site/'+mp));last=m['chunks'][-1];ir=indexByPath[PurePosixPath(mp).name]
 assert m['experiment']['steps']==last['last_tick']==ir['last_tick']==e['horizon'] and m['experiment']['frames']==ir['frames']==e['horizon']+1
 assert m['experiment']['dt_seconds']==30 and last['path']==str(PurePosixPath(cp).relative_to('data/dense-cell-chamber'))
 assert ir['sha256']==assets[mp]['sha256'] and ir['bytes']==assets[mp]['bytes'] and last['gzip_bytes']==assets[cp]['bytes'] and last['gzip_sha256']==assets[cp]['sha256']
 endpoints.append({'id':e['id'],'horizon':e['horizon'],'manifest':mp,'finalChunk':cp,'lastTick':last['last_tick'],'gzipBytes':last['gzip_bytes'],'gzipSha256':last['gzip_sha256'],'mode':'Node HTTPS pins only; not nondefault browser endpoint rendering'})
assert {r['horizon'] for r in endpoints}=={10000,100000,1000000}
htmlSource=git('site/observe.html');edge=d['hostingHtmlQualification']['knownEdgeInsertion'];assert len(htmlSource)==edge['sourceBytes']==6954 and sha(htmlSource)==edge['sourceSha256']=='105c5a860651769c0310c4185a96d7ad2442c3726b51779a8afa1f1707d0b72e' and edge['offset']==htmlSource.index(b'</body>')==6938 and edge['bytes']==367 and edge['sha256']=='bbba70d1fbb140fe2cff2d40386e726bfe911227760ad6e69e29644e42b6f40a'
htmlPins=[]
for name in ['desktop','mobile']:
 raw=z.read(name+'-observe-raw.html');insertion=raw[6938:7305];normalized=raw[:6938]+raw[7305:];assert len(raw)==7321 and len(insertion)==367 and sha(insertion)==edge['sha256'] and normalized==htmlSource
 htmlPins.append({'name':name,'rawBytes':len(raw),'rawSha256':sha(raw),'normalizedBytes':len(normalized),'normalizedSha256':sha(normalized),'exactInsertionVerified':True,'analyticsExecution':'NOT_TESTED_BLOCKED_BEFORE_TRANSMISSION'})
images=[]
for r in d['screenshots']:
 raw=z.read(r['filename']);assert sha(raw)==r['sha256'];im=Image.open(BytesIO(raw));im.load();images.append({'filename':r['filename'],'bytes':len(raw),'sha256':sha(raw),'width':im.width,'height':im.height,'rgbaSha256':sha(im.convert('RGBA').tobytes()),'independentlyViewedViaViewImage':True})
tb=z.read('trace.zip');assert len(tb)==d['trace']['bytes'] and sha(tb)==d['trace']['sha256'] and d['trace']['status']=='PASS'
tz=ZipFile(BytesIO(tb));trace=safezip(tz,64*1024*1024)
for n in ['evidence.json','desktop-observe-raw.html','mobile-observe-raw.html']:scan(z.read(n))
for r in tz.infolist():
 raw=tz.read(r.filename)
 if r.filename.endswith('.gz'):
  g=gzip.GzipFile(fileobj=BytesIO(raw));decoded=g.read(16*1024*1024+1);assert len(decoded)<=16*1024*1024
  privacy['gzipDecodedEntries']+=1;privacy['gzipDecodedBytes']+=len(decoded);scan(decoded)
 else:scan(raw)
events=[json.loads(s) for s in tz.read('trace.trace').splitlines()];network=[json.loads(s)['snapshot'] for s in tz.read('trace.network').splitlines()]
before=[r for r in events if r['type']=='before'];after={r['callId']:r for r in events if r['type']=='after'};assert len(before)==len(after)==202 and all(r['callId'] in after and not after[r['callId']].get('error') for r in before)
assert sum(r.get('method')=='close' and r.get('class')=='Page' for r in before)==sum(r['type']=='event' and r.get('method')=='pageClosed' for r in events)==2
assert sum(r.get('class')=='Response' and r.get('method')=='body' for r in before)==28
pageIds=[r['pageId'] for r in before if r.get('method')=='goto'];assert len(pageIds)==2
browserResults=[];retainedBodies=[];moduleOmissions=0;otherOmissions=0
for pageId,obs in zip(pageIds,d['browserObservations']):
 assert obs['status']=='PASS' and not obs['errors'] and obs['playbackPageRequests']==[]
 assert obs['requests'][obs['playbackRequestBaseline']:]==[] and obs['playbackRequestBaseline']==len(obs['requests'])==16
 assert obs['finalization']=={'pageClosed':True,'pendingGuardActions':0,'settledBodyReads':14,'totalBodyReads':14,'expectedAnalyticsCount':1,'finalAnalyticsCount':1}
 assert obs['requestGuard']['allowed']==15 and obs['requestGuard']['blocked']==0 and obs['requestGuard']['status']=='PASS'
 assert len(obs['analyticsRequests'])==1 and obs['analyticsRequests'][0]['status']=='BLOCKED_ACKNOWLEDGED' and obs['analyticsRequests'][0]['resourceType']=='Script' and obs['analyticsRequests'][0]['documentUrl']=='https://liminis.dev/observe'
 assert len(obs['sourceResponses'])==14 and {r['filename'] for r in obs['sourceResponses']}=={r['filename'] for r in d['browserSourceFiles']}
 for r in obs['sourceResponses']:
  asset=assets[r['filename']];assert r['status']=='PASS' and r['httpStatus']==200 and r['expectedBytes']==asset['bytes'] and r['expectedSha256']==asset['sha256']
  if r['filename']!='observe.html':assert r['bytes']==asset['bytes'] and r['sha256']==asset['sha256']
  else:assert r['bytes']==7321 and r['sha256']==htmlPins[0]['rawSha256'] and not r['rawHtmlMatchesSource'] and r['knownEdgeInjectionVerified'] and r['afterExactInsertionRemovalMatchesSource'] and r['normalizedSha256']==sha(htmlSource)
 assert obs['nextSample']=={'time':30,'tick':1,'livingCells':8}
 assert len(obs['heldSamples'])==2 and [r['drawTarget'] for r in obs['heldSamples']]==[30,60]
 held=[]
 for r in obs['heldSamples']:
  assert r['shownTick']==r['shownTime']==0 and r['canvasSha256']==obs['initialCanvas']['sha256'] and r['modelSecondsAdvanced']>=1 and abs(r['modelSecondsAdvanced']-r['wallSeconds'])<.1 and r['playhead']<30 and r['durationAt1x']=='3.47 d'
  held.append({**r,'absoluteWallModelDifferenceSeconds':abs(r['modelSecondsAdvanced']-r['wallSeconds'])})
 assert obs['initialCanvas']['paintedPixels']>10
 req=[r for r in network if r['pageref']==pageId];assert len(req)==16 and Counter(r['request']['url'] for r in req)==Counter(r['url'] for r in obs['requests'])
 playClicks=[r for r in before if r.get('pageId')==pageId and r.get('method')=='click'];assert [r['params']['selector'] for r in playClicks]==['#play','#play','#play','#play','#next','#previous']
 assert max(r['_monotonicTime'] for r in req)<playClicks[0]['startTime']
 textValues={}
 for r in before:
  if r.get('pageId')==pageId and r.get('method')=='textContent':textValues.setdefault(r['params']['selector'],[]).append(after[r['callId']]['result']['value'])
 assert '1' in textValues['#tick'] and '30' in textValues['#time'] and '8' in textValues['#living']
 browserResults.append({'name':obs['name'],'viewport':obs['viewport'],'assetBodyVerdicts':14,'sameOriginGetAndRedirects':15,'analyticsExactBlocked':1,'requestBijection':'16 exact URLs per page match trace multiset','heldSamples':held,'nextSample':obs['nextSample'],'finalization':obs['finalization'],'initialCanvas':obs['initialCanvas'],'nativeClickSelectors':[r['params']['selector'] for r in playClicks],'actualTraceTextValues':textValues,'noPlaybackRequestsThroughClose':True})
 for r in req:
  q=r['request'];res=r['response'];u=urlsplit(q['url']);assert q['method']=='GET'
  for side in [q,res]:
   privacy['cookies']+=len(side.get('cookies',[]));privacy['authenticationCookieHeaderEntries']+=sum(h['name'].lower() in ['authorization','proxy-authorization','cookie','set-cookie'] for h in side.get('headers',[]))
  if u.netloc!='liminis.dev':assert q['url']==edge['scriptUrl'] and r['_resourceType']=='script' and res['status']==-1 and not res['headers'];otherOmissions+=1;continue
  assert u.scheme=='https'
  if res['status']==307:assert u.path=='/observe.html' and res['redirectURL']=='https://liminis.dev/observe';otherOmissions+=1;continue
  assert res['status']==200
  ref=res['content'].get('_sha1')
  if not ref:assert r['_resourceType']=='script';moduleOmissions+=1;continue
  raw=tz.read('resources/'+ref);assert hashlib.sha1(raw).hexdigest()==ref.split('.')[0]
  if u.path=='/observe':assert raw==z.read(obs['name']+'-observe-raw.html')
  else:assert raw==git('site'+u.path)
  if u.path.endswith('.gz'):assert not any(h['name'].lower()=='content-encoding' for h in res['headers']) and raw[:2]==b'\x1f\x8b'
  retainedBodies.append({'name':obs['name'],'path':u.path,'bytes':len(raw),'sha256':sha(raw)})
console=[r for r in events if r['type']=='console'];assert len(console)==4
for r in console:
 if r['messageType']=='error':assert r['location']['url']==edge['scriptUrl'] and re.fullmatch(r'Failed to load resource: net::ERR_BLOCKED_BY_CLIENT(?:\.Inspector)?',r['text'])
 else:assert r['messageType']=='warning' and r['text'].startswith('Canvas2D:')
assert not d['blockedRequests'] and not d.get('cleanupErrors') and len(d['analyticsRequests'])==2 and all(r['status']=='BLOCKED_ACKNOWLEDGED' for r in d['analyticsRequests'])
assert not any(privacy['matches'].values()) and privacy['authenticationCookieHeaderEntries']==privacy['cookies']==0
# Only the initial saved chunk, not a full horizon, decoded to validate actual 0->1 scope.
chunk=git('site/data/dense-cell-chamber/chunks/00/chunk-0000000-0000255.jsonl.gz');stream=gzip.GzipFile(fileobj=BytesIO(chunk));first=json.loads(stream.readline(1024*1024));second=json.loads(stream.readline(1024*1024))
assert first['type']=='keyframe' and second['type']=='delta';static=['id','parent_id','generation','birth_tick','genome_key','division_mass_mol'];dynamic=['age_s','mass_mol','energy_j','mass_units','energy_units','starvation_s']
f=copy.deepcopy(first['frame']);f['cells']=[dict(zip(static+dynamic,a+b)) for a,b in zip(first['definitions'],first['values'])];archive=json.loads(git('site/data/cell-chamber-seed-42.json'));assert exact(f,archive['frames'][0]);nf=copy.deepcopy(f);nf.update(second['set']);cells={r['id']:r for r in nf['cells']}
for ident in second['removed']:del cells[ident]
for ident,mask,values in second['changed']:keys=[k for bit,k in enumerate(dynamic) if mask&(1<<bit)];assert len(keys)==len(values);cells[ident].update(zip(keys,values))
for a,b in second['born']:cells[a[0]]=dict(zip(static+dynamic,a+b))
nf['cells']=[cells[i] for i in second.get('order',list(cells))];assert f['tick']==f['sim_time']==0 and nf['tick']==1 and nf['sim_time']==30 and len(f['cells'])==len(nf['cells'])==8
bridgePath=ROOT.parent/'source-tree-bridge.json';bridge=json.loads(bridgePath.read_text());assert bridge['main_head']==HEAD and bridge['main_tree']==bridge['candidate_tree']==TREE
assert subprocess.check_output(['git','rev-parse',HEAD+'^{tree}',bridge['candidate_head']+'^{tree}'],cwd=REPO).decode().splitlines()==[TREE,TREE]
result={'status':'BOUNDED_LOCAL_EVIDENCE_AUDIT_PASS','head':HEAD,'tree':TREE,'runId':37273770904,'attempt':1,'recordedJobId':111646192186,'artifactId':row['artifact_id'],'original':{'bytes':len((ROOT/row['name']).read_bytes()),'sha256':row['sha256'],**outer,'unrepacked':True},'trace':{'bytes':len(tb),'sha256':sha(tb),**trace,'pairedActions':202,'actionErrors':0,'pageCloseAcks':2,'responseBodyCallAcks':28,'retainedBodyRows':len(retainedBodies),'omittedModuleBodyRows':moduleOmissions,'redirectAndBlockedBodyRowsNotAssetBytes':otherOmissions,'cdpProtocolMessagesNotRetained':True,'traceStopsBeforeContextBrowserCleanup':True},'sourcePins':d['sourceFiles'],'browserSourcePins':d['browserSourceFiles'],'nodeHttps':{'attempts':1,'responses':19,'totalDecodedResponseBytes':sum(r['bytes'] for r in node),'allResponsePinsMatchGit':True,'redirects':[h for r in node for h in r['redirects']],'finalHorizonPins':endpoints,'rawResponseBodiesNotArchived':True},'rawHtml':htmlPins,'screenshots':images,'browserObservations':browserResults,'retainedBrowserBodies':retainedBodies,'console':{'expectedAnalyticsErrors':2,'canvasReadbackWarnings':2,'unexpectedErrors':0,'noExplicitNativeUnhandledSampler':True},'sourceFirstTwoStates':{'initialTick':0,'initialSimTime':0,'nextTick':1,'nextSimTime':30,'livingCells':8,'denseGenesisExactSparseGenesis':True,'noFullDecode':True},'sandbox':{'browser':d['browser'],'playwright':d['playwrightVersion'],'node':d['nodeVersion'],'runner':d['runner'],'observedArgv':argv,'forbiddenSwitches':[],'stockSoftwareGpuQualified':True},'sourceScriptSha256':d['scriptSha256'],'checks':d['checks'],'elapsedMs':(datetime.datetime.fromisoformat(d['finishedAt'].replace('Z','+00:00'))-datetime.datetime.fromisoformat(d['startedAt'].replace('Z','+00:00'))).total_seconds()*1000,'timeouts':d['timeouts'],'privacy':privacy,'treeBridge':{'fileSha256':sha(bridgePath.read_bytes()),'sameTree':True,'qualification':bridge['qualification']},'analytics':'Exact reviewed insertion hashed; two actual pre-send blocks ACKed in final evidence. No raw CDP messages in trace; analytics execution is NOT_TESTED.', 'ownership':'Only own bounded script/data/report files; no source/Git/network/browser/model/Cargo/tests/run changes.'}
(OUT/'bounded-analysis.json').write_text(json.dumps(result,ensure_ascii=False,indent=2)+'\n')
print(json.dumps({'status':result['status'],'outer':outer,'trace':result['trace'],'nodeTotalBytes':result['nodeHttps']['totalDecodedResponseBytes'],'privacy':privacy,'images':[(r['filename'],r['width'],r['height']) for r in images]},ensure_ascii=False))
