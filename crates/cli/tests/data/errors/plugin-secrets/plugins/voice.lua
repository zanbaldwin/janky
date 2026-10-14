return {
  id = "voice",
  api = 1,
  generates = "media",
  secrets = { "api_key", "token" },
  generate = function(job) return { data = job.input, ext = "txt" } end,
  plan = function() return { ext = "txt" } end,
}
