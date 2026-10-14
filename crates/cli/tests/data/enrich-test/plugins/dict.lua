local elements = {
  ["⁵⁶Fe"] = { name = "iron-56", form = { solid = "metal" } },
  ["¹²C"] = { name = "carbon-12", form = { solid = "graphite; diamond" } },
}

local function generate(job)
  local isotope = type(job.input) == "table" and job.input.isotope or job.input
  local entry = elements[isotope]
  if not entry then
    return { value = {}, info = isotope .. " is not in the table" }
  end
  if type(job.input) == "table" then
    local warning = nil
    if not job.input.phase then warning = isotope .. ": 2 table entries, set `phase`" end
    return { value = { form = entry.form[job.input.phase or "solid"] }, warning = warning }
  end
  return { value = { name = entry.name } }
end

return { id = "dict", api = 1, generates = "data", generate = generate }
