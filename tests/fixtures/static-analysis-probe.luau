local settings = {
    endpoint = "https://example.invalid/private-endpoint",
    credential = "STATIC_PROBE_SECRET_20261119",
    label = "A deliberately long string to exercise literal fragmentation and reconstruction",
    binary = "\000\001\127\128\254\255",
}
local function emit(which)
    if which == 1 then
        return settings.endpoint, settings.credential
    end
    return settings.label, settings.binary
end
print(emit(1))
print(emit(2))
