"""
RealObfuscator VM Instruction Format
=================================
Register-based instruction encoding with variable-length operands.

Each instruction starts with a single opcode byte followed by 0-4 operands;
generated mixed instructions carry up to 16 operands for two to four operations.
Operands are bounded 16-bit values. Hardened builds select operand order and
byte order per operation. Signed branches use bias encoding; iterator branches
use two's complement. Closure link records retain a stable two-operand schema.

The flat byte stream produced by `encode_instruction()` is then encoded with
a build-specific, random-access key stream before being embedded in the output.
"""

from dataclasses import dataclass, field
from typing import List, Optional


# Operand encoding: every operand is 2 bytes; byte order is selected per opcode.
# Signed operands (jump offsets) are biased by 0x8000.
SBX_BIAS = 0x8000


# Instruction format categories. Used by both compiler and interpreter.
# 'A'   = single 16-bit register or count
# 'AB'  = two 16-bit operands (e.g. MOVE dst src)
# 'ABC' = three 16-bit operands (e.g. ADD dst lhs rhs)
# 'ABx' = same as AB but B is a constant pool index (semantic only)
# 'AsBx'= A + signed jump offset
# 'sBx' = signed jump offset only
# ''    = no operands

FORMAT_NONE = ''
FORMAT_A    = 'A'
FORMAT_AB   = 'AB'
FORMAT_ABC  = 'ABC'
FORMAT_ABCD = 'ABCD'
FORMAT_ABX  = 'ABx'
FORMAT_ASBX = 'AsBx'
FORMAT_SBX  = 'sBx'


def operand_count(fmt: str) -> int:
    if fmt.startswith('X') and fmt[1:].isdigit():
        count = int(fmt[1:])
        if 0 < count <= 16:
            return count
        raise ValueError(f'Invalid composite VM operand count: {count}')
    if fmt == FORMAT_NONE:  return 0
    if fmt == FORMAT_A:     return 1
    if fmt == FORMAT_AB:    return 2
    if fmt == FORMAT_ABC:   return 3
    if fmt == FORMAT_ABCD:  return 4
    if fmt == FORMAT_ABX:   return 2
    if fmt == FORMAT_ASBX:  return 2
    if fmt == FORMAT_SBX:   return 1
    raise ValueError(f"Unknown format: {fmt}")


def instruction_size(fmt: str) -> int:
    """Total byte size of an instruction with the given format."""
    return 1 + 2 * operand_count(fmt)


@dataclass
class Instruction:
    """A single VM instruction (un-encoded form for the compiler)."""
    op_name: str            # Semantic opcode name (e.g. 'ADD'); resolved to byte by OpcodeMap
    fmt: str                # Format string
    a: int = 0
    b: int = 0
    c: int = 0
    d: int = 0
    # Backpatch metadata
    pc: int = 0             # Byte offset where this instruction was emitted
    comment: str = ''       # For debugging output
    target: Optional[int] = None  # Symbolic original boundary, relocated at serialization
    components: tuple = ()       # Straight-line operations in a generated superinstruction

    def operand_values(self):
        if self.components:
            return [v for ins in self.components for v in ins.operand_values()]
        return [self.a,self.b,self.c,self.d][:operand_count(self.fmt)]


def encode_u16(value: int) -> List[int]:
    """Encode a 16-bit unsigned int as [lo, hi]."""
    if not 0 <= value <= 0xFFFF:
        raise ValueError(f'Unsigned VM operand out of range: {value}')
    return [value & 0xFF, (value >> 8) & 0xFF]


def encode_s16(value: int) -> List[int]:
    """Encode a signed 16-bit int with SBX_BIAS."""
    if not -32768 <= value <= 32767:
        raise ValueError(f'Signed VM jump out of range: {value}')
    return encode_u16(value + SBX_BIAS)


def encode_instruction(opcode_byte: int, fmt: str, a: int, b: int, c: int,
                       d: int = 0, *, layout=None, byte_order='little', signed_b=False,
                       values=None) -> List[int]:
    """Encode a single instruction to a flat byte list."""
    if not 0 < opcode_byte <= 255:
        raise ValueError(f'Invalid VM opcode byte: {opcode_byte}')
    count = operand_count(fmt)
    layout = tuple(range(count)) if layout is None else tuple(layout)
    if sorted(layout) != list(range(count)) or byte_order not in ('little','big'):
        raise ValueError('Invalid VM operand schema')
    out = [opcode_byte]
    operands = [a,b,c,d][:count] if values is None else values
    if len(operands) != count:
        raise ValueError('VM operand count does not match schema')
    for index in layout:
        value = operands[index]
        if (fmt == FORMAT_SBX and index == 0) or (fmt == FORMAT_ASBX and index == 1):
            encoded = encode_s16(value)
        elif signed_b and index == 1:
            if not -32768 <= value <= 32767:
                raise ValueError(f'Signed VM iterator jump out of range: {value}')
            encoded = encode_u16(value % 65536)
        else:
            encoded = encode_u16(value)
        out.extend(encoded if byte_order == 'little' else reversed(encoded))
    return out
