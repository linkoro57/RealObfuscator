"""Install the pinned official Luau CLI used by differential tests."""
from pathlib import Path
import platform
import urllib.request
import zipfile
from io import BytesIO

VERSION = "0.741"
system = {"Windows": "windows", "Linux": "ubuntu", "Darwin": "macos"}[platform.system()]
target = Path(__file__).resolve().parents[1] / "tests" / "runtime" / VERSION
target.mkdir(parents=True, exist_ok=True)
url = f"https://github.com/luau-lang/luau/releases/download/{VERSION}/luau-{system}.zip"
with urllib.request.urlopen(url, timeout=60) as response:
    archive = zipfile.ZipFile(BytesIO(response.read()))
for name in ("luau", "luau-compile"):
    name += ".exe" if system == "windows" else ""
    destination = target / name
    destination.write_bytes(archive.read(name))
    destination.chmod(0o755)
print(f"Installed Luau {VERSION} in {target}")
