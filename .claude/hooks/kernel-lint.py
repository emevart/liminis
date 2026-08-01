#!/usr/bin/env python3
"""PostToolUse: после правки .rs — grep по ядрам, затем cargo fmt и clippy.

Порядок важен. Сначала дешёвая текстовая проверка ядер, потом тулчейн: если в
ядре голый оператор над `Q`, незачем ждать clippy.

Проверок в грепе две. Вторая — адресация по индексу вещества вместо полосы
(ADR-056, `.claude/rules/kernels.md`): `s * n_voxels + idx` компилируется,
выглядит как в скелете `ARCHITECTURE.md` и читает чужое количество, а ledger при
этом сходится — потеряно ничего, прочитано не то. Живёт она здесь, а не в
`scripts/check_bare_q.py`, потому что тот файл называет ровно одно правило и
одноимённая работа CI обещает ровно его. Цена названа вслух: **CI этой половины
не гоняет**, PR стороннего контрибьютора её не видит, и единственная защита там —
`mixed_width_storage_matches_uniform_width_storage`. Закрыть разрыв значит завести
общий модуль проверок ядра, а это решение о том, где живут проверки.

PostToolUse не блокирует — файл уже записан. Ненулевой код выхода здесь означает
«показать stderr модели», и для линтера это ровно нужное поведение.

Сама grep-проверка живёт в `scripts/check_bare_q.py`, и оттуда же её запускает
CI: хук работает только внутри сессии Claude Code, а PR стороннего
контрибьютора проверяется в CI или нигде. Две копии одного правила разошлись бы
молча, поэтому здесь импорт, а не повторение. Общая часть возвращает находку
кодом; формулирует её вызывающий — здесь по-русски, в CI по-английски.

Область осталась прежней: хук смотрит только `.rs`, потому что следом идут
`cargo fmt` и `clippy`. Шаблоны WGSL проверяет CI тем же модулем.
"""

import json
import os
import re
import shutil
import subprocess
import sys

TOOLS = ("Write", "Edit")
KERNELS = "crates/liminis-core/src/kernels/"

# Умножение идентификатора на число вокселей. Законный адрес умножает на него
# элемент таблицы полос — `rx.lane[s as usize] * p.n_voxels + idx`, — а там
# слева от звёздочки скобка, а не имя. Отсюда и форма выражения: ловится ровно
# `имя * n_voxels`.
SUBSTANCE_MAJOR = re.compile(r"\b([A-Za-z_]\w*)\s*\*\s*(?:[A-Za-z_]\w*\s*\.\s*)?n_voxels\b")

# Кроме имени полосы: `let lane = rx.lane[s]` с последующим `lane * p.n_voxels`
# — законная запись, и ADR-056 приводит адрес именно через полосу.
LANE_NAMES = ("lane", "lane_of")

# Разделяемая проверка лежит в scripts/ репозитория, рядом с test-hooks.sh.
SHARED = os.path.join(
    os.path.dirname(os.path.dirname(os.path.dirname(os.path.realpath(__file__)))),
    "scripts",
)

REASON = {
    "unwrap": "распаковка Q через .0",
    "operator": "голый арифметический оператор над Q",
}


def substance_major_addressing(text):
    """Вернуть [(номер строки, текст)] для адресации по индексу вещества."""
    shared = load_shared()
    findings = []
    for number, line in enumerate(text.splitlines(), start=1):
        # Комментарии и строковые литералы вычищает та же функция, что и у
        # проверки над Q: правило про код, а не про то, как о нём пишут.
        for name in SUBSTANCE_MAJOR.findall(shared.scrub(line)):
            if name not in LANE_NAMES:
                findings.append((number, line.strip()))
                break
    return findings


