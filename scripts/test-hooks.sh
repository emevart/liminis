#!/usr/bin/env bash
#
# Тесты хуков .claude/hooks/.
#
# Хук, который молча пропускает то, что должен блокировать, хуже отсутствующего:
# он создаёт ложное чувство защиты. Поэтому на каждый хук здесь по две проверки —
# что он действительно блокирует и что он не блокирует лишнего.
#
# Блокировка засчитывается только при коде выхода 2 и непустом stderr: без
# сообщения запрет неотличим от падения скрипта.
#
# Совместимо с bash 3.2 (системный на macOS). Из внешнего нужен только Python 3 —
# тот же, на котором написаны сами хуки.

set -u

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
HOOKS="$ROOT/.claude/hooks"

# На Windows установщик с python.org кладёт python.exe, а python3 есть не всегда.
# Ищем оба и падаем громко: молчаливый пропуск здесь означал бы «тесты защиты
# прошли», хотя не запускался ни один.
PYTHON="$(command -v python3 || command -v python || true)"
if [ -z "$PYTHON" ]; then
    echo "нужен Python 3 в PATH (python3 или python) — на нём написаны хуки" >&2
    exit 1
fi

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

PASSED=0
FAILED=0

# check <метка> <ожидаемый код> <скрипт хука> <payload>
check() {
    label="$1"
    expected="$2"
    hook="$3"
    payload="$4"

    printf '%s' "$payload" | "$HOOKS/$hook" >"$WORK/stdout" 2>"$WORK/stderr"
    status=$?

    if [ "$status" -ne "$expected" ]; then
        printf 'FAIL  %-52s код выхода %s, ожидался %s\n' "$label" "$status" "$expected"
        sed 's/^/          /' "$WORK/stderr"
        FAILED=$((FAILED + 1))
        return
    fi

    if [ "$expected" -eq 2 ] && [ ! -s "$WORK/stderr" ]; then
        printf 'FAIL  %-52s заблокировал молча, без причины в stderr\n' "$label"
        FAILED=$((FAILED + 1))
        return
    fi

    if [ "$expected" -eq 0 ] && [ -s "$WORK/stderr" ]; then
        printf 'FAIL  %-52s пропустил, но что-то написал в stderr\n' "$label"
        sed 's/^/          /' "$WORK/stderr"
        FAILED=$((FAILED + 1))
        return
    fi

    printf 'ok    %s\n' "$label"
    PASSED=$((PASSED + 1))
}

# payload <инструмент> <путь> [событие]
payload() {
    TOOL="$1" FILE="$2" EVENT="${3:-PreToolUse}" "$PYTHON" - <<'PY'
import json
import os

print(json.dumps({
    "hook_event_name": os.environ["EVENT"],
    "tool_name": os.environ["TOOL"],
    "cwd": os.path.dirname(os.environ["FILE"]),
    "tool_input": {"file_path": os.environ["FILE"]},
}))
PY
}

# --------------------------------------------------------------------------
# Общий поддельный корень репозитория. CLAUDE_PROJECT_DIR указывает сюда, так
# что хуки считают путь относительным к нему и не трогают настоящее дерево.
# --------------------------------------------------------------------------

FAKE="$WORK/repo"
mkdir -p "$FAKE/docs/archive" "$FAKE/tests/golden" "$FAKE/crates/liminis-core/src/kernels"
export CLAUDE_PROJECT_DIR="$FAKE"

cat >"$FAKE/docs/DECISIONS.md" <<'EOF'
# Журнал решений

## ADR-001. Первое

**Решение.** Одно.

## ADR-002. Второе

**Решение.** Другое.
EOF

echo '# frozen-docs'

# Триггер заморозки — первый файл в kernels/, а не дата и не обещание
# (ADR-032). Поэтому проверяются оба состояния: до реализации и после.

check 'пропускает docs/SPEC.md, пока kernels/ пуст' 0 frozen-docs.py \
    "$(payload Write "$FAKE/docs/SPEC.md")"
check 'пропускает docs/NORTH_STAR.md, пока kernels/ пуст' 0 frozen-docs.py \
    "$(payload Edit "$FAKE/docs/NORTH_STAR.md")"

# .gitkeep существует затем, чтобы пустой каталог попал в git. Считать его
# началом реализации значило бы захлопнуть заморозку до первой строки кода.
touch "$FAKE/crates/liminis-core/src/kernels/.gitkeep"
check 'скрытый файл в kernels/ началом не считается' 0 frozen-docs.py \
    "$(payload Write "$FAKE/docs/SPEC.md")"

# Первый настоящий файл ядра захлопывает заморозку — и обратной дороги нет.
echo 'pub fn advect() {}' >"$FAKE/crates/liminis-core/src/kernels/advect.rs"
check 'блокирует docs/SPEC.md после первого ядра' 2 frozen-docs.py \
    "$(payload Write "$FAKE/docs/SPEC.md")"
check 'блокирует docs/NORTH_STAR.md после первого ядра' 2 frozen-docs.py \
    "$(payload Edit "$FAKE/docs/NORTH_STAR.md")"
rm "$FAKE/crates/liminis-core/src/kernels/advect.rs" \
   "$FAKE/crates/liminis-core/src/kernels/.gitkeep"

# archive заморожен по другой причине и безусловно: до реализации тоже.
check 'блокирует docs/archive/ и до реализации' 2 frozen-docs.py \
    "$(payload Write "$FAKE/docs/archive/ecosim-spec.md")"
