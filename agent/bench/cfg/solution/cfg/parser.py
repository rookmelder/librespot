import re

KEY = re.compile(r'[a-z][a-z0-9_.]*')


def parse_line(line):
    """'key = value' -> (key, value); None for a blank line or a # comment."""
    s = line.strip()
    if not s or s.startswith('#'):
        return None
    if '=' not in s:
        raise ValueError(f'no = in line: {s}')
    key, value = s.split('=', 1)
    key = key.strip()
    if not KEY.fullmatch(key):
        raise ValueError(f'bad key: {key}')
    return key, value.strip()
