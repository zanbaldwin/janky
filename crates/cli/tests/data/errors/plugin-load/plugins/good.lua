local function shout(text) return string.upper(text) end
return { id = "good", api = 1, filters = { shout = shout } }
