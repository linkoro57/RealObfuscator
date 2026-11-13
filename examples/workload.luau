local function work(n)
    local sum=0
    for i=1,n do sum=(sum+i*17)%100003 end
    return sum
end
local start=os.clock()
local answer=0
for round=1,5 do answer+=work(100)end
print(answer)
print(os.clock()-start)
