#!/usr/bin/env python3
"""PostToolUse: после правки .rs — grep по ядрам, затем cargo fmt и clippy.

Порядок важен. Сначала дешёвая текстовая проверка ядер, потом тулчейн: если в
ядре голый оператор над `Q`, незачем ждать clippy.

PostToolUse не блокирует — файл уже записан. Ненулевой код выхода здесь означает
«показать stderr модели», и для линтера это ровно нужное поведение.

О grep-проверке. Первый рубеж против голой арифметики над `Q` — сам тип: `Q` не
реализует `Deref` и не раскрывает внутреннее поле, поэтому `a * b` над двумя `Q`
не компилируется (NUMERIC.md §7-8). Эта проверка — второй рубеж, для случаев,
которые тип пропускает: распаковка через `.0` и арифметика над значениями,
объявленными как `Q`, в шаблонах и в коде, который до компилятора ещё не дошёл.
Она сознательно неполна: `qmul(a, b) * 2.0` она не увидит, потому что оператор
стоит рядом со скобкой, а не с идентификатором. Полноту даёт тип, не grep.
"""

import json
import os
import re
import shutil
import subprocess
import sys

TOOLS = ("Write", "Edit")
KERNELS = "crates/liminis-core/src/kernels/"

# Идентификаторы, объявленные с типом Q: `let flux: Q`, `fn f(rate: Q)`,
# `struct S { conc: Q }` — все три формы ловятся одним выражением.
Q_DECLARATION = re.compile(r"\b([A-Za-z_]\w*)\s*:\s*Q\b")

LINE_COMMENT = re.compile(r"//.*$")
STRING_LITERAL = re.compile(r"\"(?:\\.|[^\"\\])*\"|'(?:\\.|[^'\\])*'")
ARROW = re.compile(r"->")


def repo_relative_path(payload):
    tool_input = payload.get("tool_input") or {}
    target = tool_input.get("file_path")
    if not target:
        return None, None
    root = os.environ.get("CLAUDE_PROJECT_DIR") or payload.get("cwd") or os.getcwd()
    root = os.path.realpath(root)
    relative = os.path.relpath(os.path.realpath(target), root)
    return relative.replace(os.sep, "/"), root


def scrub(line):
    """Убрать из строки то, что арифметикой не является."""
    return ARROW.sub("  ", STRING_LITERAL.sub('""', LINE_COMMENT.sub("", line)))


def bare_q_operators(text):
    """Вернуть [(номер строки, текст, что не так)] для голой арифметики над Q."""
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
            findings.append((number, raw.strip(), "распаковка Q через .0"))
        elif operator.search(clean):
            findings.append((number, raw.strip(), "голый арифметический оператор над Q"))
    return findings


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
        with open(absolute, encoding="utf-8") as handle:
            findings = bare_q_operators(handle.read())
        if findings:
            print(
                f"{relative}: голая арифметика над Q запрещена внутри ядер "
                f"(ADR-022, NUMERIC.md §2). Только обёртки qmul/qdiv/qadd/qsub/qexp/…",
                file=sys.stderr,
            )
            for number, text, what in findings:
                print(f"  {relative}:{number}: {what}\n      {text}", file=sys.stderr)
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
