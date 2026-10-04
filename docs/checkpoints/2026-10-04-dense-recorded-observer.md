# REC-2: каждое состояние архивных записей

- Дата/цель: 2026-10-04. По поручению пользователя обновить10k/100k/1M
  записи до каждого тика, сначала измерить bytes; около3GB разрешены для
  проверки существующего размещения. Ранее выданный native ownership/write-go
  сохраняется, coordinator/integrator/release owner один.
- Base main: `61d5c22edb3d1da13029c9faa6b506253d674f91` после принятых PR17/18.
  Ветка интеграции `codex/native-dense-observer-integration`; отдельные scoped
  author/reviewer checkouts сохранены. Frozen SPEC/NORTH_STAR не менялись;
  ADR110 append-only, одобрен реальным Astra. LAB24/старые записи immutable.
- Сделано: browser lossless delta decoder, строгий SHA-chain/CRC/schema,
  bounded current+adjacent chunk cache, abort/stale intent и sticky errors;
  async observer на каждом tick без million timestamps array, buffering
  удерживает validated frame, отдельные model dt/rate/display30/60.
  Archive доступен явно; sparse samples не подменяют dense current state.
  Paused prefetch errors немедленно выключают controls; pagehide закрывает
  reader, persisted pageshow требует новый reader/reload.
- Реальный full dataset и receipt: см.
  `docs/experiments/dense-recording/million-2026-10-04/README.md`.
  1000001frames/3909chunks/2421968874 original bytes; full independent decode,
  603 архивных сравнения, both ledger0 PASS. Published path-only layout
  2424602694bytes со всей metadata, hardcap3000000000. Пересчёт для upload
  не допускается; preserved raw data повторно использовать.
- Source checks: все109Node tests PASS на combined source перед timing-only
  QA follow-up; Python29 PASS; focused current-main Rust example compile
  PASS. Producer actual source/binary/ledger отдельно закреплены архивным
  bb70 ref. Независимые code/numerics reviews decoder/UI/publisher0blockers;
  QA timing blocker закрыт реальными browser click timestamps, source review0.
  Эти результаты не выдаются за browser PASS.
- Реальный браузер: добавлен обязательный dense gate в существующий pinned,
  sandboxed protected recorded job с externally expected HEAD/tree. Проверяет
  lossless samples, corrupt/abort paths, chunk boundary, 10k/100k/1M ends,
  measured1×, held pixels/state, desktop/mobile, source bytes и bounded cache.
  Повторный full decode/симуляция браузером не выполняются.
- Не проверено: exact candidate full CI/browser artifacts, network publication
  всех3909chunks, фактическое размещение/public dense playback. Пока это
  pending, main не меняется и deployment PASS не объявляется.
- Разрешения: scoped codex commit/push/PR, merge после green CI + independent
  review, существующий сайт после приёмки. Без новых infra/secrets/spend и
  security bypass. Старые Work authors STOPPED; их WIP/VM/evidence сохранены.
  Автопробуждение вне активного Cloud turn не подтверждено.
- Следующий шаг: два bounded Git pushes≤1.5GB new gzip bytes; partial branch
  без catalog/PR. После complete source/file validation — exact-head PR/CI,
  independent protected browser artifacts; только затем guarded merge и
  существующая production verification.
