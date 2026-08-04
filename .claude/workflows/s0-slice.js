export const meta = {
  name: 's0-slice',
  description: 'Build a slice of the simulator test-first: contract, TDD build, independent verify, bounded fix loop',
  whenToUse:
    'When implementing work items against a written plan. Items inside one wave must touch disjoint files; waves run in order. Every item goes red-then-green and is verified by an agent that did not write it.',
  phases: [
    { title: 'Contract', detail: 'read-only: files, signatures, test names, ADR constraints' },
    { title: 'Build', detail: 'tests first, confirm red, then code until green' },
    { title: 'Verify', detail: 'an independent agent runs the gate and reads the diff' },
    { title: 'Fix', detail: 'bounded loop over what verification found' },
  ],
}

// args: { waves: [[item, ...], ...] }
// item: { id, title, files: [..], tests: [..], adrs: [..], notes }
//
// Waves, not a flat list, and not pipeline(): every stage after Contract edits
// the working tree. Two agents editing crates/liminis-core/src/lib.rs at the
// same moment do not conflict loudly, they conflict silently — one overwrites
// the other's module declaration and the build still passes. So concurrency is
// declared by the caller, per wave, over file sets it has checked are disjoint.

// Accepts the argument as an object or as a JSON string: the two look identical
// at the call site and differ only in what reaches the script.
const input = typeof args === 'string' ? JSON.parse(args) : args || {}
const WAVES = input.waves || []
if (!WAVES.length) {
  throw new Error('s0-slice needs args.waves: an array of waves, each an array of work items')
}

const GATE = `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace && python3 scripts/check_bare_q.py`

const RULES = `
ПРАВИЛА ПРОЕКТА, обязательные и проверяемые.

ЧИТАЙ ПЕРЕД РАБОТОЙ: CLAUDE.md, docs/ARCHITECTURE.md, docs/NUMERIC.md, .claude/rules/kernels.md,
и те записи docs/DECISIONS.md, которые названы в задании. Журнал большой — ищи grep по «## ADR-0NN».

ФОРМА ЯДРА (ADR-015, ADR-034). Всё под crates/liminis-core/src/kernels/ — свободные функции вида
fn kernel(src: &[..], dst: &mut [..], p: &Params, idx: u32), переводимые в WGSL построчно.
Внутри разрешены локальные скаляры, массивы фиксированного размера, for по диапазону, ветвления.
Запрещены self, трейты, дженерики по поведению, замыкания, Vec, Box, dyn, HashMap, рекурсия,
любые аллокации, чтение из буфера записи, зависимость результата от порядка обхода.
Параметры сворачиваются на хосте: ядро получает alpha одним числом и не знает ни D, ни dt, ни dx.

GATHER, А НЕ SCATTER (ADR-034). Воксель считает своё новое значение сам и пишет только в свою
ячейку. Сохранение следует из антисимметрии потока: flux(a, b) == -flux(b, a) при любых a и b.
Держится она на округлении половин от нуля. floor ломает её молча, по единице на грань, и не
ловится ни компилятором, ни grep — только тестом flux_is_antisymmetric.

НИКАКИХ ГОЛЫХ ОПЕРАТОРОВ НАД Q (ADR-022). Только обёртки qadd, qsub, qmul, qdiv, qexp, qlog,
qpow, qsqrt, qrcp, qsigmoid. Переходов между классами ровно три: q_conc, m_delta, xi.
Над M операторы + и - разрешены и точны.

ЦЕЛЫЕ КОЛИЧЕСТВА (ADR-004, ADR-026). Количества, энергия и объёмы целые, единица молярная.
Масштаб и разрядность не назначаются, а выводятся при загрузке (ADR-039, ADR-040).

ВЕРСИЯ СЕМАНТИКИ (ADR-020). Правка configs/**, kernels/** или process/** без инкремента
WORLD_FORMAT_VERSION в crates/liminis-core/src/version.rs роняет CI. Двигай его один раз на
задание, а не на файл, и объясняй в комментарии, что именно изменилось в семантике.

ЗАМОРОЖЕНО (ADR-032). docs/SPEC.md и docs/NORTH_STAR.md не правятся вовсе. docs/archive/** тоже.
Расхождение с ними — строка в docs/DECISIONS.md, а не правка документа.

СТИЛЬ. Комментарии и документация в коде — по-английски, как во всём существующем коде.
Комментарий объясняет ПОЧЕМУ так, а не ЧТО делает строка; смотри на существующие файлы —
numeric/convert.rs и kernels/diffuse.rs задают планку, и она высокая.
Плотность комментариев, именование и идиомы — как в соседнем коде.

ЧЕГО НЕ ДЕЛАТЬ. Не добавляй зависимости в Cargo.toml. Не угадывай величину, которой нет в
корпусе: если для работы нужно число, которого никто не решил, оставь TODO с объяснением, что
именно не решено и где это решается, — ровно как сделано в существующих TODO. Правдоподобная
выдумка неотличима от решения и переживёт тебя.
`