check 'пропускает docs/OPEN_QUESTIONS.md' 0 frozen-docs.py \
    "$(payload Write "$FAKE/docs/OPEN_QUESTIONS.md")"
check 'не вмешивается в чтение' 0 frozen-docs.py \
    "$(payload Read "$FAKE/docs/SPEC.md")"

echo
echo '# adr-append-only'

adr_payload() {
    MODE="$1" TARGET="$FAKE/docs/DECISIONS.md" "$PYTHON" - <<'PY'
import json
import os

mode = os.environ["MODE"]
target = os.environ["TARGET"]
with open(target, encoding="utf-8") as handle:
    existing = handle.read()

tail = "**Решение.** Другое.\n"
entry = "\n---\n\n## ADR-003. Третье\n\n**Решение.** Третье.\n"

cases = {
    "write-rewrite": ("Write", {"content": "переписанный журнал\n"}),
    "write-append": ("Write", {"content": existing + entry}),
    "edit-middle": ("Edit", {
        "old_string": "**Решение.** Одно.",
        "new_string": "**Решение.** Совсем не одно.",
    }),
    "edit-append": ("Edit", {"old_string": tail, "new_string": tail + entry}),
    "edit-replace-all": ("Edit", {
        "old_string": tail,
        "new_string": tail + entry,
        "replace_all": True,
    }),
}

tool, extra = cases[mode]
tool_input = {"file_path": target}
tool_input.update(extra)
print(json.dumps({
    "hook_event_name": "PreToolUse",
    "tool_name": tool,
    "tool_input": tool_input,
}))
PY
}

check 'блокирует перезапись журнала целиком' 2 adr-append-only.py \
    "$(adr_payload write-rewrite)"
check 'блокирует правку записи в середине' 2 adr-append-only.py \
    "$(adr_payload edit-middle)"
check 'блокирует replace_all' 2 adr-append-only.py \
    "$(adr_payload edit-replace-all)"
check 'пропускает дописывание через Write' 0 adr-append-only.py \
    "$(adr_payload write-append)"
check 'пропускает дописывание в хвост через Edit' 0 adr-append-only.py \
    "$(adr_payload edit-append)"
check 'не трогает другие файлы в docs/' 0 adr-append-only.py \
    "$(payload Write "$FAKE/docs/OPEN_QUESTIONS.md")"

echo
echo '# golden-guard'

unset LIMINIS_BLESS_GOLDEN
check 'блокирует запись в tests/golden/' 2 golden-guard.py \
    "$(payload Write "$FAKE/tests/golden/diffusion.txt")"
check 'блокирует Edit в tests/golden/' 2 golden-guard.py \
    "$(payload Edit "$FAKE/tests/golden/diffusion.txt")"
check 'пропускает обычный тест' 0 golden-guard.py \
    "$(payload Write "$FAKE/tests/regular.rs")"

export LIMINIS_BLESS_GOLDEN=1
check 'пропускает при LIMINIS_BLESS_GOLDEN=1' 0 golden-guard.py \
    "$(payload Write "$FAKE/tests/golden/diffusion.txt")"

# Разрешает ровно единица. Любое другое значение — не разрешение.
export LIMINIS_BLESS_GOLDEN=0
check 'блокирует при LIMINIS_BLESS_GOLDEN=0' 2 golden-guard.py \
    "$(payload Write "$FAKE/tests/golden/diffusion.txt")"
unset LIMINIS_BLESS_GOLDEN

echo
echo '# kernel-lint'

cat >"$FAKE/crates/liminis-core/src/kernels/bare.rs" <<'EOF'
pub fn advect(conc: Q, velocity: Q) -> Q {
    let flux: Q = conc * velocity;
    flux
}
EOF

cat >"$FAKE/crates/liminis-core/src/kernels/unwrapped.rs" <<'EOF'
pub fn rate(conc: Q) -> f32 {
    let raw: Q = qmul(conc, conc);
    raw.0
}
EOF

cat >"$FAKE/crates/liminis-core/src/kernels/clean.rs" <<'EOF'
// Голых операторов над Q здесь нет: индексная арифметика над usize допустима.
pub fn advect(conc: Q, velocity: Q, i: usize) -> Q {
    let neighbour = i + 1;
    let _ = neighbour;
    let flux: Q = qmul(conc, velocity);
    flux
}
EOF

check 'блокирует голый оператор над Q в ядре' 2 kernel-lint.py \
    "$(payload Write "$FAKE/crates/liminis-core/src/kernels/bare.rs" PostToolUse)"
check 'блокирует распаковку Q через .0' 2 kernel-lint.py \
    "$(payload Write "$FAKE/crates/liminis-core/src/kernels/unwrapped.rs" PostToolUse)"
check 'не трогает не-Rust файлы' 0 kernel-lint.py \
    "$(payload Write "$FAKE/configs/scenarios/hello.toml" PostToolUse)"

# Чистое ядро пропускает grep, дальше идут cargo fmt и clippy — поэтому этот
# случай проверяется на настоящем репозитории, а не на поддельном корне.
unset CLAUDE_PROJECT_DIR
export CLAUDE_PROJECT_DIR="$ROOT"
check 'пропускает чистый Rust и проходит cargo' 0 kernel-lint.py \
    "$(payload Write "$ROOT/crates/liminis-core/src/config.rs" PostToolUse)"

echo
printf 'пройдено %s, провалено %s\n' "$PASSED" "$FAILED"
[ "$FAILED" -eq 0 ]
