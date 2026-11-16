local byteValue, xorValue, packValues, unpackValues, formatValues, fail = string.byte, bit32.bxor, table.pack, table.unpack, string.format, error
local shiftLeft, shiftRight, andValue = bit32.lshift, bit32.rshift, bit32.band
local createTable, nextValue, selectCount = table.create, next, select
local emptyVarargs = {n=0}
__STRING_DECODER__
-- Decoder state is constructed only when records actually need decoding.
local function newDecoder(prototype)
    local payload = prototype[__PAYLOAD__]
    local offsets = prototype[__OFFSETS__]
    local layout = prototype[__LAYOUT__]
    local slot1,slot2,slot3,slot4,slot5 = layout[1],layout[2],layout[3],layout[4],layout[5]
    local instructionCount = prototype[__INSTRUCTION_COUNT__]
    local seed = xorValue(prototype[__SEED__],prototype[__SALT__])
    local step = prototype[__STEP__]
    -- Scratch belongs to this invocation, is allocated only on first decode,
    -- and never escapes. Recursive execute calls consume scalar operands first.
    local raw,bytes
    local function word(offset)
        return bytes[offset] + bytes[offset + 1] * 256 + bytes[offset + 2] * 65536 + bytes[offset + 3] * 16777216
    end
    local function decode(index)
        if index < 1 or index > instructionCount or index % 1 ~= 0 then fail() end
        local first = if __PACKED__ then offsets[index] else 1
        local last = if __PACKED__ then offsets[index + 1] - 1 else #payload[index]
        local rawCount = last - first + 1
        if rawCount < 1 or rawCount > 25 then fail() end
        if raw == nil then raw=createTable(25);bytes=createTable(24) end
        local key = xorValue(seed,index)
        for offset = first, last do
            local cipher = if __PACKED__ then byteValue(payload, offset) else payload[index][offset]
            key = xorValue(key, shiftLeft(key,13))
            key = xorValue(key, shiftRight(key,17))
            key = xorValue(key, shiftLeft(key,5))
            raw[offset - first + 1] = xorValue(cipher, andValue(key,255))
            key = (key + step + cipher + offset - first + 1) % 4294967296
        end
        local length = 0
        if raw[1] == 0 then
            length=rawCount-1
            for i = 2, rawCount do bytes[i-1] = raw[i] end
        elseif raw[1] == 1 then
            local i = 2
            while i <= rawCount do
                if raw[i] == 255 then
                    -- Bounds use this record's length, never stale scratch slots.
                    if i + 2 > rawCount then fail() end
                    local count,value = raw[i+1],raw[i+2]
                    if count == 0 or length + count > 24 then fail() end
                    for _ = 1, count do length+=1;bytes[length]=value end
                    i += 3
                else
                    length+=1
                    if length > 24 then fail() end
                    bytes[length]=raw[i]
                    i += 1
                end
            end
        else
            fail()
        end
        if length ~= 24 then fail() end
        if __INTEGRITY__ then
            local hash = index
            for i = 1, 20 do hash = (hash * 257 + bytes[i]) % 2147483647 end
            if hash ~= word(21) then fail() end
        end
        return word(slot1),word(slot2),word(slot3),word(slot4),word(slot5)
    end
    return decode
end
local function __RUNNER__(prototype, external, arguments, ...)
    local varargs = if selectCount("#",...)==0 then emptyVarargs else packValues(...)
    local r = arguments
    local records = prototype[__RECORDS__]
    local decode
    if __CACHED__ then
        if records == nil then
            local initialize=newDecoder(prototype)
            local count=prototype[__INSTRUCTION_COUNT__]
            records=createTable(count)
            for i=1,count do records[i]={initialize(i)} end
            prototype[__RECORDS__]=records
        end
    else
        decode=newDecoder(prototype)
    end
    -- A prototype-owned sentinel distinguishes cached nil from a cache miss.
    -- Maximum hardening still keeps plaintext values invocation-local.
    local nilValue = prototype[__CONSTANT_KNOWN__]
    local values = if __EPHEMERAL__ then {} else prototype[__CONSTANT_VALUES__]
    local factories=prototype[__CONSTANTS__]
    local entry=prototype[__ENTRY__]
    __EXECUTE_BEGIN__
      while pc ~= 0 do
        local op,a,b,c
        if __CACHED__ then
            local instruction=records[pc]
            if instruction==nil then fail() end
            op,a,b,c=instruction[1],instruction[2],instruction[3],instruction[4]
            pc=instruction[5]
        else
            op,a,b,c,pc=decode(pc)
        end
        __HANDLERS__
      end
    __EXECUTE_END__
end
