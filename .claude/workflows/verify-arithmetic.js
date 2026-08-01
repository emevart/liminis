export const meta = {
  name: 'verify-arithmetic',
  description: 'Recompute every number in a set of decision records from first principles, independently of whoever wrote them',
  whenToUse:
    'On any document whose argument rests on computed quantities — byte budgets, substep counts, traffic per tick, collision probabilities. A wrong number in a decision record is compensated by calibration for years and never found (ADR-055).',
  phases: [{ title: 'Recompute', detail: 'one arithmetic checker per record' }],
}

// args: { file: "<path to a markdown file holding the records>", records: ["ADR-056", ...] }
//
// Accepts the argument as an object or as a JSON string. The two look identical
// at the call site and differ only in what reaches the script, and the failure
// is a crash on the first property access — worth six lines to not debug twice.
const input = typeof args === 'string' ? JSON.parse(args) : args || {}

const FILE = input.file || ''
if (!FILE) throw new Error('verify-arithmetic needs args.file')

const RECORDS = input.records || []
if (!RECORDS.length) throw new Error('verify-arithmetic needs args.records: ["ADR-056", ...]')

const CHECK_SCHEMA = {
  type: 'object',
  additionalProperties: false,
  required: ['claims'],
  properties: {
    claims: {
      type: 'array',
      items: {
        type: 'object',
        additionalProperties: false,
        required: ['quoted', 'verdict', 'recomputed'],
        properties: {
          quoted: { type: 'string', description: 'Утверждение из записи, дословно' },
          verdict: { type: 'string', enum: ['correct', 'wrong', 'unverifiable', 'right-number-wrong-reason'] },
          recomputed: { type: 'string', description: 'Твой счёт: входные величины, действие, результат' },
          impact: { type: 'string', description: 'Что меняется в решении, если число неверно' },
        },
      },
    },
  },
}

phase('Recompute')

const checked = await parallel(
  RECORDS.map((id) => () =>
    agent(
      `Ты проверяешь АРИФМЕТИКУ записи ${id} в файле ${FILE} репозитория Liminis. Не формулировки, не стиль, не убедительность — только числа.

Прочитай запись ${id} в ${FILE}.

ВЫПИШИ КАЖДОЕ ЧИСЛО, которое запись утверждает как посчитанное, и пересчитай его сам, с нуля, взяв входные величины из корпуса, а не из самой записи. Числа этого класса:

- байты на воксель, мегабайты и гигабайты при 128³ и 256³;
- число подшагов диффузии n = ceil(6·D·dt/dx²) для конкретных веществ;
- масштабы kᵢ, экспоненты экстента e_r, коэффициенты хранения νᵢ;
- трафик памяти за тик, доли и проценты;
- вероятности, порядки величин, отношения;
- значения хеш-функций и любые «пересчитано, выходит вот столько».

ОТКУДА БРАТЬ ВХОДЫ:
docs/SPEC.md §1.2 (бюджет вокселя), §1.7 (коэффициенты диффузии), §2.3 (реестр веществ, молярные массы);
docs/QUANTITIES.md (единицы и типы);
docs/DECISIONS.md (ADR-039 формулы вывода, ADR-040 цена разрядности, ADR-045 накопитель энергии, ADR-042 свёрнутое условие);
crates/liminis-core/src/ — действующий код, если утверждение про него.

ОСОБО:

1. Если запись утверждает конкретные значения хеш-функции или генератора — НЕ верь им и не воспроизводи их «по модели». Либо посчитай их реально (напиши и запусти маленькую программу; в репозитории есть cargo и python3), либо верни verdict «unverifiable» и скажи, что число обязано быть получено запуском, а не выведено рассуждением. Число, выдуманное правдоподобно, здесь опаснее отсутствующего: его перенесут в тест.
2. Если число верное, но получено неверным рассуждением — это отдельный вердикт «right-number-wrong-reason», и он важен: следующий читатель повторит рассуждение на других входах.
3. Проверь единицы. Грамм против килограмма, джоуль против килоджоуля, м² против м²/с. Половина расхождений корпуса — это единицы (docs/CONFIG_SCHEMA.md §13).

Верни список: цитата, вердикт, твой счёт с показанными входами и действием, и что меняется в решении, если число неверно.

Не проверяй прозу. Если в записи нет посчитанных чисел, верни пустой список — это законный исход.`,
      { label: `arith:${id}`, phase: 'Recompute', schema: CHECK_SCHEMA, effort: 'high' },
    ),
  ),
)

const claims = checked
  .filter(Boolean)
  .flatMap((r, i) => r.claims.map((c) => ({ ...c, record: RECORDS[i] })))

const wrong = claims.filter((c) => c.verdict === 'wrong')
const shaky = claims.filter((c) => c.verdict === 'unverifiable' || c.verdict === 'right-number-wrong-reason')

log(`${claims.length} чисел проверено: ${wrong.length} неверных, ${shaky.length} сомнительных`)

return { wrong, shaky, all: claims, verdict: wrong.length ? 'has-errors' : 'arithmetic-holds' }
