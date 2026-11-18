from cfg.parser import parse_line


class Service:
    def __init__(self, store):
        self.store = store
        self.cache = {}

    def get(self, key, default=None):
        if key in self.cache:
            return self.cache[key]
        value = self.store.get(key)
        if value is None:
            return default
        self.cache[key] = value
        return value

    def load(self, text):
        pairs = {}
        for n, line in enumerate(text.splitlines(), 1):
            try:
                parsed = parse_line(line)
            except ValueError as e:
                raise ValueError(f'line {n}: {e}') from None
            if parsed:
                key, value = parsed
                pairs.pop(key, None)
                pairs[key] = value
        self.store.set_many(pairs)
        for key in pairs:
            self.cache.pop(key, None)
        return len(pairs)