const CONTRACT_SCHEMA = {
  type: 'object',
  additionalProperties: false,
  required: ['files', 'signatures', 'tests', 'constraints', 'risks'],
  properties: {
    files: { type: 'array', items: { type: 'string' } },
    signatures: { type: 'array', items: { type: 'string' }, description: 'Публичные сигнатуры, которые появятся' },
    tests: {
      type: 'array',
      items: {
        type: 'object',
        additionalProperties: false,
        required: ['name', 'asserts'],
        properties: {
          name: { type: 'string' },
          file: { type: 'string' },
          asserts: { type: 'string', description: 'Что именно утверждает тест и как он падает, если свойство нарушено' },
        },
      },
    },
    constraints: { type: 'array', items: { type: 'string' }, description: 'Ограничения из ADR, дословно применимые к этому коду' },
    risks: { type: 'array', items: { type: 'string' }, description: 'Места, где ошибка будет молчаливой' },
    unresolved: { type: 'array', items: { type: 'string' }, description: 'Величины и решения, которых в корпусе нет' },
  },
}

const BUILD_SCHEMA = {
  type: 'object',
  additionalProperties: false,
  required: ['done', 'gate_output_tail', 'files_written', 'notes'],
  properties: {
    done: { type: 'boolean', description: 'Гейт прошёл целиком' },
    gate_output_tail: { type: 'string', description: 'Последние строки вывода гейта, дословно' },
    files_written: { type: 'array', items: { type: 'string' } },
    tests_added: { type: 'array', items: { type: 'string' } },
    notes: { type: 'string', description: 'Что пришлось сделать иначе, чем в контракте, и почему' },
    left_undone: { type: 'array', items: { type: 'string' } },
  },
}

const VERIFY_SCHEMA = {
  type: 'object',
  additionalProperties: false,
  required: ['gate_passes', 'findings'],
  properties: {
    gate_passes: { type: 'boolean' },
    gate_output_tail: { type: 'string' },
    findings: {
      type: 'array',
      items: {
        type: 'object',
        additionalProperties: false,
        required: ['severity', 'file', 'what', 'why_it_matters'],
        properties: {
          severity: { type: 'string', enum: ['blocking', 'should-fix', 'note'] },
          file: { type: 'string' },
          line: { type: 'integer' },
          what: { type: 'string' },
          why_it_matters: { type: 'string' },
          adr: { type: 'string' },
        },
      },
    },
  },
}

const results = []

