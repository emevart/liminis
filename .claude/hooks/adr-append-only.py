#!/usr/bin/env python3
"""PreToolUse: docs/DECISIONS.md можно только дописывать в конец.

Шапка самого журнала: «Отмена решения оформляется новой записью со ссылкой на
отменяемую, а не правкой старой». Журнал, который можно переписать задним
числом, — не журнал: через год будет невозможно установить, что решали тогда, а
что дописали потом, чтобы прошлое выглядело последовательным.

Что пропускается:

  Write  — если старое содержимое файла является строгим префиксом нового
           (то есть запись только добавила текст в конец). Создание файла с
           нуля тоже проходит.
  Edit   — если это вставка в самый хвост: `old_string` встречается ровно один
           раз, файл им заканчивается, и `new_string` начинается с `old_string`.

Всё остальное блокируется, включая `replace_all` и NotebookEdit.

Ограничение области: как и frozen-docs, хук не видит записи через Bash.
"""

import json
import os
import sys

WRITING_TOOLS = ("Write", "Edit", "NotebookEdit")

GUARDED = "docs/DECISIONS.md"

MESSAGE = (
    "DECISIONS.md — журнал, а не документ: существующие записи не правятся. "
    "Дописывай новую запись в конец (/adr); отмена решения — новая запись со "
    "ссылкой на отменяемую"
)


def repo_relative_path(payload):
    tool_input = payload.get("tool_input") or {}
    target = tool_input.get("file_path") or tool_input.get("notebook_path")
    if not target:
        return None, None
    root = os.environ.get("CLAUDE_PROJECT_DIR") or payload.get("cwd") or os.getcwd()
    relative = os.path.relpath(os.path.realpath(target), os.path.realpath(root))
    return relative.replace(os.sep, "/"), target


def current_text(path):
    try:
        with open(path, encoding="utf-8") as handle:
            return handle.read()
    except FileNotFoundError:
        return None


def verdict(tool_name, tool_input, existing):
    """Возвращает причину отказа или None, если запись разрешена."""
    if existing is None:
        return None if tool_name == "Write" else "файла ещё нет"

    if tool_name == "Write":
        new_text = tool_input.get("content")
        if new_text is None:
            new_text = tool_input.get("file_text")
        if new_text is None:
            return "не вижу нового содержимого в tool_input"
        if not new_text.startswith(existing):
            return "перезапись меняет уже записанное, а не дописывает в конец"
        return None

    if tool_name == "Edit":
        if tool_input.get("replace_all"):
            return "replace_all по журналу запрещён"
        old = tool_input.get("old_string")
        new = tool_input.get("new_string")
        if old is None or new is None:
            return "не вижу old_string/new_string в tool_input"
        if existing.count(old) != 1:
            return f"old_string встречается в файле {existing.count(old)} раз, нужен ровно один"
        if not existing.endswith(old):
            return "якорь не в конце файла — это правка существующей записи"
        if not new.startswith(old):
            return "new_string не начинается с old_string — это правка, а не вставка в конец"
        return None

    return f"инструмент {tool_name} по журналу не применяется"


def main():
    try:
        payload = json.load(sys.stdin)
    except (json.JSONDecodeError, UnicodeDecodeError) as exc:
        print(f"adr-append-only: не разобрал вход хука: {exc}", file=sys.stderr)
        return 1

    tool_name = payload.get("tool_name")
    if tool_name not in WRITING_TOOLS:
        return 0

    relative, absolute = repo_relative_path(payload)
    if relative is None:
        print("adr-append-only: в tool_input нет пути", file=sys.stderr)
        return 1
    if relative != GUARDED:
        return 0

    reason = verdict(tool_name, payload.get("tool_input") or {}, current_text(absolute))
    if reason is not None:
        print(f"{GUARDED}: {reason}. {MESSAGE}", file=sys.stderr)
        return 2

    return 0


if __name__ == "__main__":
    sys.exit(main())
