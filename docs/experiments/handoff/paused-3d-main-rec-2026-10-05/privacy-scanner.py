"""Bounded, local-only supplement; never prints matching credential values."""
import collections
import datetime
import gzip
import hashlib
import io
import json
import pathlib
import re
import time
import zipfile

ROOT = pathlib.Path('/workspace/liminis-evidence/paused-3d-main-rec-public-review')
CONFIG = pathlib.Path('/workspace/liminis-evidence/main-rec-archive-config.json')
CAPS = {'file_bytes': 32 * 1024 * 1024, 'total_read_bytes': 512 * 1024 * 1024,
        'entry_count': 4096, 'nested_zip_depth': 2, 'elapsed_seconds': 180}
START = time.monotonic()
PUBLIC_TOKEN = 'f62c9cac700945e5b371e0dc055cbb61'
PUBLIC_INSERTION_SHA = 'bbba70d1fbb140fe2cff2d40386e726bfe911227760ad6e69e29644e42b6f40a'
total_read = 0
entries = 0
findings = []
gaps = []
inventory = []
stats = collections.Counter()
public = collections.Counter()

RULES = {
    'private_key_block': r'-----BEGIN (?:RSA |EC |DSA |OPENSSH |ENCRYPTED )?PRIVATE KEY-----',
    'github_token': r'\b(?:gh[opusr]_[A-Za-z0-9]{36,255}|github_pat_[A-Za-z0-9_]{30,255})\b',
    'openai_secret_key': r'\bsk-(?:(?:proj|svcacct)-)?[A-Za-z0-9_-]{32,255}\b',
    'aws_access_key': r'\b(?:AKIA|ASIA)[A-Z0-9]{16}\b',
    'slack_token': r'\bxox[baprs]-[0-9]{8,16}-[0-9]{8,16}-[A-Za-z0-9]{16,80}\b',
    'google_api_key': r'\bAIza[0-9A-Za-z_-]{35}\b',
    'tailscale_auth_key': r'\btskey-(?:auth|api)-[A-Za-z0-9_-]{20,200}\b',
    'concrete_authorization': r'(?i)\b(?:authorization|proxy-authorization)\s*["\']?\s*[:=]\s*["\']?\s*(?:Bearer|Basic)\s+[A-Za-z0-9+/=._-]{12,255}',
    'signed_storage_query': r'(?i)[?&](?:X-Amz-(?:Credential|Signature|Security-Token)|X-Goog-(?:Credential|Signature)|sig)=[A-Za-z0-9%+/=_-]{12,400}',
    'concrete_secret_assignment': r'(?i)\b(?:AWS_SECRET_ACCESS_KEY|SSH_PRIVATE_KEY|OPENAI_API_KEY|GITHUB_TOKEN|GH_TOKEN|API_SECRET|SECRET_KEY|ACCESS_TOKEN)\s*["\']?\s*[:=]\s*["\'][A-Za-z0-9+/=._-]{20,255}["\']',
    'private_vm_ssh_identity': r'\b(?:root|ubuntu|ec2-user)@(?:[0-9]{1,3}\.){3}[0-9]{1,3}\b',
    'private_network_url': r'(?i)https?://(?:10\.[0-9]{1,3}\.[0-9]{1,3}\.[0-9]{1,3}|192\.168\.[0-9]{1,3}\.[0-9]{1,3}|172\.(?:1[6-9]|2[0-9]|3[01])\.[0-9]{1,3}\.[0-9]{1,3})(?=[:/\s"\'])',
    'private_user_credential_path': r'/home/(?!runner/)[A-Za-z0-9_.-]{1,60}/(?:\.ssh/(?:id_rsa|id_ed25519|authorized_keys)|\.codex/(?:auth|sessions)|\.config/(?:openai|codex)/)',
    'private_chat_link': r'(?i)https?://(?:chatgpt\.com|chat\.openai\.com)/c/[0-9a-f-]{30,40}',
    'private_chat_protocol': r'<\|(?:im_start|start_header_id)\|>(?:system|user|assistant)',
    'chat_message_record': r'"role"\s*:\s*"(?:system|user|assistant)"\s*,\s*"content"\s*:',
}
COMPILED = {name: re.compile(pattern) for name, pattern in RULES.items()}


def check():
    if time.monotonic() - START > CAPS['elapsed_seconds']:
        raise RuntimeError('scan elapsed bound reached')


def charge(n):
    global total_read
    total_read += n
    if total_read > CAPS['total_read_bytes']:
        raise RuntimeError('scan aggregate read bound reached')
    check()


def bounded(stream, cap):
    parts, n = [], 0
    while True:
        check()
        chunk = stream.read(min(1024 * 1024, cap + 1 - n))
        if not chunk:
            return b''.join(parts)
        charge(len(chunk))
        n += len(chunk)
        if n > cap:
            raise ValueError('per-file scan bound reached')
        parts.append(chunk)


