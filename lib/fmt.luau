--!strict
-- Formatting helpers: ages, agent labels/icons, percent-encoding, snippet spans.

local fmt = {}

local MONTHS = {"Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"}

-- Compact age from a millisecond timestamp: now, 5m, 3h, 2d, then a calendar date.
function fmt.age(ms: number): string
	local now = os.time()
	local diff = now * 1000 - ms
	if diff < 60000 then
		return "now"
	elseif diff < 3600000 then
		return math.floor(diff / 60000) .. "m"
	elseif diff < 86400000 then
		return math.floor(diff / 3600000) .. "h"
	elseif diff < 604800000 then
		return math.floor(diff / 86400000) .. "d"
	end
	local t = os.date("*t", math.floor(ms / 1000)) :: any
	local this_year = (os.date("*t", now) :: any).year
	if t.year == this_year then return MONTHS[t.month] .. " " .. t.day end
	return MONTHS[t.month] .. " " .. t.year
end

-- Long relative time for sentences: "just now", "5 minutes ago", "on Mar 2".
function fmt.ago(ms: number): string
	local diff = os.time() * 1000 - ms
	if diff < 60000 then return "just now" end
	local function unit(n: number, word: string): string
		return tostring(n) .. " " .. word .. (if n == 1 then "" else "s") .. " ago"
	end
	if diff < 3600000 then return unit(math.floor(diff / 60000), "minute") end
	if diff < 86400000 then return unit(math.floor(diff / 3600000), "hour") end
	if diff < 604800000 then return unit(math.floor(diff / 86400000), "day") end
	return "on " .. fmt.age(ms)
end

function fmt.plural(n: number, one: string, many: string?): string
	return tostring(n) .. " " .. (if n == 1 then one else (many or (one .. "s")))
end

local home_dir: string? = nil
-- A path as people type it: the home directory as ~, macOS /private/{tmp,var} aliases unwrapped.
function fmt.path(p: string): string
	if home_dir == nil then home_dir = tern.getenv("HOME") or "" end
	local h = home_dir :: string
	local out = p
	for _, alias in {"/tmp", "/var"} do
		local real = "/private" .. alias
		if out == real or string.sub(out, 1, #real + 1) == real .. "/" then out = string.sub(out, 9); break end
	end
	if h ~= "" and h ~= "/" then
		if out == h then return "~" end
		if string.sub(out, 1, #h + 1) == h .. "/" then return "~" .. string.sub(out, #h + 1) end
	end
	return out
end

function fmt.basename(p: string): string
	local trimmed = string.gsub(p, "[/\\]+$", "")
	return string.match(trimmed, "([^/\\]+)$") or p
end

-- Agent display label.
function fmt.agent_label(agent: string): string
	if agent == "codex" then return "Codex"
	elseif agent == "claude" then return "Claude Code"
	elseif agent == "omp" then return "OMP"
	elseif agent == "pi" then return "Pi"
	elseif agent == "opencode" then return "OpenCode"
	elseif agent == "cursor" then return "Cursor"
	else return agent end
end

-- Agent icon name for ui.node icon prop.
function fmt.agent_icon(agent: string): string
	if agent == "codex" then return "app-codex"
	elseif agent == "claude" then return "app-claude"
	elseif agent == "omp" then return "app-omp"
	elseif agent == "pi" then return "pi-mark"
	elseif agent == "opencode" then return "terminal"
	elseif agent == "cursor" then return "cursor"
	else return "history" end
end

function fmt.agent_tone(agent: string, style: string?): string?
	if style == "monochrome" then return nil end
	local tones = { omp = "accent", claude = "warning", codex = "success", pi = "info", opencode = "user", cursor = "accent" }
	return (tones :: {[string]: string})[agent]
end

-- Percent-encode every byte outside [A-Za-z0-9-_.~].
function fmt.pct(s: string): string
	return (string.gsub(s, "[^A-Za-z0-9%-_.~]", function(c)
		return string.format("%%%02X", string.byte(c))
	end))
end

-- Percent-decode a string.
function fmt.unpct(s: string): string
	return (string.gsub(s, "%%(%x%x)", function(h)
		return string.char(tonumber(h, 16) :: number)
	end))
end

-- Backend stderr as a sentence: no "error:" prefix, trimmed, capitalized.
function fmt.error(err: string): string
	local msg = string.gsub((string.gsub(err, "^%s*[Ee]rror:%s*", "")), "%s+$", "")
	return string.upper(string.sub(msg, 1, 1)) .. string.sub(msg, 2)
end

-- Removes `prefix` (compared without highlight markers) from the start of a marked snippet.
local function strip_prefix(s: string, prefix: string): string
	local plain = string.gsub(s, "[\1\2]", "")
	local p = string.gsub(prefix, "%s+", " ")
	if p == "" or #plain <= #p or string.sub(plain, 1, #p) ~= p then return s end
	local out, seen, i = {}, 0, 1
	while i <= #s and seen < #p do
		local ch = string.sub(s, i, i)
		if ch == "\1" or ch == "\2" then table.insert(out, ch) else seen += 1 end
		i += 1
	end
	local rest = string.gsub(string.sub(s, i), "^[%s%p]*%s", "")
	-- Keep marker balance: an opened mark that was cut stays open into the rest.
	local text = table.concat(out) .. rest
	text = string.gsub(text, "\1\2", "")
	return if string.match(string.gsub(text, "[\1\2]", ""), "%S") then text else s
end

-- Parse snippet text (chars \1 … \2 mark highlighted spans) into a list of ui.span nodes.
-- `title` is dropped from the start of the snippet when the indexed body repeats it.
function fmt.snippet_spans(s: string, title: string?): { any }
	local ui = tern.ui
	s = string.gsub(s, "%s+", " ")
	if title then s = strip_prefix(s, title) end
	local parts: { any } = {}
	local i = 1
	local n = #s
	while i <= n do
		local a = string.find(s, "\1", i, true)
		if not a then
			-- No more marks; append the rest as plain muted text.
			local tail = string.sub(s, i)
			if tail ~= "" then
				table.insert(parts, ui.span(tail, "muted"))
			end
			break
		end
		-- Plain text before the mark.
		if a > i then
			table.insert(parts, ui.span(string.sub(s, i, a - 1), "muted"))
		end
		local b = string.find(s, "\2", a + 1, true)
		if not b then
			-- Unclosed mark; treat rest as highlighted.
			table.insert(parts, ui.span(string.sub(s, a + 1), "mark"))
			break
		end
		table.insert(parts, ui.span(string.sub(s, a + 1, b - 1), "mark"))
		i = b + 1
	end
	return parts
end

return fmt
