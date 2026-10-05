import hashlib
import json
import pathlib
import subprocess

root = pathlib.Path('/workspace/liminis-evidence/recorded-hud-main')
old = pathlib.Path('/workspace/liminis-evidence/recorded-hud-candidate/originals')
head = '813be1efad29569a3011f00697bd5d41b7397dc6'
tree = 'c5775c00a150768f2819e265a4165d8605bd48e3'
run = 37273770904
repo = '/workspace/liminis-recorded-hud-main-acceptance'
def git_bytes(path):
    return subprocess.check_output(['git', '-C', repo, 'show', head + ':' + path])
def contains(a, b):
    assert a['left'] <= b['left'] <= b['right'] <= a['right']
    assert a['top'] <= b['top'] <= b['bottom'] <= a['bottom']
def hud(h):
    rows = h['rows']
    for key in ['header', 'title', 'stats', 'footer', 'canvas']:
        r = rows[key]
        assert r['width'] > 0 and r['height'] > 0
        assert r['display'] != 'none' and r['visibility'] == 'visible' and r['opacity'] > 0
        contains(h['stage'], r)
        assert r['scrollWidth'] <= r['clientWidth'] and r['scrollHeight'] <= r['clientHeight']
        for text in r['text']:
            contains(r, text)
    contains(rows['header'], rows['title'])
    contains(rows['header'], rows['stats'])
    assert rows['header']['bottom'] <= rows['canvas']['top']
    assert rows['canvas']['bottom'] <= rows['footer']['top']
    assert rows['canvas']['width'] > 72 and rows['canvas']['height'] > 72
    assert h['roundedInterior']['width'] > 0 and h['roundedInterior']['height'] > 0
    assert h['loadingHidden'] and all(h['canvasUnobscured'])

reports = {}
layout_count = 0
geometry_count = 0
for label, name, count in [('recorded', 'evidence.json', 22), ('dense', 'report.json', 14)]:
    path = root / 'originals' / label / name
    d = json.loads(path.read_text())
    previous = json.loads((old / label / name).read_text())
    assert d['status'] == 'PASS' and len(d['checks']) == count
    assert [x['name'] for x in d['checks']] == [x['name'] for x in previous['checks']]
    assert all(x['status'] == 'PASS' for x in d['checks'])
    assert str(d['workflowRun']) in {str(run), 'https://github.com/emevart/liminis/actions/runs/' + str(run)}
    assert int(d['workflowAttempt']) == 1
    assert d['sourceCommit' if label == 'recorded' else 'sourceHead'] == head
    assert d['sourceTree'] == tree and d['launchArgumentCheck']['status'] == 'PASS'
    assert d['nodeVersion'] == previous['nodeVersion'] and d['browser'] == previous['browser']
    sources = d['sourceFiles' if label == 'recorded' else 'sources']
    for row in sources:
        path_in_git = ('site/' if label == 'recorded' else '') + row['filename']
        raw = git_bytes(path_in_git)
        sha = hashlib.sha256(raw).hexdigest()
        assert row['sha256'] == sha
        if 'bytes' in row:
            assert row['bytes'] == len(raw)
        if 'servedSha256' in row:
            assert row['servedSha256'] == sha
    assert len(d['layouts']) == len(previous['layouts'])
    for x, earlier in zip(d['layouts'], previous['layouts']):
        assert x['viewport'] == earlier['viewport'] and x['stateBefore'] == earlier['stateBefore']
        selection = x['selection']
        assert selection['clearedSelection'] == 'none selected'
        for key in ['tick', 'cells', 'selectedId', 'inspector', 'inventoryBefore', 'inventoryAfter']:
            assert selection[key] == earlier['selection'][key], (label, key)
        assert selection['inventoryBefore'] == selection['inventoryAfter']
        assert selection['ring']['pixels'] >= 4
        hud(x['hudBefore']); geometry_count += 1
        if 'hudAfterScreenshot' in x:
            hud(x['hudAfterScreenshot']); geometry_count += 1
        layout_count += 1
    if label == 'recorded':
        assert len(d['httpChecks']) == 20 and all(x['status'] == 'PASS' for x in d['httpChecks'])
        assert d['httpRequestCoverage']['status'] == 'PASS'
        assert d['recordings'] == previous['recordings']
        assert d['scriptSha256'] == hashlib.sha256(git_bytes('site/playback.browser.mjs')).hexdigest()
    else:
        for key in ['pageErrors', 'consoleErrors', 'unhandled', 'cleanupErrors']:
            assert d[key] == []
        assert d['finalRequestClassification']['status'] == 'PASS' and d['ui']['status'] == 'PASS'
        assert d['ui']['atomicBoundary']['fromTick'] == 255 and d['ui']['atomicBoundary']['toTick'] == 256
        assert d['ui']['atomicBoundary']['heldDuringBuffer'] and d['ui']['atomicBoundary']['delayExcludedFromPlayhead']
        hold = d['ui']['hold']
        assert hold['tick'] == 992 and hold['rate'] == 1 and hold['targets'] == [30, 60] and hold['httpRequests'] == 0
        assert abs(hold['modelSecondsAdvanced'] - hold['measuredWallMs']/1000) <= hold['toleranceSeconds']
        assert [(x['horizon'], x['shownTick'], x['chunkRequests']) for x in d['ui']['endpoints']] == [(100000, 100000, 3), (1000000, 1000000, 3)]
        for endpoint in d['ui']['endpoints']:
            hud(endpoint['initialHud']); hud(endpoint['finalHud']); geometry_count += 2
        assert d['fixture'] == previous['fixture']
    reports[label] = {'bytes': path.stat().st_size, 'sha256': hashlib.sha256(path.read_bytes()).hexdigest(), 'gates': count, 'source_pins': len(sources), 'layouts': len(d['layouts'])}

out = {'status': 'PASS_FRESH_MAIN_REGRESSION_REPORT_INSPECTION', 'head': head, 'tree': tree, 'run_id': run, 'attempt': 1, 'reports': reports, 'layout_selection_cases': layout_count, 'strict_hud_geometry_records': geometry_count, 'qualification': 'Passive fresh report/source-pin inspection under identical-tree bridge and Astra scope. Candidate full independent recorded/dense PNG/trace review remains bound to d8e. This is not a new independent full-artifact MAIN or GL review. Fresh fullCI and independent actual public acceptance/readback remain separate gates.'}
(root / 'recorded-dense-report-inspection.json').write_text(json.dumps(out, indent=2) + '\n')
print(json.dumps(out))
