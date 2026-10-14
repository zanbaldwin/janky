return {
  id = "style",
  api = 1,
  filters = { styled = function(text) return text end },
  variables = {
    mode = { type = "enum", values = { "bold", "italic" }, required = true },
    size = { type = "int" },
    colours = { type = "list", of = { type = "string" } },
    box = { type = "object", schema = "box" },
    defs = { box = { width = { type = "int", required = true } } },
  },
}
