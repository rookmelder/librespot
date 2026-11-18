class Store:
    def __init__(self):
        self.data = {}
        self.locked = set()

    def get(self, key):
        return self.data.get(key)

    def lock(self, key):
        self.locked.add(key)

    def set(self, key, value):
        if key in self.locked:
            raise PermissionError(f'read-only key: {key}')
        self.data[key] = value

    def set_many(self, pairs):
        saved = dict(self.data)
        try:
            for key, value in pairs.items():
                self.set(key, value)
        except Exception:
            self.data.clear()
            self.data.update(saved)
            raise
