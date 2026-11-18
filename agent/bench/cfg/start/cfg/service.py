from cfg.parser import parse_line


class Service:
    def __init__(self, store):
        self.store = store
        self.cache = {}

    def get(self, key):
        if key not in self.cache:
            self.cache[key] = self.store.get(key)
        return self.cache[key]

    def load(self, text):
        for line in text.splitlines():
            key, value = parse_line(line)
            self.store.set(key, value)
