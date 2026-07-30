#!/usr/bin/env python3
"""PreToolUse: под tests/golden/** ничего не пишется без LIMINIS_BLESS_GOLDEN=1.

Золотой тест — это записанный ответ, против которого проверяется физика
(ADR-018: новые процессы среды принимаются только с золотым тестом против
аналитического решения). Инструмент, который может переписать эталон, чтобы
тест позеленел, эталоном не является.

Обновление эталона — осознанное действие, а не побочный эффект правки кода:

    LIMINIS_BLESS_GOLDEN=1 <команда>

Ограничение области: как и остальные PreToolUse-хуки, видит только Write, Edit и
NotebookEdit.
"""

import json
import os
import sys

WRITING_TOOLS = ("Write", "Edit", "NotebookEdit")

GUARDED_TREE = "tests/golden/"
BLESS = "LIMINIS_BLESS_GOLDEN"

MESSAGE = (
    f"золотые эталоны не переписываются мимоходом. Если новый ответ действительно "
    f"верен, обнови его явно: {BLESS}=1"
)


def repo_relative_path(payload):
    tool_input = payload.get("tool_input") or {}
    target = tool_input.get("file_path") or tool_input.get("notebook_path")
    if not target:
        return None
    root = os.environ.get("CLAUDE_PROJECT_DIR") or payload.get("cwd") or os.getcwd()
    relative = os.path.relpath(os.path.realpath(target), os.path.realpath(root))
    return relative.replace(os.sep, "/")


def main():
    try:
        payload = json.load(sys.stdin)
    except (json.JSONDecodeError, UnicodeDecodeError) as exc:
        print(f"golden-guard: не разобрал вход хука: {exc}", file=sys.stderr)
        return 1

    if payload.get("tool_name") not in WRITING_TOOLS:
        return 0

    path = repo_relative_path(payload)
    if path is None:
        print("golden-guard: в tool_input нет пути", file=sys.stderr)
        return 1

    if not path.startswith(GUARDED_TREE):
        return 0

    if os.environ.get(BLESS) == "1":
        return 0

    print(f"{path}: {MESSAGE}", file=sys.stderr)
    return 2


if __name__ == "__main__":
    sys.exit(main())
