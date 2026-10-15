--!strict
-- tern-projects host half: ensure the data directory exists, then register the panel.

pcall(tern.fs.mkdir, tern.plugin.data)
tern.block.define("panel", require("./blocks/panel"))

require("./lib/cli").ensure(function(_path, err)
	if err then tern.log.error(err) end
end)
