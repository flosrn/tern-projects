--!strict
-- Create/Edit project draft: the form overlay and its Add folder picker.
-- The panel owns requests and commits; this module owns the draft, focus and rendering.
local tf = require("./textfield")
local input = require("./input")
local fmt = require("./fmt")
local c = require("./ui")
local picker = require("./picker")
local ui = tern.ui
local editor = {}

local ICONS = {
	{id = "folder", label = "Folder"},
	{id = "code", label = "Code"},
	{id = "terminal", label = "Terminal"},
	{id = "puzzle", label = "Puzzle"},
	{id = "wrench", label = "Wrench"},
	{id = "rocket", label = "Rocket"},
}
editor.ICONS = ICONS

function editor.new(project: any?): any
	local p = project or {}
	-- Decoded JSON arrays have a protected metatable; drafts must own plain tables.
	local folders: {string} = {}
	local missing = {}
	for _, path in p.folders or {} do
		table.insert(folders, path)
		local ok, exists = pcall(tern.fs.exists, path)
		if ok and not exists then missing[path] = true end
	end
	return {
		id = p.id,
		name = tf.new(p.name or ""),
		name_touched = p.name ~= nil and p.name ~= "",
		icon = p.icon or "folder",
		folders = folders,
		missing = missing,
		focus = "name",
		selected_all = p.name ~= nil and p.name ~= "",
		attempted = false,
		error = nil,
		browser = nil,
	}
end

function editor.valid(d: any): boolean
	return string.match(d.name.text, "%S") ~= nil and #d.folders > 0
end

-- Adds a folder, naming an unnamed draft after its first folder.
function editor.add_folder(d: any, path: string)
	if not table.find(d.folders, path) then table.insert(d.folders, path) end
	if not d.name_touched and not string.match(d.name.text, "%S") then
		tf.set(d.name, fmt.basename(fmt.path(path)))
		d.selected_all = true
	end
end

-- Focusable controls in visual order.
function editor.controls(d: any): {string}
	local out = {"name"}
	table.insert(out, "icon=" .. d.icon)
	for i in d.folders do
		if i > 1 then table.insert(out, "primary=" .. tostring(i)) end
		table.insert(out, "remove-folder=" .. tostring(i))
	end
	table.insert(out, "add-folder"); table.insert(out, "cancel"); table.insert(out, "save")
	return out
end

local function focus_valid(d: any)
	if not table.find(editor.controls(d), d.focus) then d.focus = "add-folder" end
end

-- Applies a form action; returns an intent for the panel: "save", "cancel", "browse" or nil.
function editor.action(d: any, act: string, value: string?): string?
	local control = if value then act .. "=" .. value else act
	if table.find(editor.controls(d), control) then d.focus = control; d.selected_all = false end
	if act == "icon" then
		d.selected_all = false
		for _, icon in ICONS do
			if icon.id == value then d.icon = value; d.focus = "icon=" .. value end
		end
	elseif act == "primary" then
		local p = table.remove(d.folders, tonumber(value) or 0)
		if p then table.insert(d.folders, 1, p) end
		d.focus = "remove-folder=1"
	elseif act == "remove-folder" then
		table.remove(d.folders, tonumber(value) or 0)
		focus_valid(d)
	elseif act == "add-folder" then return "browse"
	elseif act == "save" then
		d.attempted = true
		if editor.valid(d) then return "save" end
		d.focus = if string.match(d.name.text, "%S") then "add-folder" else "name"
	elseif act == "cancel" then return "cancel" end
	return nil
end

