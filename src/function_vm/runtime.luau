local byteValue, xorValue, packValues, unpackValues, formatValues, fail = string.byte, bit32.bxor, table.pack, table.unpack, string.format, error
local shiftLeft, shiftRight, andValue = bit32.lshift, bit32.rshift, bit32.band
local rotateLeft, rotateRight = bit32.lrotate, bit32.rrotate
local createTable, nextValue, selectCount = table.create, next, select
local emptyVarargs = {n=0}
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
    local function decode(index,recipeState)
        if index < 1 or index > instructionCount or index % 1 ~= 0 then fail() end
        local first = if __PACKED__ then offsets[index] else 1
        local last = if __PACKED__ then offsets[index + 1] - 1 else #payload[index]
        local rawCount = last - first + 1
        if rawCount < 1 or rawCount > 25 then fail() end
        if raw == nil then raw=createTable(25);bytes=createTable(24) end
        local key = xorValue(seed,index,recipeState or 0)
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
        __WORD_DECODER__
    end
    return decode
end
__STRING_DECODER__
local function __RUNNER__(prototype, external, parameterCount, variadic, bridgedIteration)
__INVOCATION__
end
