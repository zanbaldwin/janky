local function pair(first, second)
  return { first = first, second = second, both = { first, second } }
end

return { id = "word", api = 1, functions = { pair = pair } }