def pin_file(f):
    p = pathlib.Path(f['source'])
    if p.stat().st_size != f['bytes'] or f['bytes'] > CAPS['file_bytes']:
        raise ValueError('input size pin or cap mismatch')
    h = hashlib.sha256()
    with p.open('rb') as stream:
        while True:
            chunk = stream.read(1024 * 1024)
            if not chunk:
                break
            charge(len(chunk))
            h.update(chunk)
    if h.hexdigest() != f['sha256']:
        raise ValueError('input digest pin mismatch')
    return {'path': str(p), 'bytes': f['bytes'], 'sha256': f['sha256']}


def finding(path, rule, count=1, offset=None):
    record = {'path': path, 'rule': rule, 'count': count}
    if offset is not None:
        record['character_offset'] = offset
    findings.append(record)


def inspect_http(value, path):
    # Inspect concrete JSON header/cookie containers, not literal guard code.
    stack = [value]
    nodes = 0
    while stack:
        v = stack.pop()
        nodes += 1
        if nodes > 100000:
            gaps.append({'path': path, 'reason': 'HTTP structural node cap 100000'})
            return
        if isinstance(v, dict):
            if isinstance(v.get('name'), str) and 'value' in v:
                if v['name'].lower() in ('authorization', 'proxy-authorization', 'cookie', 'set-cookie') and v['value'] not in ('', None):
                    finding(path, 'concrete_sensitive_http_header')
            for k, child in v.items():
                if k.lower() in ('authorization', 'proxy-authorization', 'cookie', 'set-cookie') and isinstance(child, str) and child:
                    finding(path, 'concrete_sensitive_http_header_map')
                if k.lower() in ('cookies', 'requestcookies', 'responsecookies') and isinstance(child, list) and child:
                    finding(path, 'nonempty_http_cookie_array')
                stack.append(child)
        elif isinstance(v, list):
            stack.extend(v)


def scan_text(data, path, kind):
    try:
        text = data.decode('utf-8', errors='strict')
    except UnicodeDecodeError:
        stats['excluded_non_utf8_files'] += 1
        stats['excluded_non_utf8_bytes'] += len(data)
        inventory.append({'path': path, 'bytes': len(data), 'status': 'EXCLUDED', 'reason': 'strict UTF-8 decode refused'})
        return
    if '\x00' in text:
        stats['excluded_binary_nul_files'] += 1
        stats['excluded_binary_nul_bytes'] += len(data)
        inventory.append({'path': path, 'bytes': len(data), 'status': 'EXCLUDED', 'reason': 'NUL-bearing binary; not ordinary UTF-8 text'})
        return
    stats['scanned_utf8_files'] += 1
    stats['scanned_utf8_bytes'] += len(data)
    stats['scanned_' + kind + '_files'] += 1
    inventory.append({'path': path, 'bytes': len(data), 'sha256': hashlib.sha256(data).hexdigest(), 'status': 'SCANNED_UTF8', 'kind': kind})
    for rule, rx in COMPILED.items():
        matches = list(rx.finditer(text))
        if matches:
            finding(path, rule, len(matches), matches[0].start())
        check()
    # This exact token is public metadata in the independently pinned insertion.
    n = text.count(PUBLIC_TOKEN)
    if n:
        public['exact_public_cloudflare_token_occurrences'] += n
        public['files_with_exact_public_cloudflare_token'] += 1
    public['public_analytics_script_url_occurrences'] += text.count('https://static.cloudflareinsights.com/beacon.min.js/')
    if path.endswith('-observe-raw.html'):
        insertion = data[6878:6878 + 367]
        if len(data) != 7261 or hashlib.sha256(insertion).hexdigest() != PUBLIC_INSERTION_SHA:
            raise ValueError('public analytics insertion pin mismatch')
        public['raw_html_exact_insertion_pins'] += 1
    # .network is JSONL; reports are JSON. No source-string evaluation/unescape.
    if path.endswith('.network'):
        for line in text.splitlines():
            if line.strip():
                try:
                    inspect_http(json.loads(line), path)
                    stats['http_jsonl_records'] += 1
                except json.JSONDecodeError:
                    gaps.append({'path': path, 'reason': 'network JSONL parse refusal'})
    elif kind == 'outer_text' and path.endswith('.json'):
        try:
            inspect_http(json.loads(text), path)
            stats['outer_report_json_records'] += 1
        except json.JSONDecodeError:
            gaps.append({'path': path, 'reason': 'outer report JSON parse refusal'})


