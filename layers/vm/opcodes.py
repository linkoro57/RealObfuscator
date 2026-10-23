"""
RealObfuscator VM Opcodes
======================
Register-based instruction set with per-build randomized values AND
multiple aliases per semantic opcode (multiple distinct byte values that
map to the same handler), optional fused operations and per-operation schemas.
"""

import random
from dataclasses import dataclass, field
from typing import Dict, List, Tuple

from .instruction import (
    FORMAT_NONE, FORMAT_A, FORMAT_AB, FORMAT_ABC, FORMAT_ABCD,
    FORMAT_ABX, FORMAT_ASBX, FORMAT_SBX, instruction_size, operand_count,
)


# (name, format, default_alias_count)
INSTRUCTION_DEFS: List[Tuple[str, str, int]] = [
    # Movement / loading
    ('MOVE',      FORMAT_AB,   2),
    ('LOADK',     FORMAT_ABX,  3),
    ('LOADBOOL',  FORMAT_ABC,  1),
    ('LOADNIL',   FORMAT_AB,   1),

    # Upvalues / globals
    ('GETUPVAL',  FORMAT_AB,   2),
    ('SETUPVAL',  FORMAT_AB,   2),
    ('GETGLOBAL', FORMAT_ABX,  2),
    ('SETGLOBAL', FORMAT_ABX,  2),

    # Tables
    ('NEWTABLE',  FORMAT_ABC,  1),
    ('GETTABLE',  FORMAT_ABC,  2),
    ('SETTABLE',  FORMAT_ABC,  2),
    ('GETTABLEK', FORMAT_ABC,  2),
    ('SETTABLEK', FORMAT_ABC,  2),
    ('SELF',      FORMAT_ABC,  1),
    ('SETLIST',   FORMAT_ABC,  1),

    # Arithmetic
    ('ADD',       FORMAT_ABC,  3),
    ('SUB',       FORMAT_ABC,  3),
    ('MUL',       FORMAT_ABC,  2),
    ('DIV',       FORMAT_ABC,  2),
    ('MOD',       FORMAT_ABC,  1),
    ('POW',       FORMAT_ABC,  1),
    ('UNM',       FORMAT_AB,   1),
    ('NOT',       FORMAT_AB,   1),
    ('LEN',       FORMAT_AB,   1),
    ('CONCAT',    FORMAT_ABC,  1),

    # Comparison + conditional jump
    ('EQ',        FORMAT_ABC,  2),
    ('LT',        FORMAT_ABC,  2),
    ('LE',        FORMAT_ABC,  1),
    ('TEST',      FORMAT_AB,   2),
    ('TESTSET',   FORMAT_ABC,  1),

    # Control flow
    ('JMP',       FORMAT_SBX,  3),
    ('CALL',      FORMAT_ABC,  3),
    ('TAILCALL',  FORMAT_ABC,  1),
    ('RETURN',    FORMAT_AB,   2),

    # Loop helpers
    ('FORPREP',   FORMAT_ASBX, 1),
    ('FORLOOP',   FORMAT_ASBX, 1),
    ('TFORLOOP',  FORMAT_ABC,  1),
    ('TFORPREP',  FORMAT_A,    1),

    # Closures / varargs
    ('CLOSURE',   FORMAT_ABX,  2),
    ('VARARG',    FORMAT_AB,   1),

    # Captured-local boxes (proper closure semantics)
    ('MKBOX',     FORMAT_AB,   1),  # R[A] := {R[B]}      box wraps a value
    ('GETBOX',    FORMAT_AB,   2),  # R[A] := R[B][1]     read boxed local
    ('SETBOX',    FORMAT_AB,   2),  # R[A][1] := R[B]     write boxed local

    # No-op
    ('NOP',       FORMAT_NONE, 2),
]


@dataclass
class OpcodeInfo:
    name: str
    fmt: str
    aliases: List[int] = field(default_factory=list)
    layout: Tuple[int, ...] = ()
    byte_order: str = 'little'
    components: Tuple[str, ...] = ()

    @property
    def primary(self) -> int:
        return self.aliases[0]


class OpcodeMap:
    """
    Maps semantic opcode names to one or more byte values (aliases).
    All byte values are unique across the whole map.
    """

    _RESERVED = {0}

    def __init__(self, rng: random.Random, *, diversify=False, fusion=False, extended=False, handler_variants=False):
        self.rng = rng
        self.opcodes: Dict[str, OpcodeInfo] = {}
        self._by_value: Dict[int, OpcodeInfo] = {}
        self.definitions = list(INSTRUCTION_DEFS)
        if fusion:
            operations = ['ADD','SUB','MUL','DIV'] + (['MOD','POW'] if extended else [])
            for op in operations:
                self.definitions.append(('F'+op+'R', FORMAT_ABCD, 2))
                if extended:
                    self.definitions.append(('F'+op+'L', FORMAT_ABCD, 2))
        self.diversify = diversify
        self.table_flush_width = rng.choice((17,23,31,47,61)) if handler_variants else 50
        self._blocks = {}
        self._block_limit = 96 if extended else 48
        self._generate()

    def block(self, names):
        """Allocate a build-specific opcode/schema for an operation sequence.

        Bound generated handler growth and retain ordinary opcodes when the
        budget is full. A sequence is shared only within this output's map.
        """
        names = tuple(names)
        if names in self._blocks:
            return self._blocks[names]
        available = [v for v in range(1,256) if v not in self._by_value]
        if len(self._blocks) >= self._block_limit or not available:
            return None
        count = sum(operand_count(self.get(name).fmt) for name in names)
        if count > 16:
            return None
        name = 'BLOCK'+str(len(self._blocks))
        info = OpcodeInfo(name=name,fmt='X'+str(count),
            aliases=[self.rng.choice(available)],components=names)
        layout = list(range(count))
        self.rng.shuffle(layout)
        info.layout = tuple(layout)
        info.byte_order = self.rng.choice(('little','big'))
        self.opcodes[name] = info
        self._by_value[info.primary] = info
        self._blocks[names] = info
        return info

    def _generate(self):
        total_aliases = sum(count for _, _, count in self.definitions)
        available = [v for v in range(1, 256) if v not in self._RESERVED]
        if total_aliases > len(available):
            raise RuntimeError(f"Too many opcode aliases: {total_aliases} > {len(available)}")

        chosen = self.rng.sample(available, total_aliases)
        idx = 0
        for name, fmt, count in self.definitions:
            aliases = chosen[idx:idx + count]
            idx += count
            info = OpcodeInfo(name=name, fmt=fmt, aliases=aliases)
            layout = list(range(operand_count(fmt)))
            # Closure link records share this stable two-operand schema;
            # the CLOSURE handler consumes them without a normal dispatch.
            if self.diversify and name not in ('MOVE','GETUPVAL'):
                self.rng.shuffle(layout)
                info.byte_order = self.rng.choice(('little','big'))
            info.layout = tuple(layout)
            self.opcodes[name] = info
            for v in aliases:
                self._by_value[v] = info

    def get(self, name: str) -> OpcodeInfo:
        return self.opcodes[name]

    def primary(self, name: str) -> int:
        return self.opcodes[name].primary

    def random_alias(self, name: str) -> int:
        return self.rng.choice(self.opcodes[name].aliases)

    def fmt_of(self, name: str) -> str:
        return self.opcodes[name].fmt

    def all_aliases(self) -> List[Tuple[int, OpcodeInfo]]:
        return list(self._by_value.items())

    def fmt_of_byte(self, byte: int) -> str:
        return self._by_value[byte].fmt

    def name_of_byte(self, byte: int) -> str:
        return self._by_value[byte].name
