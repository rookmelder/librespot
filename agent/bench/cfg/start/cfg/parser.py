def parse_line(line):
    """'key = value' -> (key, value)."""
    key, value = line.split('=')
    return key.strip(), value.strip()
