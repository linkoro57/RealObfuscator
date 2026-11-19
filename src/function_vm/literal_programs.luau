local rotateValue=bit32.lrotate
local charValue,concatValues=string.char,table.concat
local function newLiteralMemory(state)
    local memory=createTable(8)
    for i=1,8 do
        memory[i]=rotateValue(xorValue(state,(i*2654435769)%4294967296),i*3)
    end
    return memory
end
local function decodeBound(context,index,entry)
    local decode=newDecoder(context)
    local pc=entry
    local state,out,length,written,memory
    local budget=context[__INSTRUCTION_COUNT__]
    while pc~=0 and budget>0 do
        budget-=1
        local op,a,b,c,nextPc=decode(pc,state)
        -- Recipe handlers share semantics; normalize the alias-specific roles.
        local roles=op%6
        if roles==1 then b,c=c,b
        elseif roles==2 then a,b=b,a
        elseif roles==3 then a,b,c=b,c,a
        elseif roles==4 then a,b,c=c,a,b
        elseif roles==5 then a,c=c,a end
        pc=nextPc
        if __LITERAL_START__ then
            if state~=nil or b~=index then fail() end
            state=xorValue(a,context[__SEED__],context[__SALT__],(index*65599)%4294967296,entry)
            length=c
            if length>budget*4 then fail() end
            out=createTable(length)
            written=0
            memory=newLiteralMemory(state)
        elseif __LITERAL_XOR__ then
            if state==nil or b<1 or b>31 then fail() end
            state=(rotateValue(xorValue(state,a),b)+c)%4294967296
        elseif __LITERAL_ADD__ then
            if state==nil or b<1 or b>31 then fail() end
            state=xorValue(rotateValue((state+a)%4294967296,b),c)
        elseif __LITERAL_MIX__ then
            if state==nil or b<1 or b>31 then fail() end
            local slot=andValue(state,7)+1
            local previous=memory[slot]
            local value=xorValue(state,previous)
            local low=andValue(value,65535)
            local multiplier=andValue(a,65535)
            multiplier=multiplier-multiplier%2+1
            local product=(low*multiplier+shiftRight(a,16))%65536
            local round=low*65536+xorValue(shiftRight(value,16),product)
            state=xorValue(rotateValue(round,b),c)
            memory[slot]=xorValue((rotateValue(previous,11)+state)%4294967296,a)
        elseif __LITERAL_BYTES__ then
            if state==nil then fail() end
            local count=b%8
            local offset=(b-count)/8
            if count<1 or count>4 or offset+count>length then fail() end
            local word=xorValue(a,state)
            for i=1,count do
                if out[offset+i]~=nil then fail() end
                out[offset+i]=charValue(andValue(word,255))
                word=shiftRight(word,8)
            end
            written+=count
            state=(xorValue(rotateValue(state,9),a)+c)%4294967296
        elseif __LITERAL_RETURN__ then
            if state==nil or c~=index or a~=length or written~=length then fail() end
            local value=concatValues(out)
            if __INTEGRITY__ then
                local hash=0
                for i=1,#value do hash=(hash*65599+byteValue(value,i))%2147483647 end
                if hash~=b then fail() end
            end
            return value
        else
            fail()
        end
    end
    fail()
end
