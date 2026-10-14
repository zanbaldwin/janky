local s = {}
for i = 1, 40 do s[i] = "s" .. i end
return { id = "many", api = 1, secrets = s }
