"""Durations like '1h30m' <-> seconds."""
import re

UNITS = (('d', 86400), ('h', 3600), ('m', 60), ('s', 1))
SECONDS = dict(UNITS)


def parse_duration(text):
    s = text.strip().lower()
    if not s:
        raise ValueError('empty duration')
    if re.fullmatch(r'[0-9]+', s):
        return int(s)
    total, last = 0, -1
    for chunk in s.split():
        if not re.fullmatch(r'(?:[0-9]+[dhms])+', chunk):
            raise ValueError(f'bad duration: {text}')
        for num, unit in re.findall(r'([0-9]+)([dhms])', chunk):
            i = 'dhms'.index(unit)
            if i <= last:
                raise ValueError(f'bad duration: {text}')
            last = i
            total += int(num) * SECONDS[unit]
    return total


def format_duration(seconds):
    if isinstance(seconds, bool) or not isinstance(seconds, int) or seconds < 0:
        raise ValueError('seconds must be a non-negative int')
    if seconds == 0:
        return '0s'
    out = ''
    for unit, size in UNITS:
        n, seconds = divmod(seconds, size)
        if n:
            out += f'{n}{unit}'
    return out
