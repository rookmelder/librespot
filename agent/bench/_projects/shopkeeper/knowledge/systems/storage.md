# storage
> One JSON file per shop; versioned format with upgrades from old versions; atomic save.

Back to [the map](../README.md).

- File: `shop/storage.py`. Tests: `tests/test_storage.py`. The format is in the module docstring.
- `VERSION` is the version written. `upgrade()` turns any older supported version into the current one, one
  version step at a time, without changing its input; newer or unknown versions are refused.
- Orders go through `models.order_to_dict` / `order_from_dict`; a new Order field needs a default so old files load.
- `save()` writes a temp file in the same folder and `os.replace`s it.