for (let w = 0; w < WAVES.length; w++) {
  const wave = WAVES[w]
  log(`волна ${w + 1} из ${WAVES.length}: ${wave.map((i) => i.id).join(', ')}`)

  const done = await parallel(
    wave.map((item) => async () => {
      const adrList = (item.adrs || []).join(', ') || 'не названы'
      const head = `ЗАДАНИЕ: ${item.title}\n\nФАЙЛЫ, которых оно касается: ${(item.files || []).join(', ') || 'определи сам'}\nИМЕНА ТЕСТОВ из docs/ACCEPTANCE.md, которые обязаны появиться: ${(item.tests || []).join(', ') || 'нет обязательных'}\nЗАПИСИ ЖУРНАЛА, которые управляют этим кодом: ${adrList}\n${item.notes ? `\nЗАМЕТКИ: ${item.notes}` : ''}`

      // 1. Контракт. Только чтение: агент не имеет права ничего изменить, и
      //    именно поэтому он видит противоречия, которых не увидит тот, кто
      //    уже начал писать.
      const contract = await agent(
        `${head}

Ты пишешь КОНТРАКТ на эту работу и ничего не реализуешь. Ни одной правки файлов.

${RULES}

Прочитай названные записи журнала и соседний код. Верни: какие файлы появятся или изменятся,
какие публичные сигнатуры возникнут, какие тесты будут написаны и что именно каждый утверждает,
какие ограничения ADR дословно применимы, и — отдельно — где ошибка окажется молчаливой.

Последнее важнее остального. В этом проекте почти каждая настоящая ошибка молчалива: floor вместо
округления половин от нуля, пересчёт наличия после каждой реакции, масштаб от самого обильного
участника вместо самого дефицитного. Назови такие места здесь.

Если для работы нужна величина, которой в корпусе нет, — верни её в unresolved, а не придумай.`,
        { label: `contract:${item.id}`, phase: 'Contract', schema: CONTRACT_SCHEMA, effort: 'high' },
      )

      // 2. Сборка. Тест сначала, красный подтверждается, потом код.
      let build = await agent(
        `${head}

КОНТРАКТ, согласованный до начала работы:
файлы: ${contract ? contract.files.join(', ') : '—'}
сигнатуры: ${contract ? contract.signatures.join('\n  ') : '—'}
тесты: ${contract ? contract.tests.map((t) => `${t.name} — ${t.asserts}`).join('\n  ') : '—'}
ограничения: ${contract ? contract.constraints.join('\n  ') : '—'}
молчаливые места: ${contract ? contract.risks.join('\n  ') : '—'}
${contract && contract.unresolved && contract.unresolved.length ? `нерешённое в корпусе: ${contract.unresolved.join('\n  ')}` : ''}

Реализуй это ТЕСТ-СНАЧАЛА, в таком порядке и без сокращений:

1. Напиши тесты. Запусти их. УБЕДИСЬ, ЧТО ОНИ ПАДАЮТ, и падают по той причине, по которой должны,
   а не потому что код не компилируется от опечатки. Тест, который никогда не был красным, ничего
   не проверяет — он проверяет, что ты умеешь писать assert.
2. Напиши код, пока тесты не станут зелёными.
3. Прогони гейт целиком:
   ${GATE}
4. Чини, пока гейт не пройдёт.

${RULES}

Верни дословный хвост вывода гейта — не пересказ. Если что-то осталось несделанным, скажи это в
left_undone честно; недоделка, названная вслух, стоит дёшево, а необъявленная — дороже всего.`,
        { label: `build:${item.id}`, phase: 'Build', schema: BUILD_SCHEMA, effort: 'high' },
      )

      // 3. Проверка. Другой агент, другой контекст, задача — не поверить.
      const isKernel = (item.files || []).some((f) => f.includes('/kernels/'))
      let verify = await agent(
        `Ты проверяешь чужую работу в репозитории Liminis. Тебе НЕ НАДО чинить — надо найти.

${head}

Автор утверждает, что гейт прошёл. Проверь это сам:
   ${GATE}

Потом прочитай дифф — git diff и git status — и найди то, что гейт пропускает.

${RULES}

ИЩИ ИМЕННО ЭТО:

1. Нарушение формы ядра: трейт, замыкание, аллокация, чтение из буфера записи, зависимость от
   порядка обхода, параметр, не свёрнутый на хосте.
2. Сломанную антисимметрию: floor, округление к чётному, разный порядок операндов на двух сторонах
   грани, ветвление, различающееся у соседей.
3. Округление там, где его быть не должно: переходов между M и Q ровно три, и в реакциях округляется
   только экстент.
4. Тест, который был бы зелёным и на неверном коде. Это самая частая находка и самая ценная:
   проверь, падает ли тест, если внести в код правдоподобную ошибку. Если не падает — это находка.
5. Комментарий, который объясняет ЧТО делает строка, а не ПОЧЕМУ она такая. Планка задана
   numeric/convert.rs и kernels/diffuse.rs.
6. Величину, которую автор выдумал вместо того, чтобы оставить TODO.
${isKernel ? '7. Отдельно: этот дифф трогает kernels/. Пройди его против ADR-003, ADR-005, ADR-015, ADR-022 и ADR-034 построчно, как это делает субагент numerics-reviewer.' : ''}

Каждая находка — с файлом, строкой и номером ADR, если он есть. blocking означает «это нельзя
принимать»: неверная физика, сломанное сохранение, тест-пустышка на несущем свойстве.`,
        { label: `verify:${item.id}`, phase: 'Verify', schema: VERIFY_SCHEMA, effort: 'high' },
      )

      // 4. Починка. Ограниченный цикл: два круга, потом честный отчёт.
      let round = 0
      while (
        round < 2 &&
        verify &&
        (!verify.gate_passes || verify.findings.some((f) => f.severity === 'blocking'))
      ) {
        round++
        const todo = verify.findings.filter((f) => f.severity !== 'note')
        log(`${item.id}: круг починки ${round}, ${todo.length} находок`)

        await agent(
          `${head}

Проверяющий нашёл в твоей работе следующее. Почини.

гейт проходит: ${verify.gate_passes}
${verify.gate_output_tail ? `хвост вывода гейта:\n${verify.gate_output_tail}` : ''}

находки:
${todo.map((f) => `- [${f.severity}] ${f.file}${f.line ? `:${f.line}` : ''}${f.adr ? ` (${f.adr})` : ''}\n  ${f.what}\n  почему важно: ${f.why_it_matters}`).join('\n')}

${RULES}

Находка проверяющего — довод, а не приказ. Если она неверна, НЕ правь код: объясни в ответе,
почему она неверна, и покажи, чем это подтверждается в репозитории. Правка ради зелёного отчёта
хуже находки.

После правок прогони гейт целиком:
   ${GATE}`,
          { label: `fix:${item.id}#${round}`, phase: 'Fix', schema: BUILD_SCHEMA, effort: 'high' },
        )

        verify = await agent(
          `Перепроверь работу после круга починки ${round}.

${head}

Прогони гейт:
   ${GATE}

Прочитай git diff. Проверь, что найденное раньше действительно исправлено, а не заглушено, и что
починка не сломала соседнее. Особенно: не ослаблен ли тест вместо того, чтобы починить код.

${RULES}`,
          { label: `reverify:${item.id}#${round}`, phase: 'Verify', schema: VERIFY_SCHEMA, effort: 'high' },
        )
      }

      return {
        id: item.id,
        title: item.title,
        contract,
        build,
        verify,
        rounds: round,
        clean: !!(verify && verify.gate_passes && !verify.findings.some((f) => f.severity === 'blocking')),
      }
    }),
  )

  const wave_results = done.filter(Boolean)
  results.push(...wave_results)

  // Гейт волны стоит в скрипте, а не в пересказе. Волна с блокирующей находкой
  // не пускает следующую: следующая строится поверх этой, и чинить придётся оба
  // слоя.
  const broken = wave_results.filter((r) => !r.clean)
  if (broken.length) {
    log(`волна ${w + 1} не чиста: ${broken.map((r) => r.id).join(', ')} — дальше не идём`)
    return { results, stopped_after_wave: w + 1, broken: broken.map((r) => r.id) }
  }
}

return { results, stopped_after_wave: null, broken: [] }
