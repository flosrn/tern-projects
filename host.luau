--!strict
-- tern-projects host half: ensure the data directory exists, then register the panel.

pcall(tern.fs.mkdir, tern.plugin.data)
tern.block.define("panel", require("./blocks/panel"))
