Goal: fix and extend the small settings package `cfg` (parser, store, service), so loading settings is safe, and write its tests.

Files: change `cfg/parser.py`, `cfg/store.py` and `cfg/service.py` (all shown below; `cfg/__init__.py` stays empty)
and create `tests/test_cfg.py` (the `tests` folder exists). Change no other file. Keep every existing name.

Spec:

`cfg/parser.py`, `parse_line(line)`:
- Strip the line. Empty, or starting with `#`: return `None`.
- No `=` in it: `ValueError(f'no = in line: {stripped line}')`.
- Split on the FIRST `=` only (a value may contain `=`). Strip key and value.
- The key must fully match `[a-z][a-z0-9_.]*`, else `ValueError(f'bad key: {key}')`. An empty value is allowed.
- Returns `(key, value)`.

`cfg/store.py`, class `Store` (keeps `self.data` and `get`):
- New `lock(key)`: from then on `set(key, ...)` raises `PermissionError(f'read-only key: {key}')`. A key can be locked
  before it has a value.
- `get(key)` returns `None` for a missing key (as now).
- New `set_many(pairs)`: `pairs` is a dict; set each key in the dict's order. All or nothing: if any `set` raises,
  put `self.data` back exactly as it was (a key that did not exist before is gone again), then raise that same
  exception object again.

`cfg/service.py`, class `Service` (keeps `self.store` and `self.cache`):
- `get(key, default=None)`: the value from the store, cached in `self.cache` after the first read. A key the store
  does not have returns `default` and is NOT cached (so a later load can add it).
- `load(text)`: parse every line first (line numbers start at 1). A line that fails to parse:
  `ValueError(f'line {n}: {message}')` with the parser's message, and nothing is stored. Lines that give `None` are
  skipped. When a key appears twice, the last value wins. Then store everything with ONE `store.set_many(...)`.
  - If that raises, the store is unchanged (set_many restored it), `self.cache` is unchanged, and the exception goes
    up unchanged.
  - On success, remove every loaded key from `self.cache` (so `get` sees the new value) and return the number of
    distinct keys stored.

Must cover: every parser rule and message, lock and the exact PermissionError text, set_many rollback (changed keys
restored, new keys removed, the same exception re-raised), load's line numbers, last-wins, that a failed load changes
neither the store nor the cache, and that `get` sees new values after a successful load.
