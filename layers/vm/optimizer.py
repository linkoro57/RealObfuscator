"""Preserve symbolic boundaries while fusing safe, straight-line operations."""
from .instruction import Instruction, instruction_size


def relocate_and_fuse(proto, opcodes, rng, *, fuse=False, extended=False, blocks=False):
    instructions = proto.instructions
    if not instructions:
        return
    old_end = instructions[-1].pc + instruction_size(instructions[-1].fmt)
    boundaries = {ins.pc for ins in instructions} | {old_end}
    protected = set()
    targets = set()
    branches = {'JMP':'a','FORPREP':'b','FORLOOP':'b','TFORLOOP':'b'}
    skip_ops = {'EQ','LT','LE','TEST','TESTSET'}
    for index,ins in enumerate(instructions):
        if ins.op_name in branches:
            ins.target = ins.pc + instruction_size(ins.fmt) + getattr(ins,branches[ins.op_name])
            if ins.target not in boundaries:
                raise ValueError(f'VM branch targets a non-instruction boundary: {ins.target}')
            targets.add(ins.target)
        if ins.op_name in skip_ops or (ins.op_name == 'LOADBOOL' and ins.c):
            if index+1 >= len(instructions):
                raise ValueError('VM conditional has no instruction to skip')
            protected.add(instructions[index+1].pc)
            # The destination after a skipped instruction must stay an entry.
            targets.add(instructions[index+2].pc if index+2 < len(instructions) else old_end)
        if ins.op_name == 'CLOSURE':
            if not 0 <= ins.b < len(proto.sub_protos):
                raise ValueError('VM closure references an invalid function')
            count = len(proto.sub_protos[ins.b].upvalues)
            records = instructions[index+1:index+1+count]
            if len(records) != count or any(r.op_name not in ('MOVE','GETUPVAL') for r in records):
                raise ValueError('VM closure has invalid upvalue link records')
            protected.update(r.pc for r in records)
    if targets & protected:
        # Skip-next operands are executable; branches may legitimately target
        # them. Only closure records must never have independent entry points.
        closure_records = set()
        for index,ins in enumerate(instructions):
            if ins.op_name == 'CLOSURE':
                count = len(proto.sub_protos[ins.b].upvalues)
                closure_records.update(r.pc for r in instructions[index+1:index+1+count])
        if targets & closure_records:
            raise ValueError('VM branch enters a closure link record')
    fused = []
    index = 0
    operations = {'ADD','SUB','MUL','DIV'} | ({'MOD','POW'} if extended else set())
    while index < len(instructions):
        first = instructions[index]
        second = instructions[index+1] if index+1 < len(instructions) else None
        eligible = (fuse and first.op_name == 'LOADK' and second is not None
                    and second.op_name in operations
                    and first.pc not in protected and second.pc not in protected
                    and second.pc not in targets)
        side = None
        if eligible:
            if second.c == first.a:
                side = 'R'
            elif extended and second.b == first.a:
                side = 'L'
        if side is not None and rng.randrange(4) != 0:
            # C keeps the loaded register: preserve that intermediate write,
            # including when it aliases the other arithmetic operand.
            other = second.b if side == 'R' else second.c
            name = 'F'+second.op_name+side
            fused.append(Instruction(op_name=name,fmt=opcodes.fmt_of(name),
                                     a=second.a,b=other,c=first.a,d=first.b,pc=first.pc))
            index += 2
        else:
            fused.append(first)
            index += 1
    if blocks:
        # Preserve every entry/skip/link boundary. Inline adjacent operations
        # verbatim, including intermediate register writes and calls. Branches,
        # returns and closure link records stay separate; no opaque work is
        # inserted into loops. Bound block lengths and generated handler count.
        barriers = set(branches) | skip_ops | {'LOADBOOL','RETURN','TAILCALL','CLOSURE','TFORPREP'}
        grouped = []
        index = 0
        while index < len(fused):
            first = fused[index]
            group = []
            limit = rng.randint(2,4)
            for candidate in fused[index:index+limit]:
                if candidate.op_name in barriers or candidate.pc in protected:
                    break
                if group and candidate.pc in targets:
                    break
                group.append(candidate)
            info = opcodes.block(ins.op_name for ins in group) if len(group)>1 else None
            if info is not None:
                grouped.append(Instruction(op_name=info.name,fmt=info.fmt,pc=first.pc,
                                           components=tuple(group)))
                index += len(group)
            else:
                grouped.append(first)
                index += 1
        fused = grouped
    positions = {}
    pc = 0
    for ins in fused:
        positions[ins.pc] = pc
        pc += instruction_size(ins.fmt)
    positions[old_end] = pc
    for ins in fused:
        original_pc = ins.pc
        ins.pc = positions[original_pc]
        if ins.target is not None:
            if ins.target not in positions:
                raise ValueError('VM transformation removed a branch destination')
            setattr(ins,branches[ins.op_name],positions[ins.target]-ins.pc-instruction_size(ins.fmt))
    proto.instructions = fused
