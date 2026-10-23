# RealObfuscator

[![Tests](https://github.com/linkoro57/RealObfuscator/actions/workflows/tests.yml/badge.svg)](https://github.com/linkoro57/RealObfuscator/actions/workflows/tests.yml)
[![Python 3.9+](https://img.shields.io/badge/python-3.9%2B-blue.svg)](https://www.python.org/downloads/)

RealObfuscator is a command-line Luau obfuscator designed for Roblox scripts. It can rename identifiers, encode strings and numbers, transform control flow, inject dead code, and virtualize programs with a custom VM.

> [!IMPORTANT]
> Always keep your original source code and test the generated output in Roblox Studio before publishing it. Obfuscation makes code harder to understand, but it cannot guarantee absolute protection.

## Requirements

- Python 3.9 or later
- `pip`

## Installation

```sh
git clone https://github.com/linkoro57/RealObfuscator.git
cd RealObfuscator
python -m pip install -r requirements.txt
```

You can alternatively install the project and its `realobfuscator` command directly:

```sh
python -m pip install .
```

## Usage

Obfuscate a file with the default lightweight mode:

```sh
python main.py script.luau
```

When installed as a package, the equivalent command is:

```sh
realobfuscator script.luau
```

The result is written to `script.obf.luau`. To choose another destination:

```sh
python main.py script.luau --output build/script.luau
```

You can also process a directory recursively:

```sh
python main.py src --output build
```

Only `.lua` and `.luau` files are processed. Their relative directory structure is preserved in the output directory.

## Modes

| Mode | Description |
| --- | --- |
| `lightweight` | Default mode: identifier renaming and lightweight string encoding. |
| `rename` | Renames identifiers only. |
| `native` | Enables advanced native transformations, including control-flow protection and anti-tamper checks. |
| `vm` | Compiles the program to bytecode executed by a custom Luau VM. |

Examples:

```sh
python main.py script.luau --mode rename
python main.py script.luau --mode native
python main.py script.luau --mode vm
```

VM mode uses rolling bytecode reads by default. Use `--vm-cached` to favor performance by caching decoded bytecode:

```sh
python main.py script.luau --mode vm --vm-cached
```

Display every option, including advanced settings and legacy aliases:

```sh
python main.py --help-all
```

## Library usage

```python
from config import ObfuscationConfig, ProtectionLevel
from obfuscator import Obfuscator

config = ObfuscationConfig(level=ProtectionLevel.MAXIMUM)
result = Obfuscator(config).obfuscate('print("Hello, Roblox!")')

with open("script.obf.luau", "w", encoding="utf-8") as output:
    output.write(result)
```

The available protection levels are `MINIMAL`, `STANDARD`, `MAXIMUM`, and `PARANOID`. The `PARANOID` level enables virtualization with the maximum VM hardening profile.

## Tests

```sh
python -m unittest discover -s tests -v
```

Every push and pull request is also tested on Python 3.9, 3.11, and 3.13 through GitHub Actions.

Runtime tests require the official [Luau tools](https://github.com/luau-lang/luau/releases/tag/0.741). Place the executable in `tests/runtime/0.741/`, or provide its path through `LUAU_BIN`.

To require the Luau runtime while running tests:

```powershell
$env:REALOBFUSCATOR_REQUIRE_RUNTIME = "1"
python -m unittest discover -s tests -v
```

## License

See [LICENSE](LICENSE).
