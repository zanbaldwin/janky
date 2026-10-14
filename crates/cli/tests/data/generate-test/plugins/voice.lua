local function plan(job, vars)
  return { ext = "ogg", key = { vars.voice, vars.speed }, units = utf8.len(job.input), unit = "characters" }
end

local function generate(job, vars)
  return { data = job.input, ext = "ogg" }
end

return {
  id = "voice",
  api = 1,
  generates = "media",
  variables = { speed = { type = "int" }, voice = { type = "string", required = true } },
  generate = generate,
  plan = plan,
}