def walk_zip(z, prefix, depth):
    global entries
    for i in z.infolist():
        if i.is_dir():
            continue
        entries += 1
        if entries > CAPS['entry_count']:
            raise RuntimeError('archive entry count bound reached')
        path = prefix + '!' + i.filename
        if i.file_size > CAPS['file_bytes']:
            gaps.append({'path': path, 'bytes': i.file_size, 'reason': 'per-file bound exclusion'})
            continue
        with z.open(i) as stream:
            data = bounded(stream, CAPS['file_bytes'])
        stats['archive_member_files'] += 1
        stats['archive_member_bytes'] += len(data)
        if data.startswith(b'PK\x03\x04'):
            if depth >= CAPS['nested_zip_depth']:
                gaps.append({'path': path, 'reason': 'nested ZIP depth exclusion'})
                continue
            stats['nested_zip_containers'] += 1
            with zipfile.ZipFile(io.BytesIO(data)) as nested:
                walk_zip(nested, path, depth + 1)
        elif data.startswith(b'\x1f\x8b'):
            try:
                with gzip.GzipFile(fileobj=io.BytesIO(data)) as stream:
                    decoded = bounded(stream, CAPS['file_bytes'])
                stats['gzip_decoded_files'] += 1
                stats['gzip_encoded_bytes'] += len(data)
                stats['gzip_decoded_bytes'] += len(decoded)
                scan_text(decoded, path + '!gzip-decoded', 'gzip_text')
            except (OSError, EOFError, ValueError):
                gaps.append({'path': path, 'reason': 'bounded gzip decode refused'})
        elif data.startswith((b'\x89PNG\r\n\x1a\n', b'\xff\xd8\xff', b'GIF87a', b'GIF89a')) or (data.startswith(b'RIFF') and data[8:12] == b'WEBP'):
            stats['excluded_image_files'] += 1
            stats['excluded_image_bytes'] += len(data)
            inventory.append({'path': path, 'bytes': len(data), 'status': 'EXCLUDED', 'reason': 'recognized binary image; no OCR/metadata scan'})
        else:
            scan_text(data, path, 'outer_text' if depth == 0 else 'nested_text')


def main():
    with CONFIG.open('rb') as stream:
        config_bytes = bounded(stream, CAPS['file_bytes'])
    c = json.loads(config_bytes.decode('utf-8', errors='strict'))
    inputs = c['files']
    before = [pin_file(f) for f in inputs]
    for f in inputs:
        if f['source'].endswith('.zip'):
            with zipfile.ZipFile(f['source']) as z:
                walk_zip(z, f['name'], 0)
        else:
            with pathlib.Path(f['source']).open('rb') as stream:
                data = bounded(stream, CAPS['file_bytes'])
            scan_text(data, f['name'], 'frozen_review_text')
    after = [pin_file(f) for f in inputs]
    assert before == after
    report = {
        'schema_version': 1, 'kind': 'bounded_utf8_privacy_supplement',
        'created_at_utc': datetime.datetime.now(datetime.timezone.utc).isoformat(),
        'source_head': c['head'], 'source_tree': c['tree'],
        'verdict': 'CHANGES_REQUESTED' if findings else ('INCOMPLETE' if gaps else 'QUALIFIED_0_BLOCKERS'),
        'blockers': findings, 'blocker_count': len(findings), 'blocker_candidates': findings, 'coverage_gaps': gaps,
        'scope': 'Local scan of all ordinary UTF-8 outer and nested trace resources of the three pinned original ZIPs, bounded gzip decoding, plus two unchanged frozen review texts; independent actual review not repeated.',
        'caps': CAPS, 'observed': dict(stats), 'total_read_bytes_including_pin_rereads': total_read,
        'archive_entry_count': entries, 'elapsed_seconds': round(time.monotonic() - START, 3),
        'inputs_before_and_after_equal': True, 'inputs': before,
        'archive_config': {'path': str(CONFIG), 'bytes': len(config_bytes), 'sha256': hashlib.sha256(config_bytes).hexdigest()},
        'public_intended_analytics_metadata': {
            'classification': 'Known intended public HTML hosting metadata; not a private bearer credential. No analytics execution claim.',
            'exact_insertion_bytes': 367, 'exact_insertion_sha256': PUBLIC_INSERTION_SHA,
            'cloudflare_token': PUBLIC_TOKEN, 'counts': dict(public)},
        'high_precision_rules': RULES,
        'http_structure_checks': 'Concrete Authorization/Proxy-Authorization/Cookie/Set-Cookie nonempty header values/maps and nonempty cookie arrays in all .network JSONL and outer JSON reports.',
        'matching_values_policy': 'No credential match values or matching text snippets emitted or saved; only rule/path/count/offset candidates.',
        'inventory': inventory,
        'scanner_sha256': hashlib.sha256(pathlib.Path(__file__).read_bytes()).hexdigest(),
        'limitations': ['Bounded strict UTF-8/gzip text scan is not an exhaustive secret guarantee.',
                       'Binary images excluded; no new visual/OCR review, steganography, arbitrary binary metadata, base64 secrets or encrypted content scan.',
                       'Generic names such as token, authorization, cookie or private are not alone treated as secrets; intended exact public analytics identifier classified separately.',
                       'Private VM/chat coverage uses the documented obvious high-precision patterns; unknown hostnames, paraphrased conversations or unlisted identifiers can escape detection.',
                       'No HTTP/browser/tests/model/Git/source edits, no actual evidence reacceptance, no LAB or interoperability audit.']}
    with (ROOT/'privacy-supplement.json').open('x') as f:
        f.write(json.dumps(report, ensure_ascii=False, indent=2) + '\n')
    for key in ('verdict','caps','observed','total_read_bytes_including_pin_rereads','archive_entry_count','elapsed_seconds','blocker_candidates','coverage_gaps','public_intended_analytics_metadata','scanner_sha256'):
        print(json.dumps({key:report[key]}, ensure_ascii=False))


if __name__ == '__main__':
    main()
