-- Data generator: element facts from the PubChem periodic table.
-- `prepare` downloads the table once and stores one record per symbol in the cache.

local URL = "https://pubchem.ncbi.nlm.nih.gov/rest/pug/periodictable/JSON"
local FORMAT = 1

local FIELDS = {
  AtomicNumber = "number",
  AtomicMass = "mass",
  ElectronConfiguration = "config",
  Electronegativity = "electronegativity",
  StandardState = "state",
  MeltingPoint = "melting_point",
  BoilingPoint = "boiling_point",
  Density = "density",
  GroupBlock = "group_block",
  YearDiscovered = "discovered",
  CPKHexColor = "colour",
}

local function prepare(vars)
  local url = vars.url or URL
  local index = janky.cache.get("index")
  if type(index) == "table" and index.format == FORMAT and index.url == url then
    return
  end
  local response = janky.http.request { url = url, headers = { accept = "application/json" } }
  if response.status ~= 200 then
    error("PubChem HTTP " .. response.status .. ": " .. response.body:sub(1, 200), 0)
  end
  local table_ = janky.json.decode(response.body).Table
  local columns = table_.Columns.Column
  local count = 0
  for _, row in ipairs(table_.Row) do
    local record = {}
    for i, column in ipairs(columns) do
      local key, value = FIELDS[column], row.Cell[i]
      if key and value ~= nil and value ~= "" then
        record[key] = value
      end
    end
    local symbol = row.Cell[2]
    if symbol and symbol ~= "" then
      janky.cache.put("element:" .. symbol, record)
      count = count + 1
    end
  end
  if count == 0 then error("the PubChem table has no elements", 0) end
  janky.cache.put("index", { format = FORMAT, url = url, count = count })
  janky.log("indexed " .. count .. " elements")
end

local function plan(job, vars)
  return { key = { url = vars.url or URL, v = FORMAT }, units = 1, unit = "lookups" }
end

local function generate(job, vars)
  if type(job.input) ~= "string" or not job.input:match("^%u%l?%l?$") then
    error("the input must be an element symbol, for example `Mo`", 0)
  end
  local record = janky.cache.get("element:" .. job.input)
  if type(record) ~= "table" then
    return { value = {}, warning = "not in the PubChem periodic table" }
  end
  record.number = tonumber(record.number)
  return { value = record }
end

return {
  id = "pubchem",
  api = 1,
  generates = "data",
  variables = {
    url = { type = "string" },
  },
  plan = plan,
  prepare = prepare,
  generate = generate,
}
