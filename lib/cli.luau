--!strict
-- Helpers for spawning tern-projects and decoding its JSON output.

local cli = {}

-- Path to the tern-projects binary, or nil when not found.
function cli.bin(): string?
	local plugin_bin = tern.plugin.dir .. "/bin/tern-projects"
	if tern.fs.exists(plugin_bin) then
		return plugin_bin
	end
	local home = tern.getenv("HOME") or ""
	local cargo_bin = home .. "/.cargo/bin/tern-projects"
	if tern.fs.exists(cargo_bin) then
		return cargo_bin
	end
	return nil
end

-- Path to the index SQLite DB.
function cli.db(): string
	return tern.plugin.data .. "/index.sqlite"
end

-- Spawn the binary with `args` prepended by `{bin, "--db", db}`.
-- Decodes stdout as JSON and calls cb(data, nil) on success,
-- or cb(nil, errmsg) on non-zero exit, decode failure, or timeout.
local function spawn(args: { string }, timeout_ms: number, stdin: string?, cb: (data: any?, err: string?) -> ())
	local b = cli.bin()
	if not b then
		cb(nil, "tern-projects binary not found — run scripts/build.sh in " .. tern.plugin.dir)
		return
	end
	local argv: { string } = { b, "--db", cli.db() }
	for _, a in args do
		table.insert(argv, a)
	end
	;(tern :: any).process.run(argv, { timeout_ms = timeout_ms, stdin = stdin }, function(result: any, _cx: any?)
		if result.timed_out then
			cb(nil, "timed out")
			return
		end
		if result.status ~= 0 then
			local msg = result.stderr ~= "" and result.stderr or result.stdout
			cb(nil, msg ~= "" and msg or ("exit " .. tostring(result.status)))
			return
		end
		local ok, decoded = pcall(tern.json.decode, result.stdout)
		if not ok then
			cb(nil, "JSON decode error: " .. tostring(decoded))
			return
		end
		cb(decoded, nil)
	end)
end

function cli.run(args: { string }, timeout_ms: number, cb: (data: any?, err: string?) -> ())
	spawn(args, timeout_ms, nil, cb)
end

function cli.request(payload: any, cb: (data: any?, err: string?) -> ())
	local ok, encoded = pcall(tern.json.encode, payload)
	if not ok then cb(nil, tostring(encoded)); return end
	local timeouts = {
		harnesses = 60000,
		["launch-plan"] = 60000,
		["delete-preview"] = 120000,
		delete = 180000,
	}
	spawn({ "library" }, timeouts[payload.op] or 15000, encoded, function(data, err)
		if payload.op == "delete" and err == "timed out" then
			err = "Deletion timed out. The original chat may already have been removed; refresh before obtaining a new deletion preview."
		end
		cb(data, err)
	end)
end

-- Convenience timeout constants (ms).
cli.TIMEOUT_INDEX = 180000
cli.TIMEOUT_QUERY = 15000

return cli