def load_shared():
    """Модуль общей проверки. Ошибка импорта — не повод молча пропустить файл."""
    # Иначе импорт оставляет в дереве scripts/__pycache__/, который никто не
    # просил и который .gitignore не знает.
    sys.dont_write_bytecode = True
    if SHARED not in sys.path:
        sys.path.insert(0, SHARED)
    import check_bare_q

    return check_bare_q


def repo_relative_path(payload):
    tool_input = payload.get("tool_input") or {}
    target = tool_input.get("file_path")
    if not target:
        return None, None
    root = os.environ.get("CLAUDE_PROJECT_DIR") or payload.get("cwd") or os.getcwd()
    root = os.path.realpath(root)
    relative = os.path.relpath(os.path.realpath(target), root)
    return relative.replace(os.sep, "/"), root


def cargo_binary():
    found = shutil.which("cargo")
    if found:
        return found
    fallback = os.path.expanduser("~/.cargo/bin/cargo")
    return fallback if os.path.exists(fallback) else None


def run(cargo, args, root):
    return subprocess.run(
        [cargo, *args],
        cwd=root,
        capture_output=True,
        text=True,
        timeout=600,
    )


def main():
    try:
        payload = json.load(sys.stdin)
    except (json.JSONDecodeError, UnicodeDecodeError) as exc:
        print(f"kernel-lint: не разобрал вход хука: {exc}", file=sys.stderr)
        return 1

    if payload.get("tool_name") not in TOOLS:
        return 0

    relative, root = repo_relative_path(payload)
    if relative is None or not relative.endswith(".rs"):
        return 0

    absolute = os.path.join(root, relative)

    if relative.startswith(KERNELS) and os.path.exists(absolute):
        try:
            shared = load_shared()
        except ImportError as exc:
            print(
                f"kernel-lint: не импортировал scripts/check_bare_q.py ({exc}) — "
                "проверка на голую арифметику над Q не выполнялась",
                file=sys.stderr,
            )
            return 1

        with open(absolute, encoding="utf-8") as handle:
            source = handle.read()

        findings = shared.bare_q_operators(source)
        if findings:
            print(
                f"{relative}: голая арифметика над Q запрещена внутри ядер "
                f"(ADR-022, NUMERIC.md §2). Только обёртки qmul/qdiv/qadd/qsub/qexp/…",
                file=sys.stderr,
            )
            for number, text, code in findings:
                what = REASON.get(code, code)
                print(f"  {relative}:{number}: {what}\n      {text}", file=sys.stderr)
            return 2

        lanes = substance_major_addressing(source)
        if lanes:
            print(
                f"{relative}: адрес считается через таблицу полос, а не по индексу "
                f"вещества (ADR-056). `rx.lane[s as usize] * p.n_voxels + idx`; "
                f"`s * n_voxels + idx` читает чужое количество, и ledger при этом "
                f"сходится — потеряно ничего, прочитано не то",
                file=sys.stderr,
            )
            for number, text in lanes:
                print(
                    f"  {relative}:{number}: адресация по индексу вещества\n"
                    f"      {text}",
                    file=sys.stderr,
                )
            return 2

    cargo = cargo_binary()
    if cargo is None:
        print(
            "kernel-lint: cargo не найден ни в PATH, ни в ~/.cargo/bin — "
            "fmt и clippy пропущены, проверка неполна",
            file=sys.stderr,
        )
        return 1

    formatted = run(cargo, ["fmt", "--all"], root)
    if formatted.returncode != 0:
        print(f"kernel-lint: cargo fmt упал:\n{formatted.stderr.strip()}", file=sys.stderr)
        return 2

    clippy = run(
        cargo,
        ["clippy", "--workspace", "--all-targets", "--", "-D", "warnings"],
        root,
    )
    if clippy.returncode != 0:
        tail = "\n".join(clippy.stderr.strip().splitlines()[-40:])
        print(f"kernel-lint: clippy недоволен:\n{tail}", file=sys.stderr)
        return 2

    return 0


if __name__ == "__main__":
    sys.exit(main())
