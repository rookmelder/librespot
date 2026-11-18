import re


def slugify(text, max_len=40):
    if max_len < 1:
        raise ValueError('max_len must be at least 1')
    slug = re.sub(r'[^a-z0-9]+', '-', text.lower()).strip('-')
    if len(slug) > max_len:
        slug = slug[:max_len].rstrip('-')
    return slug or 'n-a'


def wrap(text, width):
    if width < 1:
        raise ValueError('width must be at least 1')
    lines, cur = [], ''
    for word in text.split():
        while len(word) > width:
            if cur:
                lines.append(cur)
                cur = ''
            lines.append(word[:width])
            word = word[width:]
        if not cur:
            cur = word
        elif len(cur) + 1 + len(word) <= width:
            cur += ' ' + word
        else:
            lines.append(cur)
            cur = word
    if cur:
        lines.append(cur)
    return lines
