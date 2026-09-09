export const meta = {
  name: 'conformance-review',
  description: 'Review the working tree against the decision journal through fixed lenses, then try to refute every finding',
  whenToUse:
    'After a milestone, before accepting work. Each lens is a different way the code can be wrong; every finding faces three independent sceptics and dies on a majority. Returns a blocking verdict the caller must respect.',
  phases: [
    { title: 'Look', detail: 'one reviewer per lens over the diff' },
    { title: 'Refute', detail: 'three sceptics per finding, majority kills' },
    { title: 'Report', detail: 'what survived, ranked' },
  ],
}

// args: { range?: 'main...HEAD', scope?: 'crates/**', lenses?: [...] }
const RANGE = (args && args.range) || 'main...HEAD'
const SCOPE = (args && args.scope) || ''

const LENSES = (args && args.lenses) || [
  {
    key: 'conservation',
    adrs: 'ADR-003, ADR-005, ADR-028, ADR-034',
    hunt: `Сохранение. Инвариант тика — ТОЧНОЕ равенство целых, отдельно по веществу и отдельно по энергии.
Ищи: поток, не являющийся строго антисимметричным при любых входах; округление, не симметричное
относительно нуля; кламп, гасящий массу вместо того, чтобы её передать; величину, которая
появляется или исчезает мимо именованного канала; счётчик, который прибавляется дважды или ни разу;
энергию, зачисленную клетке или вокселю без вычета из источника.
Проверь особо: перезапись против прибавления в накопителе энергии (ADR-045) — второй вызов в том же
тике стирает первый, и ledger по веществу при этом сходится, а по энергии нет.`,
  },
  {
    key: 'kernel-shape',
    adrs: 'ADR-015, ADR-034, ADR-041, docs/agent-kernels.md',
    hunt: `Форма ядра. Всё под kernels/ обязано переводиться в WGSL построчно.
Ищи: self, трейты, дженерики по поведению, замыкания, Vec, Box, dyn, HashMap, рекурсию, любую
аллокацию, итераторные цепочки, не выражаемые циклом for по диапазону; чтение из буфера записи;
запись в чужую ячейку; зависимость результата от порядка обхода; параметр, не свёрнутый на хосте
(ядро, знающее D, dt и dx по отдельности, вместо одного alpha); ветвление по вокселю там, где на
GPU разошёлся бы варп — ветвиться разрешено по индексу вещества, он одинаков для всех вокселей.
Отдельно: локальные массивы обязаны быть фиксированного размера, отсюда S_MAX и R_MAX.`,
  },
  {
    key: 'numerics',
    adrs: 'ADR-022, ADR-027, docs/NUMERIC.md',
    hunt: `Числа. Ищи: голый арифметический оператор над Q; распаковку Q через .0 или через Deref;
библиотечный вызов exp, ln, pow, sqrt вместо обёртки; четвёртое место, где происходит округление,
помимо трёх названных переходов; округление отдельных веществ в реакции вместо округления одного
экстента; детерминированное округление там, где ADR-027 требует стохастического, и наоборот;
counter-based RNG, получивший нестабильный аргумент — идентификатор от позиции в файле, а не от
имени; f32 там, где заявлена величина класса M.`,
  },
  {
    key: 'scales',
    adrs: 'ADR-035, ADR-039, ADR-040, ADR-042',
    hunt: `Масштабы и разрядность. Ищи: масштаб или разрядность, назначенные вместо выведенных;
экспоненту экстента, взятую от самого обильного участника реакции вместо самого дефицитного;
свёрнутое условие представимости, записанное как 2^28/beta вместо 2^28·beta — это ошибка, ради
которой написан целый ADR-042; проверку по свёрнутой форме вместо первичных неравенств;
отсутствие подъёма k_i до e_r; коэффициент хранения, не проверенный на переполнение i32;
динамический диапазон вещества шире 2^14, прошедший загрузку; сообщение об отказе, называющее одно
имя там, где виновата пара «вещество, реакция».`,
  },
  {
    key: 'splitting',
    adrs: 'ADR-036, ADR-030, ADR-045, ADR-049, ADR-050',
    hunt: `Порядок и расщепление. Порядок процессов есть семантика мира.
Ищи: перестановку шагов относительно SPEC §8 без инкремента WORLD_FORMAT_VERSION; every_n_ticks
больше единицы у процесса, трогающего диффузионное поле; число подшагов, взятое из таблицы вместо
формулы; alpha, посчитанную отдельно от числа подшагов, так что они могут разойтись; разбиение
химии на несколько шагов тика — оно отменяет общий коэффициент конкуренции и стирает приращение
энергии; свёртку энергии, вызванную до света, отчего поле света принадлежит прошлому тику.`,
  },
  {
    key: 'kinetics',
    adrs: 'ADR-025, ADR-026, ADR-027, ADR-043, ADR-044, ADR-047, ADR-048',
    hunt: `Химия. Ищи: катализатор, входящий количеством на воксель вместо концентрации — это вносит dx
в скорость всякой реакции; km, сравниваемую с количеством вместо концентрации; T_ref, использованную
как опорная температура множителя Q10 вместо t_vmax; допуск массового баланса, заданный числом
вместо вывода; проверку, пропускающую реакцию, создающую вещество без отслеживаемых элементов;
произведение вместо минимума по субстратам в законе Либиха; пересчёт наличия после каждой реакции
вместо одного снимка; xi_max, посчитанную не только по входам.`,
  },
  {
    key: 'drift',
    adrs: 'ADR-020, ADR-032, ADR-034, ADR-038',
    hunt: `Дрейф документов и идентичности. SPEC.md и NORTH_STAR.md заморожены — их правка сама по себе
находка. Ищи: правку configs/**, kernels/** или process/** без инкремента WORLD_FORMAT_VERSION;
утверждение в docs/ARCHITECTURE.md, docs/QUANTITIES.md, docs/CONFIG_SCHEMA.md или
docs/ACCEPTANCE.md, которому код больше не соответствует; имя теста в коде, разошедшееся с
docs/ACCEPTANCE.md; ключ конфига, попавший в хеш, хотя симулятор его не читает, или наоборот;
величину, у которой в коде появилась единица, отличная от docs/QUANTITIES.md.`,
  },
]

