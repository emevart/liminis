#!/usr/bin/env python3
"""PreToolUse: docs/SPEC.md, docs/NORTH_STAR.md и docs/archive/** — только чтение.

Спека заморожена до первого работающего прогона (NORTH_STAR.md, «Что помнить,
когда трудно»). Проектировать приятнее, чем отлаживать, и на этом умирает
большинство таких проектов. Правило вынесено из документа в хук, потому что
инструкция в CLAUDE.md — контекст, а не запрет.

docs/archive/** заморожен по другой причине: это вытесненные черновики,
противоречащие актуальной спеке. Их не правят, на них не ссылаются.

Ограничение области: хук видит только Write, Edit и NotebookEdit. Запись через
Bash (`>>`, `sed -i`, `tee`) проходит мимо — сознательный компромисс в пользу
нулевого числа ложных срабатываний.
"""

import json
import os
import sys

WRITING_TOOLS = ("Write", "Edit", "NotebookEdit")

FROZEN_FILES = ("docs/SPEC.md", "docs/NORTH_STAR.md")
FROZEN_TREES = ("docs/archive/",)

MESSAGE = "спека заморожена до первого прогона; идея → docs/OPEN_QUESTIONS.md"


def repo_relative_path(payload):
    """Путь редактируемого файла относительно корня репозитория."""
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
        print(f"frozen-docs: не разобрал вход хука: {exc}", file=sys.stderr)
        return 1

    if payload.get("tool_name") not in WRITING_TOOLS:
        return 0

    path = repo_relative_path(payload)
    if path is None:
        print(
            "frozen-docs: в tool_input нет пути, проверить нечего",
            file=sys.stderr,
        )
        return 1

    frozen = path in FROZEN_FILES or any(path.startswith(t) for t in FROZEN_TREES)
    if frozen:
        print(f"{path}: {MESSAGE}", file=sys.stderr)
        return 2

    return 0


if __name__ == "__main__":
    sys.exit(main())
