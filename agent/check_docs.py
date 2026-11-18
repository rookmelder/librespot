"""Check markdown notes folders for broken links, missing files, orphans, and formatting: python check_docs.py [folders] [--repo REPO]
Search them, one line per note: python check_docs.py --find shop price """
from __future__ import annotations

import argparse
import os
from pathlib import Path
import re
import sys
import urllib.parse

REPO = Path(__file__).resolve().parents[1]
DEFAULT_FOLDERS = ['knowledge', 'agent/notes']
MAX_NOTE_LINES = 60
MAX_INDEX_LINES = 80

_LINK_RE = re.compile(r'\[(?:[^\]]*)\]\((?:<([^>]+)>|([^)\s]+))\)')
_FILES_BULLET_RE = re.compile(r'^- `([^`]+)`')


def _is_index_note(path: Path, root: Path) -> bool:
    if path.name == 'README.md':
        return True
    stem = path.stem
    parent_name = path.parent.name
    return stem == parent_name


def check_folder(root: Path, repo: Path) -> list[tuple[str, str, int, str]]:
    problems: list[tuple[str, str, int, str]] = []

    # 1. Notes = every *.md under root, recursive, skipping any folder whose name starts with .
    notes: list[Path] = []
    for dirpath, dirnames, filenames in os.walk(root):
        dirnames[:] = [d for d in dirnames if not d.startswith('.')]
        for f in filenames:
            if f.endswith('.md'):
                notes.append(Path(dirpath) / f)

    root_readme = root / 'README.md'
    readme_missing = not root_readme.is_file()

    # 2. Index notes: root/README.md, and any note whose file name (without .md) equals its parent folder's name
    if readme_missing:
        rel_readme = root_readme.resolve().relative_to(repo.resolve()).as_posix() if root_readme.is_relative_to(repo) else root_readme.as_posix()
        problems.append(('ERROR', rel_readme, 0, 'missing index README.md'))

    # Helper for resolving relative paths for reporting
    def get_relpath(p: Path) -> str:
        try:
            return p.resolve().relative_to(repo.resolve()).as_posix()
        except ValueError:
            return p.as_posix()

    note_links: dict[Path, list[Path]] = {n: [] for n in notes}

    # 3. For each note
    for note in notes:
        relpath = get_relpath(note)
        is_index = _is_index_note(note, root)

        try:
            content = note.read_text(encoding='utf-8')
        except Exception:
            continue

        lines = content.splitlines()

        # 3e. Size
        line_count = len(lines)
        limit = MAX_INDEX_LINES if is_index else MAX_NOTE_LINES
        if line_count > limit:
            problems.append(('WARN', relpath, 0, f'long note ({line_count} lines): split it'))

        # 3b. Summary
        # Every note that is not an index note must have, as its first non-empty line after the first
        # line starting with `# `, a line starting with `> `. Else ERROR line 0
        if not is_index:
            found_title = False
            has_summary = False
            for line in lines:
                if not found_title:
                    if line.startswith('# '):
                        found_title = True
                else:
                    if line.strip() == '':
                        continue
                    if line.startswith('> '):
                        has_summary = True
                    break
            if not has_summary:
                problems.append(('ERROR', relpath, 0, 'no summary line ("> ...") under the title'))

        # 3a & 3c: Links and Files section
        in_fence = False
        in_files_section = False

        for line_no, raw_line in enumerate(lines, start=1):
            stripped = raw_line.strip()
            if stripped.startswith('```'):
                in_fence = not in_fence
                continue

            if in_fence:
                continue

            # Heading checks for Files section
            if raw_line.rstrip() == '## Files':
                in_files_section = True
            elif raw_line.startswith('#'):
                in_files_section = False
            elif in_files_section:
                # 3c. Files section
                m_file = _FILES_BULLET_RE.match(raw_line)
                if m_file:
                    target_path_str = m_file.group(1)
                    if '<' not in target_path_str:
                        if '*' in target_path_str or '?' in target_path_str:
                            matches = list(repo.glob(target_path_str))
                            if not matches:
                                problems.append(('ERROR', relpath, line_no, f'listed file is missing: {target_path_str}'))
                        else:
                            p = repo / target_path_str
                            if not p.exists():
                                problems.append(('ERROR', relpath, line_no, f'listed file is missing: {target_path_str}'))

            # 3a. Links
            # Remove inline code spans (`...`) before matching
            clean_line = re.sub(r'`[^`]*`', '', raw_line)
            for m in _LINK_RE.finditer(clean_line):
                target_as_written = m.group(1) if m.group(1) is not None else m.group(2)
                if (target_as_written.startswith('http://') or
                        target_as_written.startswith('https://') or
                        target_as_written.startswith('mailto:') or
                        target_as_written.startswith('#')):
                    continue

                target_cut = target_as_written.split('#', 1)[0]
                unquoted = urllib.parse.unquote(target_cut)
                resolved = (note.parent / unquoted).resolve()

                if not resolved.exists():
                    problems.append(('ERROR', relpath, line_no, f'broken link: {target_as_written}'))
                else:
                    if resolved.is_file() and resolved.suffix == '.md':
                        try:
                            resolved.relative_to(root.resolve())
                            note_links[note].append(resolved)
                        except ValueError:
                            pass

    # 3d. Reachable (skip if root/README.md is missing)
    if not readme_missing:
        visited: set[Path] = set()
        resolved_root_readme = root_readme.resolve()
        # Find note object matching root_readme
        queue: list[Path] = [resolved_root_readme]
        visited.add(resolved_root_readme)

        # Build mapping from resolved path to Path object in note_links
        resolved_to_links: dict[Path, list[Path]] = {}
        for n, targets in note_links.items():
            resolved_to_links[n.resolve()] = targets

        while queue:
            curr = queue.pop(0)
            for target in resolved_to_links.get(curr, []):
                target_resolved = target.resolve()
                if target_resolved not in visited:
                    visited.add(target_resolved)
                    queue.append(target_resolved)

        for note in notes:
            if note.resolve() not in visited:
                problems.append(('ERROR', get_relpath(note), 0, 'orphan: no index links to this note'))

    # 4. Unique names. Two notes under root (except files named README.md) with the same file name,
    # compared case-insensitively: ERROR on each, line 0, duplicate note name: <name> (also <other relpath>)
    notes_by_lower_name: dict[str, list[Path]] = {}
    for note in notes:
        if note.name == 'README.md':
            continue
        lower_name = note.name.lower()
        notes_by_lower_name.setdefault(lower_name, []).append(note)

    for lower_name, group in notes_by_lower_name.items():
        if len(group) > 1:
            for note in group:
                rel = get_relpath(note)
                other_rels = [get_relpath(other) for other in group if other != note]
                for other_rel in other_rels:
                    problems.append(('ERROR', rel, 0, f'duplicate note name: {note.name} (also {other_rel})'))

    problems.sort(key=lambda x: (x[1], x[2]))
    return problems


