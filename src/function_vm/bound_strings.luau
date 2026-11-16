-- All key material still ships with the client. The outer pool contains ciphertext
-- that requires the prototype and literal index before returning the original text.
local charValue,concatValues=string.char,table.concat
local function decodeBound(data,expected,index,context)
    if #data<1 then fail() end
    local state=xorValue(context[__SEED__],context[__SALT__],context[__ENTRY__],(index*65599)%4294967296,0xA511E9B3)
    local step=context[__STEP__]
    local out=createTable(#data-1)
    local hash=0
    for i=1,#data do
        local cipher=byteValue(data,i)
        state=xorValue(state,shiftLeft(state,13))
        state=xorValue(state,shiftRight(state,17))
        state=xorValue(state,shiftLeft(state,5))
        local value=xorValue(cipher,andValue(state,255))
        state=(state+step+cipher+i)%4294967296
        if i==1 then
            if value~=index%256 then fail() end
        else
            out[i-1]=charValue(value)
            if __INTEGRITY__ then hash=(hash*65599+value)%2147483647 end
        end
    end
    if __INTEGRITY__ and hash~=expected then fail() end
    return concatValues(out)
end
