from pathlib import Path,PurePosixPath
import zipfile,io,json,hashlib,re,subprocess,struct
from urllib.parse import urlsplit
ROOT=Path('/workspace/liminis-evidence/live-mobile-readout-live-review')
ORIGINAL=Path('/workspace/liminis-evidence/live-mobile-readout-failure/live-cells-browser-a57a6854-original.zip')
REPO='/workspace/liminis-live-mobile-readout-integration'
HEAD='a57a6854b633c8e272eb4012ff11a0ec125e6fc2'
EXPECTED_SHA='c0b5cddbc1d5d520e80f079c93bbac2adffc446bff04d1b1bd1d4dac91352ba0'
CAP=64*1024*1024
sha=lambda data:hashlib.sha256(data).hexdigest()
def safe_entries(z):
 infos=z.infolist(); assert len(infos)<=400
 names=[x.filename for x in infos]; assert len(names)==len(set(names))
 for i in infos:
  name=PurePosixPath(i.filename); assert not name.is_absolute() and '..' not in name.parts and '\\' not in i.filename
  assert not (i.external_attr>>16)&0o170000==0o120000
 assert z.testzip() is None
 return infos
patterns={
 'github_token':r'\b(?:gh[pousr]_[A-Za-z0-9]{20,}|github_pat_[A-Za-z0-9_]{20,})',
 'openai_like_token':r'\bsk-(?:proj-|svcacct-)?[A-Za-z0-9_-]{24,}',
 'aws_access_key':r'\b(?:AKIA|ASIA)[A-Z0-9]{16}\b',
 'private_key_material':r'-----BEGIN (?:RSA |EC |OPENSSH )?PRIVATE KEY-----',
 'credential_bearing_url':r'https?://[^\s/<>"\x27]+:[^\s/<>"\x27]+@',
 'bearer_credential':r'(?i)\bBearer\s+[A-Za-z0-9._~-]{20,}',
 'private_chat_or_old_context_marker':r'(?i)(?:chat_[A-Za-z0-9_-]{10,}|thread_[A-Za-z0-9_-]{10,}|private[_ -]?chat[_ -]?id|old[_ -]?vm[_ -]?context)',
 'attachment_or_download_handle':r'(?:sediment://|/workspace/attachments/|file_[A-Za-z0-9]{12,})',
}
text_count=text_bytes=0; findings={k:0 for k in patterns}; text_inventory=[]
def scan(name,data):
 global text_count,text_bytes
 try: text=data.decode('utf-8')
 except UnicodeDecodeError: return
 text_count+=1;text_bytes+=len(data)
 hits={k:len(re.findall(p,text)) for k,p in patterns.items()};
 for k,v in hits.items():findings[k]+=v
 text_inventory.append({'name':name,'bytes':len(data),'sha256':sha(data),'pattern_counts':hits})