const FINDINGS_SCHEMA = {
  type: 'object',
  additionalProperties: false,
  required: ['findings'],
  properties: {
    findings: {
      type: 'array',
      items: {
        type: 'object',
        additionalProperties: false,
        required: ['file', 'claim', 'failure_scenario', 'severity'],
        properties: {
          file: { type: 'string' },
          line: { type: 'integer' },
          claim: { type: 'string', description: 'Одно предложение: в чём дефект' },
          failure_scenario: { type: 'string', description: 'Конкретные входы или состояние → неверный результат' },
          severity: { type: 'string', enum: ['blocking', 'should-fix', 'note'] },
          adr: { type: 'string' },
        },
      },
    },
  },
}

const VERDICT_SCHEMA = {
  type: 'object',
  additionalProperties: false,
  required: ['refuted', 'reason'],
  properties: {
    refuted: { type: 'boolean', description: 'true, если находка неверна или несущественна' },
    reason: { type: 'string' },
    correction: { type: 'string', description: 'Если находка верна, но описана неточно — как правильно' },
  },
}

phase('Look')

const reviewed = await pipeline(
  LENSES,

  (lens) =>
    agent(
      `Ты ревьюишь дифф репозитория Liminis через одну линзу и только через неё.

ЧТО СМОТРЕТЬ: git diff ${RANGE}${SCOPE ? ` -- ${SCOPE}` : ''}
Читай и сами файлы целиком, а не только дифф: контекст решает.

ЛИНЗА «${lens.key}». Управляющие записи: ${lens.adrs}.

${lens.hunt}

КАК РАБОТАТЬ. Открой названные записи журнала (grep по «## ADR-0NN» в docs/DECISIONS.md) и читай
код против них, а не против общего представления о хорошем коде. Замечание вида «стоило бы
отрефакторить» здесь бесполезно и вредно: оно разбавляет список, в котором каждая строка должна
стоить чтения.

Находка обязана иметь сценарий отказа: конкретные входы или состояние, при которых результат
неверен. Если сценария нет, находки нет — есть подозрение, и его не надо возвращать.

blocking означает: это нельзя принимать. Неверная физика, сломанное сохранение, молчаливая потеря
вещества или энергии, тест-пустышка на несущем свойстве.`,
      { label: `look:${lens.key}`, phase: 'Look', schema: FINDINGS_SCHEMA, effort: 'high' },
    ),

  // Каждая находка немедленно уходит на опровержение — барьера между линзами нет.
  (result, lens) =>
    parallel(
      (result ? result.findings : []).map((f) => () =>
        parallel(
          ['точность: воспроизводится ли сценарий отказа на самом коде', 'существенность: меняет ли это результат прогона или только вкус ревьюера', 'первоисточник: говорит ли названная запись журнала то, что ей приписали'].map(
            (angle) => () =>
              agent(
                `Ты опровергаешь чужую находку код-ревью в репозитории Liminis. Твой угол: ${angle}.

НАХОДКА (линза «${lens.key}»):
файл: ${f.file}${f.line ? `:${f.line}` : ''}
утверждение: ${f.claim}
сценарий отказа: ${f.failure_scenario}
серьёзность по мнению нашедшего: ${f.severity}
запись журнала: ${f.adr || 'не названа'}

Открой файл. Проверь сценарий на самом коде — не на пересказе. Если названа запись журнала, открой
её и проверь, говорит ли она то, что ей приписали.

При сомнении отвечай refuted = true. Ложная находка обходится дороже пропущенной: она тратит время
на опровержение и приучает не верить списку. Опровергай, если верно хоть что-то из:
сценарий не воспроизводится; поведение намеренное и объяснено комментарием или записью журнала;
это вопрос вкуса, а не корректности; названная запись журнала говорит другое.`,
                { label: `refute:${lens.key}:${f.file.split('/').pop()}`, phase: 'Refute', schema: VERDICT_SCHEMA },
              ),
          ),
        ).then((votes) => {
          const alive = votes.filter(Boolean)
          const kept = alive.filter((v) => !v.refuted).length
          return { ...f, lens: lens.key, survived: kept >= 2, votes: alive }
        }),
      ),
    ),
)

const all = reviewed.flat().filter(Boolean)
const survived = all.filter((f) => f.survived)
const blocking = survived.filter((f) => f.severity === 'blocking')

log(`${all.length} находок, выжило ${survived.length}, блокирующих ${blocking.length}`)

phase('Report')

const rank = { blocking: 0, 'should-fix': 1, note: 2 }
survived.sort((a, b) => rank[a.severity] - rank[b.severity])

// Вердикт возвращается вызывающему явно. Блокирующая находка обязана остановить
// следующую фазу в скрипте, а не в пересказе результата.
return {
  verdict: blocking.length ? 'blocked' : 'clear',
  blocking,
  findings: survived,
  refuted_count: all.length - survived.length,
}
