--!strict
-- Formatting helpers: ages, agent labels/icons, percent-encoding, snippet spans.

local fmt = {}

-- Human-readable age from a millisecond timestamp.
function fmt.age(ms: number): string
	local now_ms = os.time() * 1000
	local diff = now_ms - ms
	if diff < 60000 then
		return "now"
	elseif diff < 3600000 then
		return math.floor(diff / 60000) .. "m"
	elseif diff < 86400000 then
		return math.floor(diff / 3600000) .. "h"
	elseif diff < 604800000 then
		return math.floor(diff / 86400000) .. "d"
	elseif diff < 31536000000 then
		return math.floor(diff / 604800000) .. "w"
	else
		return math.floor(diff / 31536000000) .. "y"
	end
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

-- Parse snippet text (chars \1 … \2 mark highlighted spans) into a list of ui.span nodes.
-- Returns a list of mixed strings and ui.span nodes suitable for use as item `detail`.
function fmt.snippet_spans(s: string): { any }
	local ui = tern.ui
	s = string.gsub(s, "%s+", " ")
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
