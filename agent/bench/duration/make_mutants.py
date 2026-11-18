"""Write hidden/mutants/*.py: copies of start/duration.py, each with one planted bug. Run again after changing duration.py:
  python agent/bench/duration/make_mutants.py
A tester's tests 'kill' a mutant when they fail on it. Each replacement must match exactly once."""
from pathlib import Path

HERE = Path(__file__).resolve().parent
MUTANTS = {
    'm01_any_order': [('if i <= last:', 'if i == last:')],
    'm02_repeat_allowed': [('if i <= last:', 'if i < last:')],
    'm03_no_bare_number': [("if re.fullmatch(r'[0-9]+', s):", 'if False:')],
    'm04_day_size': [("('d', 86400)", "('d', 43200)")],
    'm05_zero_empty': [("return '0s'", "return ''")],
    'm06_zero_parts': [('        if n:\n', '        if n or out:\n')],
    'm07_negative_ok': [(' or seconds < 0:', ':')],
    'm08_bool_ok': [('isinstance(seconds, bool) or ', '')],
    'm09_case_sensitive': [('s = text.strip().lower()', 's = text.strip()')],
    'm10_no_spaces': [('for chunk in s.split():', 'for chunk in [s]:')],
    'm11_empty_is_zero': [("raise ValueError('empty duration')", 'return 0')],
    'm12_message_text': [("raise ValueError(f'bad duration: {text}')\n        for", "raise ValueError(f'bad duration: {s}')\n        for"),
                         ("                raise ValueError(f'bad duration: {text}')", "                raise ValueError(f'bad duration: {s}')")],
}


def main():
    src = (HERE / 'start' / 'duration.py').read_text(encoding='utf-8')
    out = HERE / 'hidden' / 'mutants'
    out.mkdir(parents=True, exist_ok=True)
    for old in out.glob('*.py'):
        old.unlink()
    for name, edits in MUTANTS.items():
        text = src
        for a, b in edits:
            assert text.count(a) == 1, f'{name}: {a!r} found {text.count(a)} times'
            text = text.replace(a, b)
        (out / f'{name}.py').write_text(text, encoding='utf-8', newline='\n')
    print(f'{len(MUTANTS)} mutants written to {out}')


if __name__ == '__main__':
    main()
