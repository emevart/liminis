#!/usr/bin/env python3
"""Голая арифметика над `Q` в ядрах и в шаблонах WGSL (ADR-022, NUMERIC.md §2).

Одна проверка на двух вызывающих. `.claude/hooks/kernel-lint.py` импортирует
отсюда `bare_q_operators` и зовёт её на только что записанном файле; CI
запускает этот же файл командой и проходит им по всему дереву. Хук живёт внутри
сессии Claude Code — PR стороннего контрибьютора не видит его вовсе, а NUMERIC.md
§8 требует «линтер или grep в CI, а не строчку в CLAUDE.md». Копия проверки
рядом с оригиналом разошлась бы с ним молча, поэтому импорт, а не копия.

Первый рубеж против голой арифметики — сам тип: `Q` не реализует `Deref` и не
раскрывает внутреннее поле, поэтому `a * b` над двумя `Q` не компилируется
(NUMERIC.md §7). Здесь второй рубеж, для того, что тип пропускает: распаковка
через `.0` и арифметика над объявленными как `Q` значениями в шаблонах WGSL, где
системы типов нет вовсе.

Проверка сознательно неполна: `qmul(a, b) * 2.0` она не увидит, потому что
оператор стоит рядом со скобкой, а не с идентификатором. Полноту даёт тип, не grep.

Находка называется кодом, а не фразой. Сообщение хука читает модель по-русски,
сообщение CI — контрибьютор по-английски (D-5); формулировка принадлежит
вызывающему, общей остаётся только находка.

Имя через подчёркивания, в отличие от `test-hooks.sh`: этот файл импортируют, а
не только запускают.
"""

import os
import re
import sys

# Ядра проверяются целиком и без разбора расширений: там не лежит ничего, кроме
# того, что построчно уезжает в WGSL (ADR-015, ARCHITECTURE.md).
KERNELS = "crates/liminis-core/src/kernels"

# Шаблон шейдера проверяется, где бы он ни лежал: систем типов в нём нет, и grep
# в нём единственный рубеж, а не второй.
TEMPLATE_SUFFIXES = (".wgsl", ".wgsl.tmpl")

# Сборочный вывод. Служебные каталоги вроде .git отсеиваются как скрытые.
SKIP_DIRS = ("target",)

# Коды находок. Формулировку даёт вызывающий, см. заголовок модуля.
UNWRAP = "unwrap"
OPERATOR = "operator"

# Идентификаторы, объявленные с типом Q: `let flux: Q`, `fn f(rate: Q)`,
# `struct S { conc: Q }`, `var<private> c: Q` — все формы ловятся одним выражением.
Q_DECLARATION = re.compile(r"\b([A-Za-z_]\w*)\s*:\s*Q\b")

LINE_COMMENT = re.compile(r"//.*$")
STRING_LITERAL = re.compile(r"\"(?:\\.|[^\"\\])*\"|'(?:\\.|[^'\\])*'")
ARROW = re.compile(r"->")

REASON = {
    UNWRAP: "Q unwrapped through .0",
    OPERATOR: "bare arithmetic operator on Q",
}

HEADER = (
    "Bare arithmetic over Q is forbidden in kernels and WGSL templates "
    "(ADR-022, NUMERIC.md section 2).\n"
    "Go through the wrappers: qmul, qdiv, qadd, qsub, qexp, ..."
)


def scrub(line):
    """Убрать из строки то, что арифметикой не является."""
    return ARROW.sub("  ", STRING_LITERAL.sub('""', LINE_COMMENT.sub("", line)))


def bare_q_operators(text):
    """Вернуть [(номер строки, текст, код находки)] для голой арифметики над Q."""
    lines = text.splitlines()
    scrubbed = [scrub(line) for line in lines]

    names = set()
    for line in scrubbed:
        names.update(Q_DECLARATION.findall(line))
    if not names:
        return []

    alternation = "|".join(re.escape(name) for name in sorted(names))
    operator = re.compile(
        rf"(?:\b(?:{alternation})\b\s*[-+*/]|[-+*/]\s*\b(?:{alternation})\b)"
    )
    unwrap = re.compile(rf"\b(?:{alternation})\b\s*\.\s*0\b")

    findings = []
    for number, (raw, clean) in enumerate(zip(lines, scrubbed), start=1):
        if unwrap.search(clean):
            findings.append((number, raw.strip(), UNWRAP))
        elif operator.search(clean):
            findings.append((number, raw.strip(), OPERATOR))
    return findings


def is_target(relative):
    """Проверяется ли файл с таким путём относительно корня репозитория."""
    return relative.startswith(KERNELS + "/") or relative.endswith(TEMPLATE_SUFFIXES)


def walk(root):
    """Пути под `root`, которые надо проверить, относительно `root`."""
    targets = []
    for current, dirs, files in os.walk(root):
        dirs[:] = sorted(
            name
            for name in dirs
            if name not in SKIP_DIRS and not name.startswith(".")
        )
        for name in sorted(files):
            if name.startswith("."):
                continue
            absolute = os.path.join(current, name)
            relative = os.path.relpath(absolute, root).replace(os.sep, "/")
            if is_target(relative):
                targets.append(relative)
    return targets


def report(relative, findings, annotate):
    """Напечатать находки одного файла. Аннотации — чтобы CI показал их в диффе."""
    for number, text, code in findings:
        reason = REASON.get(code, code)
        print(f"  {relative}:{number}: {reason}")
        print(f"      {text}")
        if annotate:
            print(f"::error file={relative},line={number}::{reason} (ADR-022)")


def main(argv):
    root = os.path.dirname(os.path.dirname(os.path.realpath(__file__)))
    annotate = os.environ.get("GITHUB_ACTIONS") == "true"

    if argv:
        targets = []
        for given in argv:
            absolute = os.path.realpath(given)
            if not os.path.isfile(absolute):
                print(f"check_bare_q: not a file: {given}")
                return 2
            targets.append(absolute)
    else:
        targets = [os.path.join(root, *path.split("/")) for path in walk(root)]

    # Пустой каталог ядер — нормальное состояние проекта до первого ядра
    # (ADR-032), а не повод падать.
    if not targets:
        print(f"No kernel under {KERNELS}/ and no WGSL template yet — nothing to check.")
        return 0

    dirty = 0
    for absolute in targets:
        relative = os.path.relpath(absolute, root).replace(os.sep, "/")
        with open(absolute, encoding="utf-8", errors="replace") as handle:
            findings = bare_q_operators(handle.read())
        if not findings:
            continue
        if dirty == 0:
            print(HEADER)
        dirty += 1
        report(relative, findings, annotate)

    if dirty:
        print(f"\n{dirty} of {len(targets)} checked file(s) go around the wrappers.")
        return 1

    print(f"{len(targets)} file(s) checked, no bare arithmetic over Q.")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
