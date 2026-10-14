-- Template filters for elements: position, locator, superscript, shells and bohr.
-- The filters are pure and draw inline SVG, so the deck needs no image files.

local PERIOD_ENDS = { 2, 10, 18, 36, 54, 86, 118 }
local CORES = {
  He = "1s2",
  Ne = "[He]2s2 2p6",
  Ar = "[Ne]3s2 3p6",
  Kr = "[Ar]3d10 4s2 4p6",
  Xe = "[Kr]4d10 5s2 5p6",
  Rn = "[Xe]4f14 5d10 6s2 6p6",
}

local function fmt(x) return string.format("%.2f", x) end

local function place(n)
  local period, start = 1, 1
  for p, last in ipairs(PERIOD_ENDS) do
    if n <= last then period = p break end
    start = last + 1
  end
  if (n >= 57 and n <= 71) or (n >= 89 and n <= 103) then
    local first = n <= 71 and 57 or 89
    return { period = period, row = period + 3, col = 3 + n - first }
  end
  local k = n - start + 1
  local group
  if period == 1 then
    group = n == 1 and 1 or 18
  elseif period <= 3 then
    group = k <= 2 and k or k + 10
  elseif period <= 5 then
    group = k
  else
    group = k <= 2 and k or k - 14
  end
  return { period = period, group = group, row = period, col = group }
end

local function valid(n)
  return type(n) == "number" and n % 1 == 0 and n >= 1 and n <= 118
end

local function electrons(config, shells)
  local core = config:match("%[(%a+)%]")
  if core then
    if not CORES[core] then error("unknown noble gas core [" .. core .. "]", 0) end
    electrons(CORES[core], shells)
  end
  for n, _, count in config:gmatch("(%d)([spdf])(%d+)") do
    n = tonumber(n)
    shells[n] = (shells[n] or 0) + tonumber(count)
  end
  return shells
end

local function shell_list(config)
  local shells = electrons(config, {})
  local list = {}
  for n = 1, 7 do
    if shells[n] then list[n] = shells[n] else break end
  end
  return list
end

local filters = {}

function filters.position(n)
  if not valid(n) then return {} end
  local p = place(math.tointeger(n))
  return { period = p.period, group = p.group }
end

function filters.locator(n)
  if not valid(n) then return "" end
  n = math.tointeger(n)
  local size, step = 10, 11
  local out = {
    '<svg class="locator" viewBox="0 0 ' .. 18 * step .. " " .. fmt(9.5 * step)
      .. '" role="img" aria-label="Position in the periodic table">',
  }
  for i = 1, 118 do
    local p = place(i)
    local y = (p.row <= 7) and (p.row - 1) * step or (p.row - 1.5) * step
    out[#out + 1] = '<rect class="cell' .. (i == n and " on" or "") .. '" x="' .. (p.col - 1) * step
      .. '" y="' .. fmt(y) .. '" width="' .. size .. '" height="' .. size .. '" rx="1.5"/>'
  end
  out[#out + 1] = "</svg>"
  return janky.html(table.concat(out))
end

function filters.superscript(config)
  if type(config) ~= "string" or config == "" then return "" end
  local note = config:match("%((%a+)%)")
  local core = config:gsub("%s*%(%a+%)", ""):gsub("%]%s*", "] ")
  local html = janky.escape(core):gsub("(%d[spdf])(%d+)", "%1<sup>%2</sup>")
  if note then
    html = html .. ' <span class="note">(' .. janky.escape(note) .. ")</span>"
  end
  return janky.html(html)
end

function filters.shells(config)
  if type(config) ~= "string" or config == "" then return {} end
  return shell_list(config)
end

function filters.bohr(config, symbol)
  if type(config) ~= "string" or config == "" then return "" end
  local shells = shell_list(config)
  local nucleus, gap = 16, 14
  local half = nucleus + gap * #shells + 8
  local out = {
    '<svg class="bohr" viewBox="' .. -half .. " " .. -half .. " " .. 2 * half .. " " .. 2 * half
      .. '" role="img" aria-label="Bohr model">',
    '<circle class="nucleus" r="' .. nucleus .. '"/>',
    '<text class="nucleus-label" text-anchor="middle" dominant-baseline="central">'
      .. janky.escape(symbol or "") .. "</text>",
  }
  for k, count in ipairs(shells) do
    local r = nucleus + gap * k
    out[#out + 1] = '<circle class="shell" r="' .. r .. '"/>'
    for i = 0, count - 1 do
      local a = 2 * math.pi * i / count - math.pi / 2
      out[#out + 1] = '<circle class="electron" cx="' .. fmt(r * math.cos(a)) .. '" cy="'
        .. fmt(r * math.sin(a)) .. '" r="2.6"/>'
    end
  end
  out[#out + 1] = "</svg>"
  return janky.html(table.concat(out))
end

return { id = "atoms", api = 1, filters = filters }
