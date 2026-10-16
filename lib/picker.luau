--!strict
-- A keyboard-driven model for Tern's native `picker` sheet. The panel owns one at a time.
local tf = require("./textfield")
local input = require("./input")
local ui = tern.ui
local picker = {}

export type Item = {
	id: string,
	label: string,
	detail: string?,
	icon: string?,
	tone: string?,
	mono: boolean?,
	badges: {any}?,
	disabled: (boolean | string)?,
	group: string?, -- group label; consecutive items with the same group share a header
	title: string?,
	data: any?,
	facts: {[string]: any}?,
}
export type Action = {id: string, label: string, keys: {string}?, primary: boolean?, danger: boolean?, disabled: (boolean | string)?, ["end"]: boolean?}
export type Picker = {
	kind: string,
	title: string,
	subtitle: string?,
	icon: string?,
	noun: string,
	placeholder: string?,
	items: {Item},
	query: tf.Field?,
	query_all: boolean,
	filter: boolean, -- false: the query is not a filter (folder path entry)
	selected: string?,
	current: {string},
	state: string,
	message: string?,
	empty: string?,
	actions: {Action},
	total: number?,
	columns: {any}?,
	layout: string?,
	busy_label: string?,
	data: any,
}

function picker.new(spec: any): Picker
	local p: Picker = {
		kind = spec.kind,
		title = spec.title,
		subtitle = spec.subtitle,
		icon = spec.icon,
		noun = spec.noun or "items",
		placeholder = spec.placeholder,
		items = spec.items or {},
		query = if spec.search then tf.new(spec.query or "") else nil,
		query_all = false,
		filter = spec.filter ~= false,
		selected = spec.selected,
		current = spec.current or {},
		state = spec.state or "ready",
		message = spec.message,
		empty = spec.empty,
		actions = spec.actions or {},
		total = spec.total,
		data = spec.data,
		columns = spec.columns,
		layout = spec.layout,
		busy_label = spec.busy_label,
	}
	picker.settle(p)
	return p
end

local function lower(s: string): string return string.lower(s) end

-- Items that match the query, in catalog order.
function picker.visible(p: Picker): {Item}
	local q = if p.query and p.filter then lower((string.gsub(p.query.text, "^%s+", ""))) else ""
	q = string.gsub(q, "%s+$", "")
	if q == "" then return p.items end
	local out = {}
	for _, it in p.items do
		if string.find(lower(it.label), q, 1, true) or (it.detail and string.find(lower(it.detail), q, 1, true)) then table.insert(out, it) end
	end
	return out
end

local function enabled(it: Item): boolean return not it.disabled end

-- Keep the selection on a visible item, preferring the first enabled one.
function picker.settle(p: Picker)
	local vis = picker.visible(p)
	for _, it in vis do if it.id == p.selected then return end end
	p.selected = nil
	for _, it in vis do if enabled(it) then p.selected = it.id; return end end
	if vis[1] then p.selected = vis[1].id end
end

function picker.selected_item(p: Picker): Item?
	for _, it in picker.visible(p) do if it.id == p.selected then return it end end
	return nil
end

function picker.move(p: Picker, dir: number, count: number?)
	local vis = picker.visible(p)
	if #vis == 0 then return end
	local idx = 0
	for i, it in vis do if it.id == p.selected then idx = i end end
	for _ = 1, count or 1 do
		local next = idx + dir
		if next < 1 or next > #vis then break end
		idx = next
	end
	p.selected = vis[math.max(1, idx)].id
end

function picker.primary(p: Picker): Action?
	for _, a in p.actions do if a.primary then return a end end
	return nil
end

-- Match keyboard availability to the native action bar.
local function actionable(p: Picker, a: Action): boolean
	if a.disabled or p.state == "loading" then return false end
	if a.primary and p.filter then
		local selected = picker.selected_item(p)
		if not selected or selected.disabled then return false end
	end
	return true
end

