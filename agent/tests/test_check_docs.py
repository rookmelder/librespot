import contextlib
import io
from pathlib import Path
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import check_docs as cd


def write(path: Path, text: str) -> Path:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding='utf-8')
    return path


class TestCheckDocs(unittest.TestCase):
    def test_clean_folder(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            repo = Path(tmpdir)
            # real file for Files section
            write(repo / 'src' / 'real.py', 'print(1)')
            # a target folder with spaces and a file with space
            write(repo / 'knowledge' / 'space folder' / 'space note.md', '# Space Note\n\n> Summary\n')

            # README -> systems/systems.md -> systems/system-shop.md
            readme_text = (
                "# Knowledge Index\n\n"
                "[Systems](systems/systems.md)\n"
                "[Space](<space folder/space note.md>)\n"
            )
            write(repo / 'knowledge' / 'README.md', readme_text)

            systems_text = (
                "# Systems\n\n"
                "[Shop](system-shop.md#anchor)\n"
                "[Space Note](../space%20folder/space%20note.md)\n"
                "[External](https://example.com)\n"
            )
            write(repo / 'knowledge' / 'systems' / 'systems.md', systems_text)

            shop_text = (
                "# System Shop\n\n"
                "> The shop system handles purchasing items.\n\n"
                "## Files\n"
                "- `src/real.py`: real implementation file\n"
            )
            write(repo / 'knowledge' / 'systems' / 'system-shop.md', shop_text)

            problems = cd.check_folder(repo / 'knowledge', repo)
            self.assertEqual(problems, [])

    def test_broken_link_line_number(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            repo = Path(tmpdir)
            readme_text = (
                "# Index\n\n"
                "Line 3 is text\n"
                "Line 4 [broken link](does_not_exist.md)\n"
            )
            write(repo / 'knowledge' / 'README.md', readme_text)
            problems = cd.check_folder(repo / 'knowledge', repo)
            self.assertEqual(len(problems), 1)
            level, relpath, line, msg = problems[0]
            self.assertEqual(level, 'ERROR')
            self.assertEqual(relpath, 'knowledge/README.md')
            self.assertEqual(line, 4)
            self.assertEqual(msg, 'broken link: does_not_exist.md')

    def test_link_in_code_ignored(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            repo = Path(tmpdir)
            readme_text = (
                "# Index\n\n"
                "Here is an inline code: `[broken](inline_ghost.md)`\n"
                "```python\n"
                "[broken](fence_ghost.md)\n"
                "```\n"
            )
            write(repo / 'knowledge' / 'README.md', readme_text)
            problems = cd.check_folder(repo / 'knowledge', repo)
            self.assertEqual(problems, [])

    def test_orphan_note_and_index_reachability(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            repo = Path(tmpdir)
            write(repo / 'knowledge' / 'README.md', "# Index\n\n[Sub](sub/sub.md)\n")
            # sub/sub.md reached through README, and note-a.md reached through sub/sub.md
            write(repo / 'knowledge' / 'sub' / 'sub.md', "# Sub\n\n[Note A](note-a.md)\n")
            write(repo / 'knowledge' / 'sub' / 'note-a.md', "# Note A\n\n> Summary\n")
            # orphan.md is not linked anywhere
            write(repo / 'knowledge' / 'orphan.md', "# Orphan\n\n> Summary\n")

            problems = cd.check_folder(repo / 'knowledge', repo)
            self.assertEqual(len(problems), 1)
            self.assertEqual(problems[0], ('ERROR', 'knowledge/orphan.md', 0, 'orphan: no index links to this note'))

    def test_missing_summary_and_index_exemption(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            repo = Path(tmpdir)
            write(repo / 'knowledge' / 'README.md', "# Index\n\n[Note](note.md)\n[Sub Index](sub/sub.md)\n")
            # sub/sub.md is an index note, no summary needed
            write(repo / 'knowledge' / 'sub' / 'sub.md', "# Sub Index\n\nContent without summary\n")
            # note.md has no summary line
            write(repo / 'knowledge' / 'note.md', "# Note\n\nSome paragraph without quote\n")

            problems = cd.check_folder(repo / 'knowledge', repo)
            self.assertEqual(len(problems), 1)
            self.assertEqual(problems[0], ('ERROR', 'knowledge/note.md', 0, 'no summary line ("> ...") under the title'))

            # summary after blank line is fine
            write(repo / 'knowledge' / 'note.md', "# Note\n\n\n> Valid summary\n\nBody\n")
            problems = cd.check_folder(repo / 'knowledge', repo)
            self.assertEqual(problems, [])

    def test_files_section(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            repo = Path(tmpdir)
            write(repo / 'src' / 'real.py', 'code')
            write(repo / 'src' / 'module' / 'test1.py', 'code')

            # Outside ## Files section, backticked path is not checked
            # Inside ## Files section:
            # - `src/real.py` (exists)
            # - `src/module/*.py` (glob matches)
            # - `src/<placeholder>/file.py` (skipped)
            # - `src/missing.py` (ERROR)
            # - `src/missing_glob/*.py` (ERROR)
            note_text = (
                "# Note\n\n"
                "> Summary\n\n"
                "Here is `not_a_file.txt` outside Files.\n\n"
                "## Files\n"
                "- `src/real.py`: exists\n"
                "- `src/module/*.py`: glob matches\n"
                "- `<placeholder>/foo.py`: placeholder skipped\n"
                "- `src/missing.py`: missing\n"
                "- `src/none/*.txt`: missing glob\n\n"
                "## Other Section\n"
                "- `src/also_ignored.py`: ignored\n"
            )
            write(repo / 'knowledge' / 'README.md', "# Index\n\n[Note](note.md)\n")
            write(repo / 'knowledge' / 'note.md', note_text)

            problems = cd.check_folder(repo / 'knowledge', repo)
            expected = [
                ('ERROR', 'knowledge/note.md', 11, 'listed file is missing: src/missing.py'),
                ('ERROR', 'knowledge/note.md', 12, 'listed file is missing: src/none/*.txt'),
            ]
            self.assertEqual(problems, expected)

    def test_missing_readme(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            repo = Path(tmpdir)
            write(repo / 'knowledge' / 'note.md', "# Note\n\n> Summary\n")
            problems = cd.check_folder(repo / 'knowledge', repo)
            # missing index README.md on line 0, and reachability check is skipped (no orphan error for note.md)
            self.assertEqual(problems, [('ERROR', 'knowledge/README.md', 0, 'missing index README.md')])

    def test_long_note_warn_only(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            repo = Path(tmpdir)
            # MAX_INDEX_LINES is 80, MAX_NOTE_LINES is 60
            readme_lines = ["# Index\n\n[Note](note.md)\n"] + [f"line {i}" for i in range(85)]
            write(repo / 'knowledge' / 'README.md', "\n".join(readme_lines))

            note_lines = ["# Note\n\n> Summary\n"] + [f"line {i}" for i in range(65)]
            write(repo / 'knowledge' / 'note.md', "\n".join(note_lines))

            problems = cd.check_folder(repo / 'knowledge', repo)
            levels = [p[0] for p in problems]
            self.assertNotIn('ERROR', levels)
            self.assertEqual(len(problems), 2)
            self.assertEqual(problems[0], ('WARN', 'knowledge/README.md', 0, 'long note (89 lines): split it'))
            self.assertEqual(problems[1], ('WARN', 'knowledge/note.md', 0, 'long note (69 lines): split it'))

    def test_duplicate_note_names(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            repo = Path(tmpdir)
            write(repo / 'knowledge' / 'README.md', "# Index\n\n[A](sub1/item.md)\n[B](sub2/ITEM.md)\n")
            write(repo / 'knowledge' / 'sub1' / 'item.md', "# Item 1\n\n> Summary\n")
            write(repo / 'knowledge' / 'sub2' / 'ITEM.md', "# Item 2\n\n> Summary\n")

            problems = cd.check_folder(repo / 'knowledge', repo)
            self.assertEqual(len(problems), 2)
            self.assertEqual(problems[0], ('ERROR', 'knowledge/sub1/item.md', 0, 'duplicate note name: item.md (also knowledge/sub2/ITEM.md)'))
            self.assertEqual(problems[1], ('ERROR', 'knowledge/sub2/ITEM.md', 0, 'duplicate note name: ITEM.md (also knowledge/sub1/item.md)'))

    def test_find(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            repo = Path(tmpdir)
            write(repo / 'knowledge' / 'README.md', "# Index\n\n> shop index, must not be listed\n")
            write(repo / 'knowledge' / 'systems' / 'systems.md', "# Systems\n\nshop\n")
            write(repo / 'knowledge' / 'systems' / 'system-shop.md', "# System: Shop\n> Buying items with coins.\n\nPrices in cents.\n")
            write(repo / 'knowledge' / 'findings' / 'finding-coins.md', "# Coins\n\n> Coins are saved per shop visit.\n")
            write(repo / 'knowledge' / 'findings' / 'finding-ui.md', "# UI\n> Buttons use Activated.\n\nThe shop button too.\n")
            write(repo / 'knowledge' / 'findings' / 'finding-old.md', "# Old note without summary\n\ntext about the shop\n")
            hits = cd.find_notes([repo / 'knowledge'], repo, ['Shop'])
            self.assertEqual([h[1] for h in hits], ['knowledge/systems/system-shop.md',   # name 3
                                                    'knowledge/findings/finding-coins.md',  # summary 2
                                                    'knowledge/findings/finding-old.md',    # text 1, ties by path
                                                    'knowledge/findings/finding-ui.md'])
            self.assertEqual(hits[0][2], 'Buying items with coins.')
            self.assertEqual(hits[2][2], 'Old note without summary')   # no summary: the title
            self.assertEqual([h[1] for h in cd.find_notes([repo / 'knowledge'], repo, ['shop', 'cents'])],
                             ['knowledge/systems/system-shop.md'])   # every word must match
            self.assertEqual(cd.find_notes([repo / 'knowledge'], repo, ['nothing']), [])

            out = io.StringIO()
            with contextlib.redirect_stdout(out):
                self.assertEqual(cd.main(['--repo', str(repo), '--find', 'shop', 'cents']), 0)
            self.assertEqual(out.getvalue(), 'knowledge/systems/system-shop.md: Buying items with coins.\n')
            for i in range(12):
                write(repo / 'knowledge' / 'findings' / f'finding-x{i:02}.md', f"# X{i}\n> about shop {i}\n")
            out = io.StringIO()
            with contextlib.redirect_stdout(out):
                self.assertEqual(cd.main(['--repo', str(repo), '--find', 'shop']), 0)
            self.assertEqual(len(out.getvalue().splitlines()), cd.FIND_MAX + 1)
            self.assertIn('... 6 more: add a word', out.getvalue())
            with contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(cd.main(['--repo', str(repo), '--find', 'zebra']), 1)

    def test_main(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            repo = Path(tmpdir)
            write(repo / 'knowledge' / 'README.md', "# Index\n")

            # Clean repo with default folders (agent/notes missing, silently skipped)
            buf = io.StringIO()
            with contextlib.redirect_stdout(buf):
                exit_code = cd.main(['--repo', str(repo)])
            self.assertEqual(exit_code, 0)
            output = buf.getvalue().strip()
            self.assertTrue(output.endswith('check_docs: 1 notes, 0 errors, 0 warnings'))

            # Error exit code when folder has error
            write(repo / 'knowledge' / 'orphan.md', "# Orphan\n\n> Summary\n")
            buf = io.StringIO()
            with contextlib.redirect_stdout(buf):
                exit_code = cd.main(['--repo', str(repo)])
            self.assertEqual(exit_code, 1)
            output = buf.getvalue().strip()
            self.assertIn('ERROR knowledge/orphan.md:0: orphan: no index links to this note', output)
            self.assertTrue(output.endswith('check_docs: 2 notes, 1 errors, 0 warnings'))

            # Missing folder given on CLI is ERROR and exit 1
            buf = io.StringIO()
            with contextlib.redirect_stdout(buf):
                exit_code = cd.main(['nonexistent_folder', '--repo', str(repo)])
            self.assertEqual(exit_code, 1)
            output = buf.getvalue().strip()
            self.assertIn('ERROR nonexistent_folder:0: folder not found', output)
            self.assertTrue(output.endswith('check_docs: 0 notes, 1 errors, 0 warnings'))


if __name__ == '__main__':
    unittest.main()
