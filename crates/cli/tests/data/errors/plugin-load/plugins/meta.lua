return setmetatable({ [{}] = 1 }, { __index = function() while true do end end })