-- Returns handled, then an intent: an action id, "query" when the query text changed, or nil.
function picker.key(p: Picker, k: Key): (boolean, string?)
	local name = k.name
	if name == "escape" then return true, "close" end
	if name == "up" or name == "down" then picker.move(p, if name == "up" then -1 else 1); return true, nil end
	if name == "page_up" or name == "page_down" then picker.move(p, if name == "page_up" then -1 else 1, 8); return true, nil end
	if name == "home" and not p.query then local v = picker.visible(p); if v[1] then p.selected = v[1].id end; return true, nil end
	if name == "end" and not p.query then local v = picker.visible(p); if #v > 0 then p.selected = v[#v].id end; return true, nil end
	if name == "enter" then
		if k.meta or k.ctrl then
			for _, a in p.actions do
				for _, key in a.keys or {} do
					if ((key == "cmd+enter" and k.meta) or (key == "ctrl+enter" and k.ctrl and not k.meta)) then
						return true, if actionable(p, a) then a.id else nil
					end
				end
			end
		end
		local a = picker.primary(p)
		return true, if a and actionable(p, a) then a.id else nil
	end
	-- Action shortcuts named with a single modifier chord, e.g. "cmd+backspace".
	for _, a in p.actions do
		for _, key in a.keys or {} do
			local mod, base = string.match(key, "^(%a+)%+(.+)$")
			if base == name and ((mod == "cmd" and k.meta) or (mod == "ctrl" and k.ctrl and not k.meta)) and not (mod == "cmd" and base == "enter") then return true, if actionable(p, a) then a.id else nil end
		end
	end
	if p.query then
		local before = p.query.text
		local handled, all = input.key(p.query, p.query_all, k, tern.runtime.os)
		p.query_all = all
		if p.query.text ~= before then
			if p.filter then picker.settle(p) end
			return true, "query"
		end
		return handled or true, nil
	end
	return true, nil
end

-- The native sheet. `tone_of` maps an item to its icon tone (harness colors), when given.
function picker.view(p: Picker, busy: boolean?): any
	local vis = picker.visible(p)
	local items, order = {}, {}
	local group: string? = nil
	for _, it in vis do
		if it.group and it.group ~= group then
			table.insert(order, {group = "g:" .. it.group, label = it.group})
		end
		group = it.group
		table.insert(items, {
			id = it.id,
			label = it.label,
			detail = it.detail,
			icon = it.icon,
			tone = it.tone,
			mono = it.mono,
			badges = it.badges,
			disabled = it.disabled,
			title = it.title,
			facts = it.facts,
		})
		table.insert(order, it.id)
	end
	local actions = {}
	for _, a in p.actions do
		local disabled = a.disabled
		if busy and not disabled and a.id ~= "close" then disabled = true end
		if a.primary and not disabled then
			local sel = picker.selected_item(p)
			if p.filter and (not sel or sel.disabled) then disabled = true end
		end
		local label = if busy and a.id == "close" then "Hide" elseif busy and a.primary and p.busy_label then p.busy_label else a.label
		table.insert(actions, {id = a.id, label = label, keys = a.keys, primary = a.primary, danger = a.danger, ["end"] = a["end"], disabled = disabled})
	end
	local props: any = {
		key = "picker:" .. p.kind,
		size = "md",
		preview = "none",
		title = p.title,
		subtitle = p.subtitle,
		icon = p.icon,
		noun = p.noun,
		placeholder = p.placeholder,
		items = items,
		order = order,
		selected = p.selected,
		current = p.current,
		state = p.state,
		message = p.message,
		empty = p.empty,
		total = if p.total and p.total ~= #vis then p.total else nil,
		actions = actions,
		columns = p.columns,
		layout = p.layout,
		role = "tern-projects.picker",
	}
	if p.query then
		props.query = p.query.text
		props.cursor = tf.utf16(p.query)
	end
	return ui.node("picker", props)
end

return picker
