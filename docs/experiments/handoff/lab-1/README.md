# LAB-1 QA evidence handoff

Evidence-only архивирование ранее ignored QA. Coding/stage/merge/deploy STOPPED.
Ни source, ни PR #14/#15, ни config/chemistry/validator здесь не меняются.
LAB-2 24 runs не пересчитывались и duplicate repeat не включён.

## Сохранность и ограничение

ZIP содержит четыре оригинальных JSON с immutable bytes и один явно redacted
public log. Manifest перечисляет исходные relative paths, bytes/SHA256, archive
paths, сохранённые bytes/SHA256, status, provenance и outcomes.

У исходного negative-control log найден абсолютный путь с приватным VM context.
Только compile path заменён на `<source-checkout>/crates/liminis`; исходный log
не опубликован и остаётся локально. Его SHA/size сохранены. Это явный remaining
unsaved-to-Git gap для одного оригинала; не объявляется byte-exact архивом всех5.
Исторический EXPECTED FAIL — следствие временного удаления byte reservation,
после которого source был восстановлен. Точная временная source mutation не
аттестована commit. При архивировании это испытание не запускалось.

Pattern scan credentials/private context выполнен до публикации. Нулевые hits
относятся к опубликованным decompressed files и metadata, а не доказывают
универсальное отсутствие любого возможного секрета. Original files не удалены.

## Проверка архива без симуляции

Из каталога, содержащего `manifest.json` и `lab-1-qa-evidence.zip`:

```sh
python3 - <<'PYVERIFY'
import hashlib, json, pathlib, zipfile
m = json.loads(pathlib.Path('manifest.json').read_text())
b = pathlib.Path(m['archive_name']).read_bytes()
assert len(b) == m['archive_bytes']
assert hashlib.sha256(b).hexdigest() == m['archive_sha256']
with zipfile.ZipFile(m['archive_name']) as z:
    assert z.testzip() is None
    assert set(z.namelist()) == {r['archive_path'] for r in m['files']}
    for r in m['files']:
        data = z.read(r['archive_path'])
        assert len(data) == r['stored_bytes']
        assert hashlib.sha256(data).hexdigest() == r['stored_sha256']
        if r['status'] == 'original_bytes_preserved':
            assert len(data) == r['original_bytes']
            assert hashlib.sha256(data).hexdigest() == r['original_sha256']
print('Archive bytes, CRC and entry SHA256 verified')
PYVERIFY
```

Распаковка выполняется в новом свободном каталоге (`python3 -m zipfile -e ...`).
Она не требуется для hash проверки и не должна перезаписывать исходные QA files.

## Будущее воспроизведение smoke evidence

Архивирование не выполняло Cargo или повторный numerical run. Для отдельного
явно разрешённого воспроизведения использовать свой новый detached worktree,
Rust1.97.1, release/native FLOAT и immutable архивный smoke-manifest. Не менять
config/guards и не модифицировать historical source ради negative-control log.

```sh
# Переменные указывают на собственные новые каталоги, не чужой checkout.
# lab_evidence_dir — каталог распакованного ZIP; lab_source_checkout — новый path.
git worktree add --detach "$lab_source_checkout" 21cbe90732128edee61963544e414d29a7420577
cd "$lab_source_checkout"
cargo run --locked --release -p liminis --example compare_cell_experiments -- \
  --manifest "$lab_evidence_dir/target/qa/lab-1/smoke-manifest.json" \
  --source-commit 21cbe90732128edee61963544e414d29a7420577 \
  --output target/qa/lab-1/reproduced-final-head-results.json
```

`final-head-results.json` принадлежит указанному commit. Smoke results/repeat
принадлежат `dd0ce4df1c00b51a2412952c9cd37f68591309c6`; для них source worktree и
`--source-commit` должны использовать этот исторический ref. Whole-file digest
содержит runtime provenance и не обещается между hardware/toolchain/profile.
Source checkpoint содержит исходную методику; этот архив новых PASS не заявляет.

На архивной ветке не открыт PR, merge/deploy не выполняется. Передача ownership
и продолжение этапов остаются отдельным действием native coordinator.
