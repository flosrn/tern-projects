--!strict
local tf = require("./textfield")
local input = require("./input")
local fmt = require("./fmt")
local ui = tern.ui
local editor = {}
local icons = { "folder", "code", "terminal", "puzzle", "wrench", "rocket" }

function editor.new(project: any?): any
	local p = project or {}
	-- Decoded JSON arrays have a protected metatable; drafts must own plain tables.
	local folders: {string} = {}
	for _, path in p.folders or {} do table.insert(folders, path) end
	return { id = p.id, name = tf.new(p.name or ""), icon = p.icon or "folder", folders = folders, focus = "name", selected_all = false, error = nil, browser = nil }
end

function editor.valid(d: any): boolean
	return string.match(d.name.text, "%S") ~= nil and #d.folders > 0
end

function editor.field(d: any): any
	if d.browser then return if d.browser.focus == "path" then d.browser.path else nil end
	return if d.focus == "name" then d.name else nil
end

function editor.controls(d: any): {string}
	if d.browser then
		local b = d.browser
		local out = { "path", "browse", "parent", "use", "browser-cancel" }
		for i in b.entries do table.insert(out, "directory=" .. tostring(i)) end
		table.insert(out, "browser-prev"); table.insert(out, "browser-next")
		return out
	end
	local out = { "name" }
	for _, icon in icons do table.insert(out, "icon=" .. icon) end
	for i in d.folders do
		table.insert(out, "primary=" .. tostring(i)); table.insert(out, "remove-folder=" .. tostring(i))
	end
	table.insert(out, "add-folder"); table.insert(out, "cancel"); table.insert(out, "save")
	return out
end

function editor.action(d: any, act: string, value: string?): string?
	local owner = d.browser or d
	local control = value and (act .. "=" .. value) or act
	if table.find(editor.controls(d), control) then owner.focus = control; owner.selected_all = false end
	if act == "icon" then d.icon = value
	elseif act == "primary" then local p = table.remove(d.folders, tonumber(value) or 0); if p then table.insert(d.folders, 1, p) end
	elseif act == "remove-folder" then
		table.remove(d.folders, tonumber(value) or 0)
		if not table.find(editor.controls(d), d.focus) then d.focus = "add-folder" end
	elseif act == "add-folder" then
		d.browser = { path = tf.new(d.folders[1] or ""), entries = {}, offset = 0, total = 0, focus = "path", selected_all = false, gen = 0 }
		return "browse"
	elseif act == "browser-cancel" then d.browser = nil
	elseif act == "use" then
		local b = d.browser
		if b and b.valid_path and b.valid_path == b.path.text then
			if not table.find(d.folders, b.valid_path) then table.insert(d.folders, b.valid_path) end
			d.browser = nil
		end
	elseif act == "save" then if editor.valid(d) then return "save" end
	elseif act == "cancel" then return "cancel"
	elseif act == "browse" or act == "parent" or act == "directory" or act == "browser-prev" or act == "browser-next" then return act
	end
	return nil
end

