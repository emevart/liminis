#!/usr/bin/env python3
"""PreToolUse: docs/archive/** — только чтение всегда; docs/SPEC.md и
docs/NORTH_STAR.md — с момента, когда началась реализация ядер.

Заморозка наступает не по календарю и не по обещанию, а по состоянию
репозитория: как только в crates/liminis-core/src/kernels/ появляется первый
файл, реализация началась и спека перестаёт редактироваться (ADR-032). До
этого момента она приводится в соответствие с журналом решений — но не
дополняется: новая идея по-прежнему идёт в docs/OPEN_QUESTIONS.md.

Мотив самой заморозки прежний (NORTH_STAR.md, «Что помнить, когда трудно»):
проектировать приятнее, чем отлаживать, и на этом умирает большинство таких
проектов. Сменился момент. Прежним триггером был первый работающий прогон, и до
него документ успевал разойтись с принятыми решениями — то есть врал ровно
тогда, когда его читают как источник истины.

docs/archive/** заморожен по другой причине и безусловно: это вытесненные
черновики, противоречащие актуальной спеке. Их не правят, на них не ссылаются,
и никакая стадия проекта этого не меняет.

Скрытые файлы в kernels/ началом реализации не считаются: .gitkeep существует
ровно затем, чтобы пустой каталог попал в git.

Ограничение области: хук видит только Write, Edit и NotebookEdit. Запись через
Bash (`>>`, `sed -i`, `tee`) проходит мимо — сознательный компромисс в пользу
нулевого числа ложных срабатываний.
"""

import json
import os
import sys

WRITING_TOOLS = ("Write", "Edit", "NotebookEdit")

# Правятся, пока не началась реализация ядер (ADR-032).
THAWED_UNTIL_KERNELS = ("docs/SPEC.md", "docs/NORTH_STAR.md")

# Заморожено безусловно, на любой стадии.
FROZEN_TREES = ("docs/archive/",)

KERNELS_DIR = "crates/liminis-core/src/kernels"

FROZEN_MESSAGE = (
    "реализация ядер началась, спека заморожена (ADR-032); "
    "идея → docs/OPEN_QUESTIONS.md"
)
ARCHIVE_MESSAGE = (
    "вытесненный черновик, противоречит актуальной спеке; не правится и не "
    "цитируется"
)


def repo_root(payload):
    """Корень репозитория, относительно которого считаются пути."""
    root = os.environ.get("CLAUDE_PROJECT_DIR") or payload.get("cwd") or os.getcwd()
    return os.path.realpath(root)


def repo_relative_path(payload, root):
    """Путь редактируемого файла относительно корня репозитория."""
    tool_input = payload.get("tool_input") or {}
    target = tool_input.get("file_path") or tool_input.get("notebook_path")
    if not target:
        return None
    relative = os.path.relpath(os.path.realpath(target), root)
    return relative.replace(os.sep, "/")


def implementation_started(root):
    """Есть ли в kernels/ хоть один нескрытый файл."""
    kernels = os.path.join(root, *KERNELS_DIR.split("/"))
    for _, dirs, files in os.walk(kernels):
        dirs[:] = [name for name in dirs if not name.startswith(".")]
        if any(not name.startswith(".") for name in files):
            return True
    return False


def main():
    try:
        payload = json.load(sys.stdin)
    except (json.JSONDecodeError, UnicodeDecodeError) as exc:
        print(f"frozen-docs: не разобрал вход хука: {exc}", file=sys.stderr)
        return 1

    if payload.get("tool_name") not in WRITING_TOOLS:
        return 0

    root = repo_root(payload)
    path = repo_relative_path(payload, root)
    if path is None:
        print(
            "frozen-docs: в tool_input нет пути, проверить нечего",
            file=sys.stderr,
        )
        return 1

    if any(path.startswith(tree) for tree in FROZEN_TREES):
        print(f"{path}: {ARCHIVE_MESSAGE}", file=sys.stderr)
        return 2

    if path in THAWED_UNTIL_KERNELS and implementation_started(root):
        print(f"{path}: {FROZEN_MESSAGE}", file=sys.stderr)
        return 2

    return 0


if __name__ == "__main__":
    sys.exit(main())
