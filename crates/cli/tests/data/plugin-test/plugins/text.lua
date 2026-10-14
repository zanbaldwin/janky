local filters = {}

function filters.shout(text)
  local ending = janky.variables.ending
  janky.variables.ending = "?"
  return string.upper(text) .. ending
end

function filters.wrap(text, tag)
  return janky.html("<" .. tag .. ">" .. janky.escape(text) .. "</" .. tag .. ">")
end

function filters.length(text)
  print("length of", text)
  return utf8.len(text)
end

return {
  id = "text",
  api = 1,
  filters = filters,
  variables = { ending = { type = "string", required = true } },
}