FIND_MAX = 10


def _summary(lines: list[str]) -> str:
    """The "> ..." line under the title, or the title itself."""
    title = ''
    for line in lines:
        if not title:
            if line.startswith('# '):
                title = line[2:].strip()
        elif line.strip():
            return line[2:].strip() if line.startswith('> ') else title
    return title


def find_notes(roots: list[Path], repo: Path, words: list[str]) -> list[tuple[int, str, str]]:
    """Notes matching the words, best first: (score, relpath, summary). Per word: file name 3, summary 2, text 1.
    Every word must match somewhere. Index notes are left out: they only repeat the summaries."""
    words = [w.lower() for w in words if w.strip()]
    hits = []
    for root in roots:
        for dirpath, dirnames, filenames in os.walk(root):
            dirnames[:] = [d for d in dirnames if not d.startswith('.')]
            for f in filenames:
                note = Path(dirpath) / f
                if not f.endswith('.md') or _is_index_note(note, root):
                    continue
                try:
                    lines = note.read_text(encoding='utf-8').splitlines()
                except (OSError, UnicodeDecodeError):
                    continue
                summary, text, name = _summary(lines), '\n'.join(lines).lower(), f.lower()
                scores = [3 if w in name else 2 if w in summary.lower() else 1 if w in text else 0 for w in words]
                if words and all(scores):
                    rel = note.resolve().relative_to(repo.resolve()).as_posix() if note.resolve().is_relative_to(repo.resolve()) else note.as_posix()
                    hits.append((sum(scores), rel, summary))
    return sorted(hits, key=lambda h: (-h[0], h[1]))


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description='Check markdown notes folders.')
    parser.add_argument('folders', nargs='*', default=DEFAULT_FOLDERS,
                        help='Folders to check (relative to repo)')
    parser.add_argument('--repo', default=str(REPO),
                        help='Repository root path')

    parser.add_argument('--find', nargs='+', metavar='WORD',
                        help='search instead of check: notes containing all these words, one line each, best first')

    args = parser.parse_args(argv)
    repo = Path(args.repo).resolve()

    if args.find is not None:
        roots = [(repo / f).resolve() for f in args.folders if (repo / f).is_dir()]
        words = ' '.join(args.find).split()
        hits = find_notes(roots, repo, words)
        for _, rel, summary in hits[:FIND_MAX]:
            print(f'{rel}: {summary}')
        if len(hits) > FIND_MAX:
            print(f'... {len(hits) - FIND_MAX} more: add a word')
        if not hits:
            print('no note matches all of: ' + ' '.join(words))
        return 0 if hits else 1

    folders_to_check = args.folders
    is_default = (folders_to_check == DEFAULT_FOLDERS)

    all_problems: list[tuple[str, str, int, str]] = []
    total_notes = 0

    for folder_str in folders_to_check:
        folder_path = (repo / folder_str).resolve()
        if not folder_path.is_dir():
            if is_default:
                continue
            all_problems.append(('ERROR', folder_str.replace('\\', '/'), 0, 'folder not found'))
            continue

        folder_notes = []
        for dirpath, dirnames, filenames in os.walk(folder_path):
            dirnames[:] = [d for d in dirnames if not d.startswith('.')]
            for f in filenames:
                if f.endswith('.md'):
                    folder_notes.append(Path(dirpath) / f)
        total_notes += len(folder_notes)

        probs = check_folder(folder_path, repo)
        all_problems.extend(probs)

    errors = sum(1 for p in all_problems if p[0] == 'ERROR')
    warnings = sum(1 for p in all_problems if p[0] == 'WARN')

    for level, relpath, line, message in all_problems:
        print(f'{level} {relpath}:{line}: {message}')

    print(f'check_docs: {total_notes} notes, {errors} errors, {warnings} warnings')

    return 1 if errors > 0 else 0


if __name__ == '__main__':
    sys.exit(main())
