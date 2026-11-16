  return function(...)
    local r = createTable(parameterCount)
    for i=1,parameterCount do r[i]=selectCount(i,...) end
    local varargs = if not variadic or selectCount("#",...)<=parameterCount then emptyVarargs else packValues(selectCount(parameterCount+1,...))
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
