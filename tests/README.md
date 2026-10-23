# Tests

```sh
python -m unittest discover -s tests -v
```

Runtime checks need the official [Luau tools](https://github.com/luau-lang/luau/releases/tag/0.741).
Place `luau.exe` in `tests/runtime/0.741/`, or set `LUAU_BIN` to its absolute path.
Without Luau, runtime checks skip. Set `REALOBFUSCATOR_REQUIRE_RUNTIME=1` to require it.
The suite covers CLI behavior, returns, assignments, iteration and payload checks.
