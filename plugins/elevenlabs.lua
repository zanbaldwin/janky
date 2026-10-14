-- Text to speech with ElevenLabs: POST /v1/text-to-speech/{voice_id}.

local function extension(format)
  if format:sub(1, 5) == "opus_" then return "ogg" end
  return "mp3"
end

local MP3_BITRATES = {
  [1] = { 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320 },
  [2] = { 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160 },
}
local MP3_RATES = {
  [1] = { 44100, 48000, 32000 },
  [2] = { 22050, 24000, 16000 },
  [3] = { 11025, 12000, 8000 },
}

local function u32(data, at)
  local a, b, c, d = data:byte(at, at + 3)
  return ((a << 24) | (b << 16) | (c << 8) | d)
end

local function be32(n)
  return string.char((n >> 24) & 0xFF, (n >> 16) & 0xFF, (n >> 8) & 0xFF, n & 0xFF)
end

local function mp3_append_silence(data, seconds)
  local first = 1
  if data:sub(1, 3) == "ID3" then
    local _, _, _, _, _, flags, s1, s2, s3, s4 = data:byte(1, 10)
    first = 11 + ((s1 << 21) | (s2 << 14) | (s3 << 7) | s4) + ((flags & 0x10 ~= 0) and 10 or 0)
  end
  local h1, h2, h3, h4 = data:byte(first, first + 3)
  if not h4 or h1 ~= 0xFF or (h2 & 0xE0) ~= 0xE0 or (h2 & 0x06) ~= 0x02 then
    error("experimental_trailing_silence: the response is not an MPEG Layer III stream", 0)
  end
  local version = ({ [3] = 1, [2] = 2, [0] = 3 })[(h2 >> 3) & 0x03]
  local table_version = version == 1 and 1 or 2
  local kbps = MP3_BITRATES[table_version][(h3 >> 4) & 0x0F]
  local rate = version and MP3_RATES[version][((h3 >> 2) & 0x03) + 1]
  if not (version and kbps and rate) then
    error("experimental_trailing_silence: unsupported MP3 frame header", 0)
  end
  local mono = (h4 >> 6) == 3
  local samples = version == 1 and 1152 or 576
  local size = (samples // 8) * kbps * 1000 // rate
  local side = version == 1 and (mono and 17 or 32) or (mono and 9 or 17)
  local header = string.char(h1, h2 | 0x01, h3 & 0xFD, h4)
  local frame = header .. string.rep("\0", size - 4)
  local count = math.ceil(seconds * rate / samples)

  local tail = ""
  if #data >= 128 and data:sub(-128, -126) == "TAG" then
    data, tail = data:sub(1, -129), data:sub(-128)
  end
  local tag = first + 4 + ((h2 & 0x01) == 0 and 2 or 0) + side
  local id = data:sub(tag, tag + 3)
  if id == "Xing" or id == "Info" then
    local fields = u32(data, tag + 4)
    local at = tag + 8
    if fields & 1 ~= 0 then
      data = data:sub(1, at - 1) .. be32(u32(data, at) + count) .. data:sub(at + 4)
      at = at + 4
    end
    if fields & 2 ~= 0 then
      data = data:sub(1, at - 1) .. be32(u32(data, at) + count * size) .. data:sub(at + 4)
    end
  end
  return data .. string.rep(frame, count) .. tail
end

local function plan(job, vars)
  if not vars.voice_id:match("^%w+$") then
    error("voice_id must have only letters and digits", 0)
  end
  local silence = vars.experimental_trailing_silence
  if silence ~= nil then
    if extension(vars.output_format) ~= "mp3" then
      error("experimental_trailing_silence works only with an mp3_* output_format", 0)
    end
    if silence <= 0 or silence > 3 then
      error("experimental_trailing_silence must be more than 0 and at most 3 seconds", 0)
    end
  end
  return {
    ext = extension(vars.output_format),
    key = {
      model = vars.model,
      voice = vars.voice_id,
      language = vars.language,
      output_format = vars.output_format,
      voice_settings = vars.voice_settings,
      experimental_trailing_silence = silence,
    },
    units = utf8.len(job.input),
    unit = "characters",
  }
end

local function detail(body)
  local ok, value = pcall(janky.json.decode, body)
  if ok and type(value) == "table" and type(value.detail) == "table" then
    local detail = value.detail
    local text = detail.message or detail.status or (type(detail[1]) == "table" and detail[1].msg)
    if text then
      return tostring(text)
    end
  end
  return body:sub(1, 200)
end

local function generate(job, vars)
  local response = janky.http.request {
    method = "POST",
    url = "https://api.elevenlabs.io/v1/text-to-speech/" .. vars.voice_id .. "?output_format=" .. vars.output_format,
    headers = {
      ["xi-api-key"] = janky.secret("api_key"),
      ["content-type"] = "application/json",
      accept = "audio/*",
    },
    body = janky.json.encode {
      text = job.input,
      model_id = vars.model,
      language_code = vars.language,
      voice_settings = (vars.voice_settings and next(vars.voice_settings)) and vars.voice_settings or nil,
    },
  }
  if response.status ~= 200 then
    return nil, "ElevenLabs HTTP " .. response.status .. ": " .. detail(response.body)
  end
  local data = response.body
  if vars.experimental_trailing_silence then
    data = mp3_append_silence(data, vars.experimental_trailing_silence)
  end
  return {
    data = data,
    ext = extension(vars.output_format),
    cost = tonumber(response.headers["character-cost"]),
  }
end

return {
  id = "elevenlabs",
  api = 1,
  generates = "media",
  secrets = { "api_key" },
  variables = {
    voice_id = { type = "string", required = true },
    model = { type = "string", required = true },
    language = { type = "string" },
    output_format = {
      type = "enum",
      required = true,
      values = {
        "opus_48000_32", "opus_48000_64", "opus_48000_96", "opus_48000_128", "opus_48000_192",
        "mp3_44100_64", "mp3_44100_96", "mp3_44100_128", "mp3_44100_192",
      },
    },
    voice_settings = {
      type = "object",
      fields = { stability = { type = "float" }, similarity_boost = { type = "float" } },
    },
    experimental_trailing_silence = { type = "float" },
  },
  generate = generate,
  plan = plan,
}
