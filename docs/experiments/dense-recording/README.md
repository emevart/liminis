# Everytick storage preflight

2026-10-04. Пользователь поручил пересобрать прежние три публичные записи:
каждый тик на всём горизонте, сначала оценить объём хранения. Старые URL/JSON/SHA
сохраняются как immutable archive; LAB24/source/raw не пересчитываются.

`storage-preflight.json` получен чтением existing catalog/трёх201-frame JSON,
без modelsteps. Root независимо повторил Astra расчёт: compact JSON каждого
sample, независимые chunks64samples, gzip6/mtime0; средний размер умножен на
число будущих everytick состояний. Десятичные единицы:

| Horizon | States including genesis | JSON estimate | Gzip estimate |
|---|---:|---:|---:|
| 10k | 10001 | 300654441 bytes | 40431854 bytes |
| 100k | 100001 | 2662185328 bytes | 347718403 bytes |
| 1M | 1000001 | 26404832375 bytes | 3591506079 bytes |

Это sparse-sample экстраполяция, не измерение adjacenttick compression и не
верхняя граница. Среднее sample living108.26/95.07/92.74; max_cells512.
Astra lossless tuples+gzip на тех жеsamples даёт примерно34.0/300.6/3132MB;
удаление повторяемых JSON ключей само по себе не решает объём1M.

Новый bounded codec хранит каждый настоящий tick без потери значений:
independent keyframe chunks, ID definitions и изменения mass/energy/starvation,
birth/removal и accounting/resources. Width/overflow проверяются, данные не
округляются и cadence не снижается при превышении бюджета. Browser не держит
millionframes array; только currentchunk и bounded prefetch/seek. Один1M run
может обслуживать три horizons общими immutable chunks вместо повторения prefixes.

Первый следующий gate — actual10k everytick export на чистой архивной базе:
actual producer commit отдельно от historical engine reference. Engine/config/
Cargo/toolchain должны побайтно совпасть со старымsource, world30/chamber1,
seed42/dt30. Сравнение всех561old unique контрольныхticks при полном новомrun;
на bounded prefix проверяется соответствующая часть. Новые transient genotype
entries допустимы, старые frame values/известные genome definitions неизменны.

Предложенные инженерные бюджеты: chunk≤1MiBcompressed/4MiBdecoded/256ticks;
pilot10k≤64MiB; full target≤256MiB/hardstop512MiB/8192chunks. Это наши gates,
а не проверенные ограничения Cloudflare тарифа. Dense compression ещё не
измерен, actual codec/decode/source/browser acceptance pending. Новая
инфраструктура, секреты и расходы не разрешены; hosted3GB rollout не выполнен.
