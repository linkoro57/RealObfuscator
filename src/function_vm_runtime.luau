local byteValue, xorValue, packValues, unpackValues, formatValues, fail = string.byte, bit32.bxor, table.pack, table.unpack, string.format, error
local shiftLeft, shiftRight, andValue = bit32.lshift, bit32.rshift, bit32.band
local createTable, nextValue = table.create, next
local function __RUNNER__(prototype, external, arguments, ...)
    local varargs = packValues(...)
    local r = arguments
    local payload = prototype[__S1__]
    local offsets = prototype[__S12__]
    local layout = prototype[__S5__]
    local function decode(index)
        if index < 1 or index > prototype[__S7__] or index % 1 ~= 0 then fail() end
        local first = if __PACKED__ then offsets[index] else 1
        local last = if __PACKED__ then offsets[index + 1] - 1 else #payload[index]
        local key = xorValue(prototype[__S2__], prototype[__S4__], index)
        local raw = {}
        for offset = first, last do
            local cipher = if __PACKED__ then byteValue(payload, offset) else payload[index][offset]
            key = xorValue(key, shiftLeft(key,13))
            key = xorValue(key, shiftRight(key,17))
            key = xorValue(key, shiftLeft(key,5))
            local value = xorValue(cipher, andValue(key,255))
            raw[#raw + 1] = value
            key = (key + prototype[__S3__] + cipher + offset - first + 1) % 4294967296
        end
        local bytes = {}
        if raw[1] == 0 then
            for i = 2, #raw do bytes[#bytes + 1] = raw[i] end
        elseif raw[1] == 1 then
            local i = 2
            while i <= #raw do
                if raw[i] == 255 then
                    local count, value = raw[i + 1], raw[i + 2]
                    if count == nil or count == 0 or value == nil or #bytes + count > 24 then fail() end
                    for _ = 1, count do bytes[#bytes + 1] = value end
                    i += 3
                else
                    bytes[#bytes + 1] = raw[i]
                    i += 1
                end
                if #bytes > 24 then fail() end
            end
        else
            fail()
        end
        if #bytes ~= 24 then fail() end
        local function word(offset)
            return bytes[offset] + bytes[offset + 1] * 256 + bytes[offset + 2] * 65536 + bytes[offset + 3] * 16777216
        end
        if __INTEGRITY__ then
            local hash = index
            for i = 1, 20 do hash = (hash * 257 + bytes[i]) % 2147483647 end
            if hash ~= word(21) then fail() end
        end
        return {word(layout[1]), word(layout[2]), word(layout[3]), word(layout[4]), word(layout[5])}
    end
    local records = prototype[__S11__]
    if __CACHED__ and records == nil then
        records = {}
        for i = 1, prototype[__S7__] do records[i] = decode(i) end
        prototype[__S11__] = records
    end
    local known = if __EPHEMERAL__ then {} else prototype[__S10__]
    local values = if __EPHEMERAL__ then {} else prototype[__S9__]
    local function constant(index)
        if not known[index] then
            values[index] = prototype[__S8__][index]()
            known[index] = true
        end
        return values[index]
    end
    local function execute(pc)
      while pc ~= 0 do
        local instruction = if __CACHED__ then records[pc] else decode(pc)
        if instruction == nil then fail() end
        local op, a, b, c = instruction[1], instruction[2], instruction[3], instruction[4]
        pc = instruction[5]
        __HANDLERS__
      end
      return 0,nil
    end
    local signal,result = execute(prototype[__S6__])
    if signal == 2 then return unpackValues(result,1,result.n) end
end
