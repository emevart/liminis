export const meta = {
  name: 'doc-sync',
  description: 'Check the living documents against the code and report drift; frozen documents are read, never edited',
  whenToUse:
    'After a milestone. ARCHITECTURE, QUANTITIES, CONFIG_SCHEMA and ACCEPTANCE are maintained in step with the code; SPEC and NORTH_STAR are frozen, so drift there becomes a line in the journal instead of an edit.',
  phases: [
    { title: 'Check', detail: 'one reader per document, claim by claim against the code' },
    { title: 'Merge', detail: 'one pass over all drift for the same claim told twice' },
  ],
}

const LIVING = [
  {
    file: 'docs/ARCHITECTURE.md',
    checks: `Карта модулей против crates/liminis-core/src/. Сигнатура ядра против того, что в kernels/.
Оба скелета — диффузия и реакции — против настоящего кода: имена, порядок аргументов, типы срезов,
поля структур параметров. Утверждение «kernels тянет только numeric» против настоящих use.
Список того, что в kernels/ не кладётся, против того, что там лежит.`,
  },
  {
    file: 'docs/QUANTITIES.md',
    checks: `Каждая строка таблиц: единица, класс, тип хранения, масштаб, где объявлена — против кода и конфига.
Особое внимание: величины, у которых в коде появилась единица, отличная от объявленной; величины,
объявленные выводимыми, но заданные числом; раздел «Что не объявлено нигде» — не закрылось ли что-то
из него новым решением, и не появилось ли нового.`,
  },
  {
    file: 'docs/CONFIG_SCHEMA.md',
    checks: `Каждый ключ против структур в crates/liminis-core/src/config*: имя, тип, единица, обязательность,
умолчание. Таблица отказов §10 против настоящих имён тестов. Раздел §11 про хеш против реализации.
Пример сценария §12 — грузится ли он сегодня, и если нет, названы ли причины честно. Раздел §13 —
какие пункты закрылись новыми записями журнала.`,
  },
  {
    file: 'docs/ACCEPTANCE.md',
    checks: `Каждое имя теста: существует ли тест с таким именем, и утверждает ли он то, что описано рядом.
Отдельно ищи тест, существующий в коде под другим именем — это хуже отсутствующего, потому что
выглядит как покрытие. И тест, который был бы зелёным на неверном коде.`,
  },
]

const FROZEN = [
  {
    file: 'docs/SPEC.md',
    checks: `Спека ЗАМОРОЖЕНА (ADR-032) и не правится ни при каких условиях. Твоя задача — найти места, где
код ей уже не соответствует, и сказать, чем это оформляется: новой записью журнала или строкой в
docs/ACCEPTANCE.md. Прецедент уже есть — трактовка теста согласованности масштаба в §10, переписанная
ADR-054. Расхождение здесь штатно и накапливается; молчаливое расхождение — нет.`,
  },
]

const DRIFT_SCHEMA = {
  type: 'object',
  additionalProperties: false,
  required: ['drift'],
  properties: {
    drift: {
      type: 'array',
      items: {
        type: 'object',
        additionalProperties: false,
        required: ['section', 'document_says', 'code_says', 'fix'],
        properties: {
          section: { type: 'string' },
          document_says: { type: 'string' },
          code_says: { type: 'string' },
          fix: { type: 'string', description: 'Что править: документ, код, или запись в журнал' },
          severity: { type: 'string', enum: ['misleading', 'stale', 'note'] },
        },
      },
    },
  },
}

phase('Check')

const checked = await parallel(
  [...LIVING, ...FROZEN].map((doc) => () =>
    agent(
      `Ты сверяешь документ ${doc.file} проекта Liminis с настоящим кодом в crates/.

ЧТО СВЕРЯТЬ:
${doc.checks}

КАК. Иди по документу утверждение за утверждением и проверяй КАЖДОЕ по коду. Не по памяти и не по
соседнему документу — по файлам. Документ, отставший от кода, врёт ровно тогда, когда его читают
как источник истины, и это худший момент.

ЧТО ВЕРНУТЬ. На каждое расхождение: раздел, что говорит документ, что говорит код, и что править.
Обрати внимание на направление правки — оно не всегда «править документ». Иногда прав документ, а
код от него отступил, и тогда находка про код.

${doc.file === 'docs/SPEC.md' ? 'ЭТОТ ДОКУМЕНТ ЗАМОРОЖЕН. Ни одной правки. Направление правки для него — всегда «запись в журнал» или «строка в ACCEPTANCE».' : 'Правок файлов не делай — только отчёт. Правит человек, увидев весь список сразу.'}

misleading означает: читатель, поверивший документу, напишет неверный код.`,
      { label: `check:${doc.file.split('/').pop()}`, phase: 'Check', schema: DRIFT_SCHEMA, effort: 'high' },
    ),
  ),
)

const drift = checked
  .filter(Boolean)
  .flatMap((r, i) => r.drift.map((d) => ({ ...d, file: [...LIVING, ...FROZEN][i].file })))

log(`${drift.length} расхождений по ${LIVING.length + FROZEN.length} документам`)

if (!drift.length) return { drift: [], plan: 'документы в согласии с кодом' }

// Барьер оправдан: одно и то же расхождение обычно видно из двух документов
// сразу, и правка нужна одна. Судить об этом можно только увидев оба отчёта.
phase('Merge')

const plan = await agent(
  `Ты сводишь список расхождений между документами и кодом проекта Liminis в план правок.

РАСХОЖДЕНИЯ:
${drift.map((d) => `[${d.file} — ${d.section}] ${d.severity || 'stale'}\n  документ: ${d.document_says}\n  код: ${d.code_says}\n  предложено: ${d.fix}`).join('\n\n')}

СДЕЛАЙ ТРИ ВЕЩИ:

1. Склей расхождения, которые на деле одно: одна величина, названная по-разному в двух документах,
   даёт две находки и требует одной правки.
2. Раздели по направлению: правится документ / правится код / нужна запись в docs/DECISIONS.md.
   Последнее — единственный законный исход для расхождений со SPEC.md и NORTH_STAR.md: они заморожены.
3. Отсортируй по тому, насколько дорого обходится вера в неверное утверждение. Наверх — то, из-за
   чего кто-то напишет неверный код; вниз — то, что просто устарело.

Верни план как список правок, каждая с файлом и одним предложением о том, что именно меняется.`,
  { label: 'merge', phase: 'Merge', effort: 'high' },
)

return { drift, plan }
