local function plan(job)
  local missing = nil
  return { ext = missing.ext }
end

return { id = "broken", api = 1, generates = "media", generate = function() end, plan = plan }
