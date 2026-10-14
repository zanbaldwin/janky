local filters = {}

function filters.crash(text)
  local parts = nil
  return parts[1] .. text
end

function filters.handle(text)
  return { fn = print }
end

function filters.spin(text)
  while true do end
end

local function never(entry)
  error("no cards for " .. entry)
end

return { id = "broken", api = 1, filters = filters, functions = { never = never } }
