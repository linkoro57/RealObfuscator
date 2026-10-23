"""Canonical loader checksum and ciphertext-bound sequential key records.

This is analysis resistance and accidental/tamper detection, not cryptography:
an analyst can emulate the loader or replace its embedded checks.
"""
import math


def key_record(key, salt, rng, encoded):
    if not encoded:
        return list(key)
    seed, multiplier, increment = rng.randrange(65536), rng.randrange(129,4096,2), rng.randrange(65536)
    state, previous = (seed + salt) % 65536, 0
    cipher = []
    for i, value in enumerate(key, 1):
        state = (state * multiplier + increment + previous * 257 + i) % 65536
        cipher.append(value ^ (state // 256) ^ (state % 256))
        previous = value
    return dict(q=cipher, s=seed, a=multiplier, d=increment)


class PayloadHash:
    def __init__(self, seed, multiplier):
        self.value, self.multiplier = seed, multiplier

    def byte(self, value):
        self.value = (self.value * self.multiplier + value) % 4294967296

    def uint(self, value):
        for _ in range(4):
            self.byte(value % 256)
            value //= 256

    def blob(self, values):
        self.uint(len(values))
        for value in values:
            self.byte(value)

    def key(self, record):
        self.byte(int(isinstance(record, dict)))
        if isinstance(record, dict):
            self.uint(record['s']); self.uint(record['a']); self.uint(record['d'])
            self.blob(record['q'])
        else:
            self.blob(record)

    def constant(self, value):
        if value is None:
            self.byte(0)
        elif isinstance(value, bool):
            self.byte(1); self.byte(int(value))
        elif isinstance(value, (int, float)):
            self.byte(2)
            value = float(value)
            if math.isnan(value):
                self.byte(3)
            elif math.isinf(value):
                self.byte(2 if value < 0 else 1)
            else:
                self.byte(0); self.byte(int(math.copysign(1,value) < 0))
                mantissa, exponent = math.frexp(abs(value))
                self.uint(exponent + 2048)
                mantissa = int(mantissa * 9007199254740992)
                for _ in range(7):
                    self.byte(mantissa % 256); mantissa //= 256
        else:
            self.byte(3)
            self.blob(value if isinstance(value, bytes) else value.encode('utf-8','surrogateescape'))

    def pool(self, values, pending, key):
        self.uint(len(values)); self.key(key)
        for value,kind in zip(values,pending):
            self.byte(kind); self.constant(value)
