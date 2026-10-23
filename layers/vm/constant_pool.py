"""
RealObfuscator VM Constant Pool
==============================
Holds and deduplicates literal values (numbers, strings, booleans, nil)
and provides per-build encoding for string and numeric content.

Hardened numeric values use round-trip decimal text in the same encrypted
pool as strings. The interpreter decodes each function pool once, preserving
direct numeric lookups afterward.
"""

import base64
import math
import random
from typing import List, Any, Dict


class ConstantPool:
    """Manages deduplicated literal constants for a VM build."""

    PACKING_THRESHOLD = 16

    def __init__(self, rng: random.Random, whiten: bool = False):
        self.rng = rng
        self.whiten = whiten
        self.constants: List[Any] = []
        self._index: Dict[Any, int] = {}
        # Per-build repeating XOR key (4-16 bytes)
        key_len = self.rng.randint(4, 16)
        self.key: List[int] = [self.rng.randint(1, 255) for _ in range(key_len)]

    # ---- Insertion ----

    def add(self, value: Any) -> int:
        key = self._make_key(value)
        if key in self._index:
            return self._index[key]
        idx = len(self.constants)
        self.constants.append(value)
        self._index[key] = idx
        return idx

    def _make_key(self, value: Any) -> Any:
        if value is None:
            return ('nil',)
        if isinstance(value, bool):
            return ('bool', value)
        if isinstance(value, (int, float)):
            return ('num', float(value).hex())
        if isinstance(value, str):
            return ('str', value)
        return ('other', repr(value))

    # ---- Encryption ----

    def encrypt_string(self, s: str, index: int = 1) -> str:
        """Return an escaped Luau string body for the encrypted bytes."""
        return ''.join(f"\\{byte:03d}" for byte in self.encrypt_bytes(s, index))

    def encrypt_bytes(self, s: str, index: int = 1) -> bytes:
        """Encrypt UTF-8 bytes, optionally whitening each pool position."""
        raw = s.encode('utf-8', 'surrogateescape')
        encrypted = []
        klen = len(self.key)
        for offset, byte in enumerate(raw):
            position = offset + 1
            value = byte ^ self.key[offset % klen]
            if self.whiten:
                secondary = self.key[(index + position * 3 - 2) % klen]
                value ^= (index * 131 + position * 17 + secondary) % 256
            encrypted.append(value)
        return bytes(encrypted)

    # ---- Luau emission ----

    def number_text(self, value) -> str:
        """Round-trip IEEE doubles without arithmetic identities or precision loss."""
        if math.isinf(value):
            return '-inf' if value < 0 else 'inf'
        if math.isnan(value):
            return 'nan'
        return repr(value)

    def encoded_values(self, encode_numbers=False):
        values, pending = [], []
        for index, value in enumerate(self.constants, 1):
            numeric = isinstance(value, (int, float)) and not isinstance(value, bool)
            kind = 2 if numeric and encode_numbers else (1 if isinstance(value, str) else 0)
            text = self.number_text(value) if kind == 2 else value
            if kind:
                value = self.encrypt_bytes(text, index)
            values.append(value)
            pending.append(kind)
        return values, pending

    def to_luau_table(self, encode_numbers=False, unpacker=None) -> str:
        """Generate a Luau table literal containing the (encrypted) constants."""
        entries = []
        for index, c in enumerate(self.constants, 1):
            if c is None:
                entries.append("nil")
            elif isinstance(c, bool):
                entries.append("true" if c else "false")
            elif isinstance(c, (int, float)):
                if encode_numbers:
                    text = self.number_text(c)
                    encrypted = self.encrypt_bytes(text, index)
                    if unpacker and len(encrypted) >= self.PACKING_THRESHOLD:
                        packed = base64.b64encode(encrypted).decode('ascii')
                        entries.append(f'{unpacker}("{packed}")')
                    else:
                        entries.append(f'"{self.encrypt_string(text, index)}"')
                    continue
                if not math.isfinite(c):
                    entries.append('(0/0)' if math.isnan(c) else ('(-1/0)' if c < 0 else '(1/0)'))
                    continue
                # Preserve integers when possible
                if isinstance(c, int) or (isinstance(c, float) and c.is_integer()):
                    entries.append(str(int(c)))
                else:
                    entries.append(repr(float(c)))
            elif isinstance(c, str):
                encrypted = self.encrypt_bytes(c, index)
                if unpacker and len(encrypted) >= self.PACKING_THRESHOLD:
                    packed = base64.b64encode(encrypted).decode('ascii')
                    entries.append(f'{unpacker}("{packed}")')
                else:
                    entries.append(f'"{self.encrypt_string(c, index)}"')
            else:
                entries.append("nil")
        return '{' + ','.join(entries) + '}'

    def is_string(self, idx: int) -> bool:
        return isinstance(self.constants[idx], str)

    def size(self) -> int:
        return len(self.constants)