raw=ORIGINAL.read_bytes();assert len(raw)==8544954 and sha(raw)==EXPECTED_SHA
with zipfile.ZipFile(io.BytesIO(raw)) as outer:
 oi=safe_entries(outer); assert set(x.filename for x in oi)=={'failure.png','held-30fps.png','held-60fps.png','host.log','report.json','trace.zip','viewer-1440-viewport-inspector-upper.png','viewer-1440-viewport-readout.png','viewer-1440-viewport-stage.png'}
 ob={x.filename:outer.read(x) for x in oi}; report=json.loads(ob['report.json'])
 assert report['gitHead']==HEAD and report['gitTree']=='2f9f0760ada0bbd1812a232bc6a30883242c76d3' and report['status']=='failed'
 for name,data in ob.items():
  if name!='trace.zip':scan('outer/'+name,data)
 with zipfile.ZipFile(io.BytesIO(ob['trace.zip'])) as nested:
  ni=safe_entries(nested);assert sum(x.file_size for x in oi)+sum(x.file_size for x in ni)<=CAP
  nb={x.filename:nested.read(x) for x in ni}
  for name,data in nb.items():scan('trace/'+name,data)
  events=[json.loads(x) for x in nb['trace.trace'].decode().splitlines()]
  networks=[json.loads(x)['snapshot'] for x in nb['trace.network'].decode().splitlines()]
  expected_source=subprocess.check_output(['git','-C',REPO,'show',HEAD+':crates/liminis/src/cell-viewer.html'])
  summaries=[];states=[];nonempty_auth=[]
  for n in networks:
   req=n['request'];res=n['response'];u=urlsplit(req['url']); content=res.get('content',{});ref=content.get('_sha1');body=nb.get('resources/'+ref) if ref else None
   item={'method':req['method'],'path':u.path,'origin_class':'loopback' if u.hostname in ['127.0.0.1','localhost'] else 'other','start_ms':n.get('_monotonicTime'),'duration_ms':n.get('time'),'status':res.get('status'),'declared_content_bytes':content.get('size'),'retained_body_bytes':len(body) if body is not None else None,'retained_body_sha256':sha(body) if body is not None else None}
   if body is not None:
    assert hashlib.sha1(body).hexdigest()==ref.split('.')[0]
    if u.path=='/':item['exact_final_git_html']=body==expected_source
    if u.path=='/api/state':
     s=json.loads(body);states.append((item,s));item['state_tick']=s.get('tick');item['state_running']=s.get('running');item['state_cells']=len(s.get('cells',[]));item['state_dt_seconds']=s.get('dt_seconds')
   for where,hlist in [('request',req.get('headers',[])),('response',res.get('headers',[]))]:
    for h in hlist:
     if h['name'].lower() in ['authorization','proxy-authorization','cookie','set-cookie'] and h.get('value','').strip():nonempty_auth.append({'where':where,'header':h['name'].lower(),'path':u.path})
   assert not req.get('cookies') and not res.get('cookies')
   summaries.append(item)
  late=[(i,s) for i,s in states if i['start_ms']>=6800]
  target=report['responsive']['selectedCell'];late_state_checks=[]
  for i,s in late:
   actual=next((c for c in s['cells'] if c['id']==target['id']),None)
   late_state_checks.append({'start_ms':i['start_ms'],'tick':s['tick'],'paused':s['running'] is False,'selected_cell_exact_to_report':actual==target,'body_sha256':i['retained_body_sha256']})
  actions=[]
  for e in events:
   if e.get('type')=='before' and e.get('startTime',0)>=8000:
    p=e.get('params',{});actions.append({'call_id':e['callId'],'start_ms':e['startTime'],'method':e.get('method'),'selector':p.get('selector'),'title':e.get('title'),'full_page':p.get('fullPage')})
  snapshots=[e['snapshot'] for e in events if e.get('type')=='frame-snapshot']
  def raw_find(node,node_id):
   if isinstance(node,list):
    if len(node)>1 and isinstance(node[0],str) and isinstance(node[1],dict) and node[1].get('id')==node_id:return node
    for c in node:
     r=raw_find(c,node_id)
     if r is not None:return r
   return None
  snapshot_evidence=[]
  for s in snapshots:
   if s['timestamp']>=8200:
    inspector=raw_find(s['html'],'cell-detail');snapshot_evidence.append({'name':s['snapshotName'],'time_ms':s['timestamp'],'raw_root_is_prior_snapshot_reference':isinstance(s['html'],list) and len(s['html'])==1 and isinstance(s['html'][0],list) and all(isinstance(v,int) for v in s['html'][0]),'raw_serialized_inspector_present':inspector is not None,'raw_serialized_inspector_sha256':sha(json.dumps(inspector,ensure_ascii=False,separators=(',',':')).encode()) if inspector is not None else None})
  pngs=[]
  for name,data in ob.items():
   if name.endswith('.png'):
    assert data[:8]==b'\x89PNG\r\n\x1a\n';width,height=struct.unpack('>II',data[16:24]);pngs.append({'name':name,'bytes':len(data),'sha256':sha(data),'width':width,'height':height,'mandated_viewed':name in report['screenshots']})
  forbidden=['--no-sandbox','--disable-sandbox','--disable-setuid-sandbox','--disable-namespace-sandbox','--disable-seccomp-filter-sandbox','--disable-gpu-sandbox','--single-process','--in-process-gpu','--disable-web-security','--ignore-gpu-blocklist']
  absent=[f for f in forbidden if not any(a==f or a.startswith(f+'=') for a in report['browserArguments'])]
  result={
   'original':{'artifact_id':11332948215,'name':ORIGINAL.name,'bytes':len(raw),'sha256':sha(raw),'outer_crc_ok':True,'outer_entries':len(oi),'outer_expanded_bytes':sum(x.file_size for x in oi),'nested_crc_ok':True,'nested_entries':len(ni),'nested_expanded_bytes':sum(x.file_size for x in ni),'combined_expanded_bytes':sum(x.file_size for x in oi)+sum(x.file_size for x in ni),'cap_bytes':CAP,'safe_unique_inventory':True},
   'outer_inventory':[{'name':x.filename,'bytes':x.file_size,'sha256':sha(ob[x.filename])} for x in oi],
   'trace':{'events':len(events),'frame_snapshots':len(snapshots),'network_records':len(networks),'late_actions':actions,'late_snapshot_evidence':snapshot_evidence,'network':summaries,'late_paused_state_checks':late_state_checks,'recorded_page_error_events':sum(e.get('type')=='event' and e.get('method')=='pageError' for e in events),'recorded_navigation_events':sum(e.get('type')=='event' and e.get('method')=='navigated' for e in events)},
   'pngs':pngs,'held_encoded_bytes_equal':ob['held-30fps.png']==ob['held-60fps.png'],
   'sandbox_observation':{'reported_enabled':report['chromiumSandbox'],'browser_version':report['browserVersion'],'playwright':report['playwright'],'observed_forbidden_flags_absent':absent,'software_gpu_args_present':[a for a in report['browserArguments'] if a.startswith(('--use-gl=','--use-angle=','--enable-unsafe-swiftshader'))]},
   'privacy':{'method':'Bounded complete UTF-8 decoding of outer non-ZIP and nested trace entries plus explicit HAR cookie/auth-header check; values not emitted. All four report PNG visually inspected separately.','decoded_text_entries':text_count,'decoded_text_bytes':text_bytes,'pattern_counts':findings,'nonempty_auth_cookie_headers':nonempty_auth,'text_inventory':text_inventory,'qualification':'Heuristic obvious-credential/private-context scan, not exhaustive secret detection.'}
  }
(ROOT/'bounded-analysis.json').write_text(json.dumps(result,ensure_ascii=False,indent=2)+'\n')
print(json.dumps({'integrity':result['original'],'source_body_matches':[n for n in summaries if n.get('path')=='/'],'late_states':late_state_checks,'privacy':{'decoded_text_entries':text_count,'decoded_text_bytes':text_bytes,'pattern_counts':findings,'nonempty_auth_cookie_headers':nonempty_auth},'held_equal':result['held_encoded_bytes_equal'],'pngs':pngs},ensure_ascii=False))
