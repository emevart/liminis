import importlib.util,json,platform,tempfile,time,hashlib,subprocess
from pathlib import Path
root=Path.cwd()
spec=importlib.util.spec_from_file_location('acceptance',root/'scripts/check_live_cells_pacing.py')
mod=importlib.util.module_from_spec(spec);spec.loader.exec_module(mod)
rows=[]
for label,limit in [('first1000',1000),('first2000',2000),('five_seconds',None)]:
    with tempfile.TemporaryDirectory(prefix='liminis-occupied-benchmark-') as tmp:
        host=mod.Host(root/'target/release/liminis',root,Path(tmp))
        try:
            host.control('pause');host.settled();host.control('reset');before=host.settled()
            assert not before['running'] and before['tick']==0
            host.control('maximum');start=time.monotonic();host.control('run')
            samples=[{'tick':before['tick'],'living_cells':before['summary']['living_cells']}];latencies=[]
            deadline=start+8
            while time.monotonic()<deadline:
                state,latency=host.request('/api/state');latencies.append(latency)
                samples.append({'tick':state['tick'],'living_cells':state['summary']['living_cells']})
                assert not state['error'],state['error']
                if (limit is not None and state['tick']>=limit) or (limit is None and time.monotonic()-start>=5):break
                time.sleep(.005)
            else:raise AssertionError('bounded benchmark deadline')
            end,pause_latency=host.control('pause');elapsed=time.monotonic()-start
            samples.append({'tick':end['tick'],'living_cells':end['summary']['living_cells']})
            settled=host.settled();time.sleep(.05);assert host.state()['tick']==end['tick']
            counts=[s['living_cells'] for s in samples]
            row={'name':'live_cells_maximum_cell_chamber_seed42_release_'+label,'dt_seconds':end['dt_seconds'],'start':samples[0],'end':samples[-1],'observed_living_min':min(counts),'observed_living_max':max(counts),'all_observed_occupied':min(counts)>0,'first_observed_zero':next((s for s in samples if not s['living_cells']),None),'wall_seconds_including_http_and_pause':elapsed,'ticks':end['tick']-before['tick'],'actual_tps':(end['tick']-before['tick'])/elapsed,'actual_multiplier':(end['tick']-before['tick'])*end['dt_seconds']/elapsed,'polls':len(latencies),'http_max_ms':max(latencies)*1000,'pause_ms':pause_latency*1000,'save_ack_tick':settled['persistence']['saved_tick'],'samples':samples}
            rows.append(row)
            print(json.dumps({k:v for k,v in row.items() if k!='samples'}),flush=True)
        finally:host.close()
report={'platform':platform.platform(),'cpu':next((line.strip().split(':',1)[1].strip() for line in Path('/proc/cpuinfo').read_text().splitlines() if line.startswith('model name')),None),'git_head':subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip(),'binary_sha256':hashlib.sha256((root/'target/release/liminis').read_bytes()).hexdigest(),'benchmarks':rows}
(root/'target/qa/occupied_cells_benchmark.json').write_text(json.dumps(report,indent=2)+'\n')