-- Keyboard for the form (not the folder picker). Returns handled, intent, value.
function editor.key(d: any, k: Key): (boolean, string?, string?)
	local name = k.name
	if name == "escape" then return true, "cancel", nil end
	if name == "enter" and (k.meta or k.ctrl) then return true, "save", nil end
	if name == "tab" then
		local controls = editor.controls(d)
		local idx = table.find(controls, d.focus) or 1
		d.focus = controls[(idx - 1 + (if k.shift then -1 else 1)) % #controls + 1]
		d.selected_all = false
		return true, "focus", d.focus
	end
	local act, value = string.match(d.focus, "^([^=]+)=(.*)$")
	act = act or d.focus
	if act == "icon" and (name == "left" or name == "right") then
		local idx = 1
		for i, icon in ICONS do if icon.id == value then idx = i end end
		idx = math.clamp(idx + (if name == "left" then -1 else 1), 1, #ICONS)
		d.icon = ICONS[idx].id
		d.focus = "icon=" .. d.icon
		return true, nil, nil
	end
	if d.focus == "name" then
		if name == "enter" then return true, "save", nil end
		local before = d.name.text
		local handled, all = input.key(d.name, d.selected_all, k, tern.runtime.os)
		d.selected_all = all
		if d.name.text ~= before then d.name_touched = true end
		return handled or true, nil, nil
	end
	if name == "enter" or name == "space" or k.text == " " then return true, act, value end
	if name == "up" or name == "down" then
		local controls = editor.controls(d)
		local idx = table.find(controls, d.focus) or 1
		d.focus = controls[math.clamp(idx + (if name == "up" then -1 else 1), 1, #controls)]
		return true, "focus", d.focus
	end
	return true, nil, nil
end

local function act(a: string): string return "editor=" .. fmt.pct(a) end

function editor.view(d: any, busy: boolean): any
	local f = d.focus
	local name_blank = not string.match(d.name.text, "%S")
	local body: {any} = {}
	table.insert(body, c.label("Name"))
	table.insert(body, ui.node("input", {
		key = "name",
		text = d.name.text,
		cursor = tf.utf16(d.name),
		anchor = if d.selected_all then 0 else nil,
		placeholder = "e.g. Archivr",
		aria = "Project name",
		role = if (d.attempted or d.name_touched) and name_blank then "tern-projects.field-invalid" else "tern-projects.field",
	}))
	if (d.attempted or d.name_touched) and name_blank then table.insert(body, c.note("Enter a project name.", "error")) end

	local icon_label = "Folder"
	for _, icon in ICONS do if icon.id == d.icon then icon_label = icon.label end end
	table.insert(body, c.label("Icon", icon_label))
	local tiles = {}
	for _, icon in ICONS do
		local cls = "tp-ico"
		if d.icon == icon.id then cls ..= " on" end
		if f == "icon=" .. icon.id then cls ..= " kfocus" end
		table.insert(tiles, ui.el("button", {key = icon.id, class = cls, title = icon.label, aria = icon.label, attrs = {role = "radio", tabindex = if d.icon == icon.id then "0" else "-1", ["aria-checked"] = if d.icon == icon.id then "true" else "false"}, actions = {click = act("icon=" .. icon.id)}}, {ui.node("icon", {name = icon.id})}))
	end
	table.insert(body, ui.el("div", {class = "tp-icons", attrs = {role = "radiogroup", ["aria-label"] = "Project icon"}}, tiles))

	table.insert(body, c.label("Source folders", if #d.folders > 1 then tostring(#d.folders) .. " folders" else nil))
	local rows = {}
	for i, path in d.folders do
		local missing = d.missing and d.missing[path]
		local kids: {any} = {
			ui.node("icon", {name = if missing then "warn" elseif i == 1 then "folder-open" else "folder", tone = if missing then "warning" else nil, title = if missing then "Folder not found" else nil}),
			ui.node("text", {text = fmt.path(path), truncate = "middle", title = path .. (if missing then " — Folder not found" else ""), role = "tern-projects.path", grow = 1}),
		}
		local primary = if i == 1
			then ui.el("span", {class = "tp-tag", text = "Primary", title = "New chats start here"})
			else c.icon_button("arrow-up", act("primary=" .. tostring(i)), "Make primary", {variant = "ghost", size = "xs", focused = f == "primary=" .. tostring(i), disabled = busy})
		table.insert(kids, ui.el("div", {class = "tp-folder-slot"}, {primary}))
		table.insert(kids, c.icon_button("x", act("remove-folder=" .. tostring(i)), "Remove " .. fmt.path(path), {variant = "ghost", size = "xs", focused = f == "remove-folder=" .. tostring(i), disabled = busy}))
		table.insert(rows, ui.el("div", {key = path, class = "tp-folder" .. (if i == 1 then " primary" else "")}, kids))
	end
	if #d.folders == 0 then
		table.insert(rows, ui.el("div", {class = "tp-folder empty" .. (if d.attempted then " invalid" else "")}, {
			ui.node("icon", {name = "folder-plus"}),
			ui.el("span", {class = "tp-folder-empty", text = "Add the folder where this project's chats run."}),
		}))
	end
	table.insert(body, ui.el("div", {class = "tp-folders"}, rows))
	if #d.folders > 0 then
		table.insert(body, c.note("New chats start in the primary folder. Chats from " .. (if #d.folders == 1 then "this folder" else "these folders") .. ", including subfolders, appear here."))
	end
	table.insert(body, ui.el("div", {class = "tp-row"}, {
		c.button("Add folder…", act("add-folder"), {icon = "plus", size = "sm", focused = f == "add-folder", disabled = busy}),
	}))
	if d.error then table.insert(body, c.banner("editor-error", "error", "warn", d.error)) end
	local save_label = if d.id then "Save changes" else "Create project"
	local footer = c.footer({
		c.button("Cancel", act("cancel"), {variant = "ghost", focused = f == "cancel", disabled = busy}),
		c.grow(),
		c.button(if busy then "Saving…" else save_label, act("save"), {variant = "primary", focused = f == "save", disabled = busy or not editor.valid(d), title = if editor.valid(d) then "⌘⏎" else "Add a name and at least one folder"}),
	})
	return ui.node("overlay", {key = "editor", modal = true, size = "md", head = if d.id then "Edit project" else "Create project", role = "tern-projects.dialog"}, {
		ui.el("div", {key = "form", class = "tp-form"}, body),
		footer,
	})
end

-- Folder picker -------------------------------------------------------------------------
-- The field names a folder. Typing after the last "/" filters that folder's subfolders, so
-- paths complete as they are typed; the panel lists a new parent folder when the text leaves
-- the current one.

function editor.open_browser(d: any, start: string?)
	local b = {
		listing = nil, -- canonical path of the folder whose subfolders are listed
		parent = nil,
		entries = {},
		total = 0,
		filter = "",
		gen = 0,
		busy = false,
		error = nil,
		loaded = false,
		target = nil,
	}
	b.picker = picker.new({
		kind = "folder",
		title = if d.launch_only then "New chat" else "Add folder",
		subtitle = if d.launch_only then "Choose a starting folder" elseif string.match(d.name.text, "%S") then "To " .. d.name.text else "Choose a source folder",
		icon = "folder-plus",
		noun = "folders",
		search = true,
		query = if start then fmt.path(start) else "",
		filter = false,
		placeholder = "Type a path, like ~/work/app",
		state = "loading",
		items = {},
		actions = {},
	})
	d.browser = b
	editor.sync_browser(d)
end

-- Splits field text into the folder it names and the partial name typed after it.
function editor.split(text: string): (string?, string)
	if text == "" then return nil, "" end
	if string.sub(text, -1) == "/" then return (if #text > 1 then string.sub(text, 1, -2) else "/"), "" end
	local dir, base = string.match(text, "^(.*)/([^/]*)$")
	if dir == nil then return nil, text end
	return (if dir == "" then "/" else dir), base :: string
end

-- Whether `dir` (as typed) names the listed folder.
function editor.is_listing(b: any, dir: string?): boolean
	if not dir or not b.listing then return false end
	return dir == b.listing or dir == fmt.path(b.listing)
end

-- Rebuilds picker rows and actions from the browser state.
function editor.sync_browser(d: any)
	local b = d.browser
	if not b then return end
	local p = b.picker
	local q = string.lower(b.filter or "")
	local items = {}
	if q == "" and b.listing then
		table.insert(items, {id = "use:" .. b.listing, label = "Use “" .. fmt.basename(fmt.path(b.listing)) .. "”", detail = fmt.path(b.listing), icon = "check", data = {path = b.listing, use = true}})
	end
	if q == "" and b.parent then table.insert(items, {id = "parent:" .. b.parent, label = "Parent folder", detail = fmt.path(b.parent), icon = "arrow-up", data = {path = b.parent, parent = true}}) end
	local matches = {}
	for _, entry in b.entries do
		local name = string.lower(entry.name)
		local prefix = q == "" or string.sub(name, 1, #q) == q
		if prefix or string.find(name, q, 1, true) then
			table.insert(matches, {id = "dir:" .. entry.path, label = entry.name, icon = "folder", tone = if string.sub(entry.name, 1, 1) == "." then "muted" else nil, detail = if table.find(d.folders, entry.path) then "In project" else nil, data = entry, prefix = prefix})
		end
	end
	table.sort(matches, function(a, b)
		local ah, bh = string.sub(a.label, 1, 1) == ".", string.sub(b.label, 1, 1) == "."
		if ah ~= bh then return not ah end
		if a.prefix ~= b.prefix then return a.prefix end
		local an, bn = string.lower(a.label), string.lower(b.label)
		if an == bn then return a.id < b.id end
		return an < bn
	end)
	for _, it in matches do table.insert(items, it) end
	if q == "" and #b.entries < b.total then
		table.insert(items, {id = "more", label = "Show more folders", detail = tostring(b.total - #b.entries) .. " more", icon = "more"})
	end
	p.items = items
	p.state = if b.error then "error" elseif not b.loaded then "loading" else "ready"
	p.message = if b.error and string.find(string.lower(b.error), "not an existing directory", 1, true)
		then "No folder at " .. p.query.text .. ". Check the path or choose a parent folder."
		else b.error
	p.empty = if q ~= "" then "No folders match “" .. b.filter .. "”" else "No subfolders"
	picker.settle(p)
	-- Both the primary action and keyboard acceptance use the highlighted row only.
	local sel = picker.selected_item(p)
	local target = if sel and sel.data then sel.data.path else nil
	b.target = if b.busy or b.error then nil else target
	local exists = b.target ~= nil and table.find(d.folders, b.target) ~= nil
	local name = if b.target then fmt.basename(fmt.path(b.target)) else nil
	p.actions = {
		{id = "close", label = if d.launch_only then "Cancel" else "Back", keys = {"esc"}},
		{id = "go", label = if sel and sel.data and sel.data.use then "Use this folder" elseif sel and sel.id == "more" then "Show more" else "Open folder", keys = {"enter"}, disabled = b.busy or b.error ~= nil or not sel},
		{id = "add", label = if exists then "Already in project" elseif name then (if d.launch_only then "Start in “" else "Add “") .. name .. "”" else (if d.launch_only then "Start here" else "Add folder"), keys = {if tern.runtime.os == "macos" then "cmd+enter" else "ctrl+enter"}, primary = true, disabled = (b.target == nil or exists)},
	}
end

-- Folder picker keys. Returns handled, an intent ("close", "open", "browse", "complete",
-- "more", "add", "query") and a path for open/complete.
function editor.browser_key(d: any, k: Key): (boolean, string?, string?)
	local b = d.browser
	local p = b.picker
	if b.busy and (k.name == "enter" or k.name == "tab") then return true, nil, nil end
	if k.name == "enter" and (k.meta or k.ctrl) then
		editor.sync_browser(d)
		return true, if b.target and not table.find(d.folders, b.target) then "add" else nil, nil
	end
	if k.name == "enter" or k.name == "tab" then
		local it = picker.selected_item(p)
		if it and it.data and it.data.use then
			if k.name == "enter" and not table.find(d.folders, it.data.path) then return true, "add", nil end
			return true, nil, nil
		end
		if it and it.data and it.data.parent then return true, "open", it.data.path end
		if it and it.id == "more" then return true, "more", nil end
		if it and it.data then return true, if k.name == "tab" then "complete" else "open", it.data.path end
		-- Nothing to pick: resolve the typed text as a folder (reports why it can't be opened).
		if k.name == "enter" and p.query.text ~= "" then return true, "browse", nil end
		return true, nil, nil
	end
	local handled, intent = picker.key(p, k)
	if intent == "query" then return true, "query", nil end
	if intent == "close" then return true, "close", nil end
	if intent == "add" then return true, "add", nil end
	if intent == nil then editor.sync_browser(d) end
	return handled, nil, nil
end

return editor