function editor.key(d: any, k: Key): (boolean, string?, string?)
	if k.name == "escape" then
		if d.browser then return true, "browser-cancel", nil end
		return true, "cancel", nil
	end
	local owner = d.browser or d
	if k.name == "tab" then
		local controls = editor.controls(d)
		local idx = table.find(controls, owner.focus) or 1
		owner.focus = controls[(idx - 1 + (k.shift and -1 or 1)) % #controls + 1]
		owner.selected_all = false
		return true, "focus", owner.focus
	end
	if k.name == "enter" or ((k.name == "up" or k.name == "down") and owner.focus ~= "name" and owner.focus ~= "path") then
		if k.name ~= "enter" then
			local controls = editor.controls(d); local idx = table.find(controls, owner.focus) or 1
			owner.focus = controls[(idx - 1 + (k.name == "up" and -1 or 1)) % #controls + 1]
			return true, "focus", owner.focus
		end
		local act, value = string.match(owner.focus, "^([^=]+)=(.*)$")
		act = act or owner.focus
		if act == "path" then act = "browse" elseif act == "name" then act = "save" end
		return true, act, value
	end
	local field = editor.field(d)
	if field then
		local before = field.text
		local handled, all = input.key(field, owner.selected_all, k, tern.runtime.os)
		owner.selected_all = all
		if d.browser and field.text ~= before then
			d.browser.valid_path = nil
			d.browser.gen += 1
			d.browser.busy = false
		end
		return handled, nil, nil
	end
	return true, nil, nil
end

local function chip(label: string, act: string, focused: boolean?, disabled: boolean?): any
	return ui.node("badge", { key = act, text = label, tone = focused and "accent" or "neutral", disabled = disabled, actions = { click = "editor=" .. fmt.pct(act) } })
end

function editor.view(d: any, busy: boolean): any
	local children: {any} = {}
	local b = d.browser
	if b then
		table.insert(children, ui.text("Folder path"))
		table.insert(children, ui.node("input", { key = "path", text = b.path.text, cursor = tf.utf16(b.path), anchor = b.selected_all and 0 or nil, placeholder = "Directory path" }))
		table.insert(children, ui.row({ chip("Browse", "browse", b.focus == "browse", busy), chip("Parent", "parent", b.focus == "parent", busy or b.parent == nil) }))
		if b.error then table.insert(children, ui.text({ui.span(b.error, "error")})) end
		for i, entry in b.entries do table.insert(children, chip(entry.name, "directory=" .. tostring(i), b.focus == "directory=" .. tostring(i), busy)) end
		table.insert(children, ui.row({chip("Previous", "browser-prev", b.focus == "browser-prev", busy or b.offset == 0), chip("Next", "browser-next", b.focus == "browser-next", busy or b.offset + #b.entries >= b.total)}))
		table.insert(children, ui.row({chip("Cancel", "browser-cancel", b.focus == "browser-cancel"), chip("Use this folder", "use", b.focus == "use", busy or b.valid_path ~= b.path.text)}))
	else
		table.insert(children, ui.text("Name"))
		table.insert(children, ui.node("input", { key = "name", text = d.name.text, cursor = tf.utf16(d.name), anchor = d.selected_all and 0 or nil, placeholder = "Project name" }))
		if not string.match(d.name.text, "%S") then table.insert(children, ui.text({ui.span("A project name is required", "muted")})) end
		table.insert(children, ui.text("Icon"))
		local picks = {}
		for _, icon in icons do table.insert(picks, ui.node("badge", { key = icon, icon = icon, text = (d.focus == "icon=" .. icon and "› " or "") .. icon, tone = d.icon == icon and "accent" or "neutral", actions = {click = "editor=" .. fmt.pct("icon=" .. icon)} })) end
		table.insert(children, ui.row(picks))
		table.insert(children, ui.text("Source folders"))
		for i, path in d.folders do
			table.insert(children, ui.node("text", {text = path, wrap = "char"}))
			table.insert(children, ui.row({chip(i == 1 and "Primary" or "Make primary", "primary=" .. tostring(i), d.focus == "primary=" .. tostring(i)), chip("Remove", "remove-folder=" .. tostring(i), d.focus == "remove-folder=" .. tostring(i))}))
		end
		if #d.folders == 0 then table.insert(children, ui.text({ui.span("At least one source folder is required", "error")})) end
		table.insert(children, chip("Add folder", "add-folder", d.focus == "add-folder", busy))
		if d.error then table.insert(children, ui.text({ui.span(d.error, "error")})) end
		table.insert(children, ui.row({chip("Cancel", "cancel", d.focus == "cancel", busy), chip(d.id and "Save changes" or "Create project", "save", d.focus == "save", busy or not editor.valid(d))}))
	end
	return ui.node("overlay", {key = "editor", modal = true, size = "md", head = b and "Add folder" or (d.id and "Edit project" or "Create project")}, children)
end
return editor
