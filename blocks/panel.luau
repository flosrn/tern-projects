--!strict
-- Durable library metadata comes from the backend, never pane persistence.
local ui = tern.ui
local cli = require("../lib/cli")
local fmt = require("../lib/fmt")
local tf = require("../lib/textfield")
local input = require("../lib/input")
local editor = require("../lib/project-editor")

local function pane_alive(id: number): boolean
	for _, p in tern.pane.list() do if p.pane == id then return true end end
	return false
end
local function live(s: any): boolean return s.alive and pane_alive(s.cx.pane) end
local function chip(label: string, action: string, active: boolean?, disabled: boolean?): any
	return ui.node("badge", {text = label, tone = active and "accent" or "neutral", disabled = disabled, actions = {click = action}})
end
local function project_key(s: any, p: any): string return s.view == "mine" and ("project:" .. tostring(p.id)) or ("discovered:" .. p.root) end
local function rebuild(s: any)
	local rows, order = {}, {}
	local function add(k: string, kind: string, ref: any, parent: string?)
		rows[k] = {kind = kind, ref = ref, parent = parent}; table.insert(order, k)
	end
	if s.mode == "projects" then
		if #s.pinned > 0 then
			add("h:pinned", "header", {label = "Pinned"})
			for _, chat in s.pinned do add("pin:" .. chat.uid, "session", chat) end
		end
		add("h:projects", "header", {label = s.view == "mine" and "My Projects" or "Discovered"})
		if #s.projects == 0 and s.view == "mine" then add("create", "create", {label = "Create project"}) end
		for _, p in s.projects do
			local key = project_key(s, p)
			add(key, "project", p)
			if s.expanded[key] then
				local chats = p.sessions or {}
				if #chats == 0 then add(key .. ":empty", "header", {label = "No chats"}, key) end
				for i = 1, math.min(#chats, s.more[key] and 25 or 5) do local chat = chats[i]; add(key .. ":" .. chat.uid, "session", chat, key) end
				if #chats > 5 and not s.more[key] then add(key .. ":more", "more", p, key) end
				if (p.count or 0) > #chats then add(key .. ":older", "older", p, key) end
			end
		end
		add("h:recent", "header", {label = "Recents"})
		for _, chat in s.recent do add("recent:" .. chat.uid, "session", chat) end
	elseif s.mode == "search" then
		for _, chat in s.results do add("search:" .. chat.uid, "session", chat) end
	end
	s.rows, s.order = rows, order
	s.page = math.min(s.page, math.max(0, math.ceil(#order / 150) - 1))
	if s.sel and not rows[s.sel] then s.sel = nil end
end
local reload: (any) -> ()
local action: (any, string, string?) -> ()
local function cancel_pending(s: any)
	s.flow = (s.flow or 0) + 1
	s.pending_request = nil
	s.sheet = nil
	s.pending_plan = nil
end
local function request(s: any, payload: any, cb: (any) -> (), channel: string?)
	local key = channel or "request"
	s.tokens[key] = (s.tokens[key] or 0) + 1
	local token = s.tokens[key]
	local mode, flow = s.mode, s.flow or 0
	local transient = key ~= "mutation" and key ~= "prefill"
	if transient then s.pending_request = {key = key, token = token} end
	cli.request(payload, function(data, err)
		if not live(s) or s.tokens[key] ~= token then return end
		if transient and s.pending_request and s.pending_request.key == key and s.pending_request.token == token then s.pending_request = nil end
		if key ~= "mutation" and (s.mode ~= mode or (s.flow or 0) ~= flow) then s.cx:render(); return end
		if err then s.error = err; if key == "mutation" then s.busy = false end else cb(data) end
		s.cx:render()
	end)
end
local function sheet(s: any, title: string, choices: any, detail: string?)
	s.sheet = {title = title, choices = choices, selected = 1, detail = detail}
end
local function mutate(s: any, payload: any, done: (() -> ())?)
	if s.busy then return end
	s.busy = true
	local mode, flow = s.mode, s.flow or 0
	request(s, payload, function(_)
		s.busy = false; s.error = nil
		if done and s.mode == mode and (s.flow or 0) == flow then done() end
		reload(s)
	end, "mutation")
end
reload = function(s: any)
	s.gen += 1
	local gen = s.gen
	local function accept(data: any, err: string?)
		if not live(s) or s.gen ~= gen then return end
		s.loading = false
		if err then s.error = err else
			s.error = nil
			if s.mode == "search" then s.results = data.sessions or {}; s.total = data.total or 0
			else s.projects = data.projects or {}; s.pinned = data.pinned or {}; s.recent = data.recent or {}; s.settings = data.settings or s.settings end
			rebuild(s)
		end
		s.cx:render()
	end
	s.loading = true
	if s.mode == "search" then
		cli.request({op = "snapshot", view = "mine", per = 0, recent = 0}, function(data, err)
			if not live(s) or s.gen ~= gen or err or not data then return end
			s.settings = data.settings or s.settings
			if s.project_id then
				local found = false
				for _, p in data.projects or {} do if tostring(p.id) == s.project_id then found = true; s.project_label = p.name end end
				if not found then s.project_id = nil; s.project_label = nil; s.search_cwd = nil; s.offset = 0; reload(s); return end
			end
			s.cx:render()
		end)
		local args = {"query", "--scope", s.scope, "--cwd", s.search_cwd or s.ctx_cwd, "--sort", s.sort, "--limit", "50", "--offset", tostring(s.offset), "--q", s.query.text}
		if s.project_id then table.insert(args, "--project-id"); table.insert(args, s.project_id) end
		cli.run(args, cli.TIMEOUT_QUERY, accept)
	else
		cli.request({op = "snapshot", view = s.view, per = 25, recent = 15}, accept)
	end
end
local function refresh(s: any)
	reload(s)
	if s.indexing or not live(s) then return end
	s.indexing = true
	cli.run({"index"}, cli.TIMEOUT_INDEX, function(_, err)
		if not live(s) then return end
		s.indexing = false
		if err then s.error = err; s.cx:render() else reload(s) end
	end)
end
local function resume(s: any, chat: any)
	local r = chat.resume
	if not r then return end
	if r.kind == "spawn" then
		(tern :: any).process.run(r.argv or {}, {timeout_ms = 10000}, function(result: any)
			if not live(s) then return end
			if result.status ~= 0 then s.cx:toast("error", "Resume failed", result.stderr ~= "" and result.stderr or result.stdout) end
		end)
	elseif r.command and r.command ~= "" then
		s.cx:open("tern-projects://resume?cwd=" .. fmt.pct(chat.cwd or "") .. "&cmd=" .. fmt.pct(r.command))
	end
end
local function open_editor(s: any, p: any?)
	s.editor = editor.new(p)
	s.sheet = nil
	local draft = s.editor
	if not p and s.ctx_cwd ~= "" then
		request(s, {op = "folders", path = s.ctx_cwd, cwd = s.ctx_cwd, offset = 0, limit = 1}, function(data)
			if s.editor == draft and #draft.folders == 0 then draft.folders = {data.path} end
		end, "prefill")
	end
	s.cx:render(); s.cx:frame({{"focus", "layer.editor.name"}})
end
local function browser(s: any, intent: string, value: string?)
	local d = s.editor
	if not d or not d.browser then return end
	local b = d.browser
	if intent == "parent" then if not b.parent then return end; tf.set(b.path, b.parent); b.offset = 0
	elseif intent == "directory" then local entry = b.entries[tonumber(value) or 0]; if not entry then return end; tf.set(b.path, entry.path); b.offset = 0
	elseif intent == "browser-prev" then b.offset = math.max(0, b.offset - 100)
	elseif intent == "browser-next" then if b.offset + #b.entries >= b.total then return end; b.offset += 100
	else b.offset = 0 end
	b.gen += 1
	local gen, path = b.gen, b.path.text
	b.busy = true; b.valid_path = nil
	cli.request({op = "folders", path = path, cwd = s.ctx_cwd, offset = b.offset, limit = 100}, function(data, err)
		if not live(s) or s.editor ~= d or d.browser ~= b or b.gen ~= gen or b.path.text ~= path then return end
		b.busy = false; b.error = err
		if data then
			tf.set(b.path, data.path); b.valid_path = data.path; b.parent = type(data.parent) == "string" and data.parent or nil
			b.entries = data.entries or {}; b.total = data.total or 0
		end
		s.cx:render()
	end)
end
local harness_picker: (any, string) -> ()
local function editor_action(s: any, act: string, value: string?)
	if not s.editor or s.busy then return end
	if s.launch_folder_only and (act == "browser-cancel" or act == "cancel") then s.editor = nil; s.launch_folder_only = false; return end
	if act == "focus" then
		if value == "name" or value == "path" then s.cx:frame({{"focus", "layer.editor." .. value}}) end
		return
	end
	local intent = editor.action(s.editor, act, value)
	if s.launch_folder_only and act == "use" and not s.editor.browser and #s.editor.folders > 0 then
		s.launch_target = {cwd = s.editor.folders[1]}; s.editor = nil; s.launch_folder_only = false; harness_picker(s, "launch")
		return
	end
	if intent == "cancel" then s.editor = nil; s.launch_folder_only = false; s.tokens.prefill = (s.tokens.prefill or 0) + 1
	elseif intent == "save" then
		local d = s.editor
		s.busy = true
		cli.request({op = "project-save", id = d.id, name = d.name.text, icon = d.icon, folders = (tern.json :: any).array(d.folders), cwd = s.ctx_cwd}, function(_, err)
			if not live(s) then return end
			s.busy = false
			if s.editor ~= d then if err then s.error = err end; reload(s); return end
			if err then d.error = err else s.editor = nil; s.view = "mine"; reload(s) end
			s.cx:render()
		end)
	elseif intent then browser(s, intent, value) end
	if act == "add-folder" then s.cx:render(); s.cx:frame({{"focus", "layer.editor.path"}}) end
end
local function project_choices(s: any, title: string, prefix: string, initial: any)
	request(s, {op = "snapshot", view = "mine", per = 0, recent = 0}, function(data)
		local choices = initial or {}
		for _, p in data.projects or {} do table.insert(choices, {label = p.name, detail = table.concat(p.folders or {}, " · "), act = prefix, value = tostring(p.id)}) end
		sheet(s, title, choices)
	end, "choices")
end
local function launch_route(s: any, plan: any)
	if type(plan.cwd) ~= "string" or type(plan.command) ~= "string" or plan.command == "" then s.error = "Invalid launch plan"; return end
	s.sheet = nil
	s.cx:open("tern-projects://launch?cwd=" .. fmt.pct(plan.cwd) .. "&cmd=" .. fmt.pct(plan.command))
end
local function launch_plan(s: any, harness: string)
	local target = s.launch_target
	if not target then return end
	local payload = {op = "launch-plan", harness = harness, project_id = target.project_id, cwd = target.cwd}
	request(s, payload, function(plan)
		if #(plan.warnings or {}) > 0 then
			s.pending_plan = plan
			local text = "Primary: " .. plan.cwd .. "\n" .. table.concat(plan.warnings, "\n")
			local retains_extras = false
			for _, arg in plan.argv or {} do if arg == "--add-dir" then retains_extras = true end end
			local choices = {{label = "Cancel", act = "cancel"}, {label = retains_extras and "Open with remaining folders" or "Open primary folder only", act = "launch-accept"}}
			for _, cmd in plan.manual_commands or {} do table.insert(choices, {label = "Copy " .. cmd, act = "copy-manual", value = cmd}) end
			sheet(s, "Additional folders were not added", choices, text)
		else launch_route(s, plan) end
	end, "launch")
end
harness_picker = function(s: any, purpose: string)
	request(s, {op = "harnesses"}, function(data)
		local choices = {}
		if purpose == "default" then table.insert(choices, {label = "Ask every time", act = "default-harness", value = "ask"}) end
		for _, h in data.harnesses or {} do
			table.insert(choices, {label = h.label, detail = type(h.reason) == "string" and h.reason or (type(h.path) == "string" and h.path or ""), icon = fmt.agent_icon(h.id), agent = h.id, disabled = not h.installed, act = purpose == "default" and "default-harness" or "launch-harness", value = h.id})
		end
		sheet(s, purpose == "default" and "Default harness" or "New chat with…", choices, purpose == "launch" and s.default_unavailable or nil)
	end, "harnesses")
end
local function new_chat(s: any, key: string?, choose: boolean)
	local row = s.rows[key or s.sel or ""]
	if row and row.parent then row = s.rows[row.parent] end
	if row and row.kind == "project" then
		s.launch_target = s.view == "mine" and {project_id = tostring(row.ref.id)} or {cwd = row.ref.root}
	elseif s.ctx_cwd ~= "" then s.launch_target = {cwd = s.ctx_cwd}
	else
		open_editor(s, {name = "", folders = {}})
		s.launch_folder_only = true
		editor_action(s, "add-folder", nil)
		return
	end
	local default = s.settings.default_harness
	s.default_unavailable = nil
	if choose or type(default) ~= "string" then harness_picker(s, "launch")
	else
		request(s, {op = "harnesses"}, function(data)
			for _, h in data.harnesses or {} do
				if h.id == default then
					if h.installed then launch_plan(s, default); return end
					s.default_unavailable = "Saved default " .. fmt.agent_label(default) .. " is unavailable. " .. (type(h.reason) == "string" and h.reason or "Choose an installed harness.")
				end
			end
			s.default_unavailable = s.default_unavailable or ("Saved default " .. fmt.agent_label(default) .. " is unavailable. Choose an installed harness.")
			harness_picker(s, "launch")
		end, "harnesses")
	end
end
local function menu(s: any, key: string): any
	local row = s.rows[key]
	if not row then return {} end
	local acts
	if row.kind == "session" then
		acts = {row.ref.pinned and "unpin" or "pin", "change-project", row.ref.archived and "restore" or "archive", "permanently-delete"}
	elseif row.kind == "project" then
		acts = s.view == "mine" and {"new-chat", "new-chat-with...", "edit-project", "remove-project"} or {"new-chat", "new-chat-with...", "create-project-from-folder"}
	else return {} end
	-- Native menu labels include the complete action string in this Tern build.
	-- Route its originating item id instead of exposing encoded ids in labels.
	return acts
end
local function archived(s: any, offset: number)
	request(s, {op = "archived", offset = offset, limit = 50}, function(data)
		s.archive_offset = offset; s.archive_total = data.total or 0; s.archived = data.sessions or {}
		local choices = {{label = "Close", act = "cancel"}}
		for _, chat in s.archived do
			table.insert(choices, {label = chat.title or chat.uid, detail = fmt.agent_label(chat.agent) .. " · " .. (chat.cwd or "") .. " · " .. fmt.age(chat.updated_ms or 0), act = "archived-actions", value = chat.uid, agent = chat.agent, icon = fmt.agent_icon(chat.agent)})
		end
		if offset > 0 then table.insert(choices, {label = "Previous", act = "archived-page", value = tostring(math.max(0, offset - 50))}) end
		if offset + 50 < s.archive_total then table.insert(choices, {label = "Next", act = "archived-page", value = tostring(offset + 50)}) end
		sheet(s, "Archived chats · " .. tostring(s.archive_total), choices)
	end, "archived")
end
local function find_chat(s: any, key: string): any
	local row = s.rows[key]
	if row and row.kind == "session" then return row.ref end
	for _, chat in s.archived do if chat.uid == key then return chat end end
	return nil
end
local function delete_preview(s: any, chat: any)
	request(s, {op = "delete-preview", uid = chat.uid}, function(data)
		s.delete_preview = data
		local detail = (data.title or chat.title or chat.uid) .. "\n" .. fmt.agent_label(data.agent) .. "\n" .. tostring(#(data.dependent_ids or {})) .. " affected child chats\n" .. table.concat(data.targets or {}, "\n") .. "\nThis removes the original saved chat and cannot be undone."
		if type(data.blocked_reason) == "string" then detail ..= "\n" .. data.blocked_reason end
		sheet(s, "Permanently delete chat?", {{label = "Cancel", act = "cancel"}, {label = "Permanently delete", act = "delete-confirm", disabled = type(data.blocked_reason) == "string"}}, detail)
	end, "delete-preview")
end
local function activate(s: any, key: string?)
	local row = s.rows[key or ""]
	if not row then return end
	s.sel = key
	if row.kind == "session" then resume(s, row.ref)
	elseif row.kind == "project" then s.expanded[key] = not s.expanded[key]; rebuild(s)
	elseif row.kind == "create" then open_editor(s, nil)
	elseif row.kind == "more" then s.more[row.parent] = true; rebuild(s)
	elseif row.kind == "older" then
		cancel_pending(s)
		s.mode = "search"; s.offset = 0; s.page = 0; s.search_cwd = nil; s.project_label = nil
		if s.view == "mine" then s.scope = "all"; s.project_id = tostring(row.ref.id); s.project_label = row.ref.name
		else s.scope = "repository"; s.search_cwd = row.ref.root; s.project_id = nil end
		reload(s)
	end
end
local function change_mode(s: any, mode: string)
	cancel_pending(s)
	if s.search_cwd then s.search_cwd = nil; s.offset = 0 end
	s.mode = mode; s.page = 0; rebuild(s); reload(s)
	if mode == "search" then s.cx:frame({{"focus", "main.query"}}) end
end
local function search_changed(s: any)
	s.offset = 0; s.page = 0; s.gen += 1
	local gen = s.gen
	tern.timer(150, function() if live(s) and s.gen == gen then reload(s) end end)
end
action = function(s: any, act: string, value: string?)
	if act == "cancel" then cancel_pending(s); return end
	if s.busy then return end
	if act == "editor" then local a, v = string.match(fmt.unpct(value or ""), "^([^=]+)=(.*)$"); editor_action(s, a or fmt.unpct(value or ""), v); return end
	if act == "mode" then change_mode(s, value or "projects")
	elseif act == "view" then s.view = value; s.page = 0; reload(s)
	elseif act == "create" then open_editor(s, nil)
	elseif act == "refresh" then refresh(s)
	elseif act == "scope" then s.scope = value; s.search_cwd = nil; s.offset = 0; reload(s)
	elseif act == "sort" then s.sort = s.sort == "updated" and "relevance" or "updated"; s.offset = 0; reload(s)
	elseif act == "filter" then project_choices(s, "My project filter", "filter-project", {{label = "All projects", act = "filter-project", value = "all"}})
	elseif act == "filter-project" then
		s.project_id = value ~= "all" and value or nil; s.project_label = nil; s.search_cwd = nil
		if s.project_id then s.scope = "all" end
		s.sheet = nil; s.offset = 0; reload(s)
	elseif act == "page" then s.page = math.max(0, s.page + (value == "next" and 1 or -1))
	elseif act == "search-page" then s.offset = math.max(0, s.offset + (value == "next" and 50 or -50)); reload(s)
	elseif act == "activate" then activate(s, fmt.unpct(value or ""))
	elseif act == "actions" then
		local key = value and fmt.unpct(value) or s.sel
		if key then
			local choices = {{label = "Cancel", act = "cancel"}}
			for _, a in menu(s, key) do table.insert(choices, {label = a == "new-chat-with..." and "New chat with…" or string.gsub(a, "-", " "), act = a, value = fmt.pct(key)}) end
			sheet(s, "Actions", choices)
		end
	elseif act == "new-chat" or act == "new-chat-with..." then s.sheet = nil; new_chat(s, value and fmt.unpct(value), act == "new-chat-with...")
	elseif act == "launch-harness" then s.sheet = nil; launch_plan(s, value or "")
	elseif act == "launch-accept" then if s.pending_plan then launch_route(s, s.pending_plan) end
	elseif act == "copy-manual" then s.cx:copy(value or "")
	elseif act == "default-picker" then harness_picker(s, "default")
	elseif act == "default-harness" then
		local payload: any = {op = "settings-update", default_harness = value == "ask" and (tern.json :: any).null or value}
		mutate(s, payload, function() s.sheet = nil end)
	elseif act == "icon-style" then mutate(s, {op = "settings-update", icon_style = s.settings.icon_style == "monochrome" and "color" or "monochrome"})
	elseif act == "archived" or act == "archived-page" then archived(s, tonumber(value) or 0)
	elseif act == "archived-actions" then
		local chat = find_chat(s, value or "")
		if chat then sheet(s, chat.title or chat.uid, {{label = "Back", act = "archived"}, {label = "Restore", act = "restore", value = fmt.pct(chat.uid)}, {label = "Permanently delete", act = "permanently-delete", value = fmt.pct(chat.uid)}, {label = "Resume", act = "archived-resume", value = chat.uid}, {label = "Copy command", act = "archived-copy", value = chat.uid}}, fmt.agent_label(chat.agent) .. "\n" .. (chat.cwd or "")) end
	elseif act == "archived-resume" or act == "archived-copy" then
		local chat = find_chat(s, value or ""); if chat then if act == "archived-resume" then resume(s, chat) else s.cx:copy(chat.resume and chat.resume.command or "") end end
	elseif act == "delete-confirm" then
		local p = s.delete_preview
		if p and type(p.blocked_reason) ~= "string" then mutate(s, {op = "delete", uid = p.uid, expected_source = p.source, expected_version = p.version}, function() s.sheet = nil; s.delete_preview = nil end) end
	elseif act == "remove-confirm" then
		local id = s.remove_id
		if id then mutate(s, {op = "project-remove", id = id}, function() s.sheet = nil; if s.project_id == id then s.project_id = nil; s.project_label = nil; s.search_cwd = nil; s.offset = 0 end end) end
	elseif act == "assignment" then
		local assignment = if value == "automatic" or value == "none" then {mode = value} else {mode = "project", project_id = value}
		mutate(s, {op = "chat-update", uid = s.assignment_uid, assignment = assignment}, function() s.sheet = nil end)
	elseif act == "resume" or act == "copycommand" or act == "copyid" then
		local row = s.rows[s.sel or ""]
		if row and row.kind == "session" then
			if act == "resume" then resume(s, row.ref) elseif act == "copyid" then s.cx:copy(row.ref.id or row.ref.uid) else s.cx:copy(row.ref.resume and row.ref.resume.command or "") end
		end
	else
		local key = fmt.unpct(value or "")
		local row = s.rows[key]
		if act == "edit-project" and row then open_editor(s, row.ref)
		elseif act == "create-project-from-folder" and row then open_editor(s, {name = row.ref.label or "", folders = {row.ref.root}})
		elseif act == "remove-project" and row then
			s.remove_id = tostring(row.ref.id)
			sheet(s, "Remove project?", {{label = "Cancel", act = "cancel"}, {label = "Remove project", act = "remove-confirm"}}, (row.ref.name or "") .. "\nChats and source folders will not be deleted.")
		else
			local chat = find_chat(s, key)
			if chat then
				if act == "pin" or act == "unpin" then mutate(s, {op = "chat-update", uid = chat.uid, pinned = act == "pin"}, function() s.sheet = nil end)
				elseif act == "archive" or act == "restore" then mutate(s, {op = "chat-update", uid = chat.uid, archived = act == "archive"}, function() s.sheet = nil end)
				elseif act == "change-project" then s.assignment_uid = chat.uid; project_choices(s, "Change project", "assignment", {{label = "Automatic (use source folders)", act = "assignment", value = "automatic"}, {label = "No project", act = "assignment", value = "none"}})
				elseif act == "permanently-delete" then delete_preview(s, chat) end
			end
		end
	end
end
local function nav(s: any, dir: number)
	local idx = table.find(s.order, s.sel) or (dir < 0 and #s.order + 1 or 0)
	idx += dir
	while idx >= 1 and idx <= #s.order do
		local row = s.rows[s.order[idx]]
		if row.kind ~= "header" then s.sel = s.order[idx]; s.page = math.floor((idx - 1) / 150); return end
		idx += dir
	end
end
local panel: BlockDef<any> = {
	init = function(cx, args, saved)
		local s: any = {cx = cx, mode = args and args[1] or "projects", ctx_cwd = args and args[2] or "", view = "mine", projects = {}, pinned = {}, recent = {}, expanded = {}, more = {}, settings = {icon_style = "color"}, query = tf.new(""), query_all = false, scope = "workspace", sort = "updated", results = {}, total = 0, offset = 0, page = 0, rows = {}, order = {}, gen = 0, tokens = {}, alive = true, busy = false, indexing = false, archived = {}}
		if saved then
			if saved.view == "mine" or saved.view == "discovered" then s.view = saved.view end
			if saved.scope == "workspace" or saved.scope == "repository" or saved.scope == "all" then s.scope = saved.scope end
			if saved.sort == "updated" or saved.sort == "relevance" then s.sort = saved.sort end
			if type(saved.query) == "string" then tf.set(s.query, saved.query) end
		end
		pcall(tern.fs.write, tern.plugin.data .. "/panel-cols", tostring(cx.cols))
		refresh(s)
		local function tick()
			tern.timer(120000, function() if not live(s) then s.alive = false; return end; refresh(s); tick() end)
		end
		tick()
		return s
	end,
	save = function(s) return {mode = s.mode, view = s.view, query = s.query.text, scope = s.scope, sort = s.sort} end,
	title = function(s) return s.mode == "projects" and "Projects" or (s.mode == "search" and "Projects · Search" or "Projects · Settings") end,
	view = function(s, _cx)
		local children: {any} = {ui.row({chip("Projects", "mode=projects", s.mode == "projects"), chip("Search", "mode=search", s.mode == "search"), chip("Settings", "mode=settings", s.mode == "settings")})}
		local status = s.indexing and "Indexing…" or (s.loading and "Loading…" or "")
		if s.mode ~= "projects" then
			table.insert(children, ui.row({ui.text({ui.span(status, "muted")}), chip("Refresh", "refresh")}))
		elseif status ~= "" then
			table.insert(children, ui.text({ui.span(status, "muted")}))
		end
		if s.error then table.insert(children, ui.text({ui.span(s.error, "error")})) end
		if s.mode == "settings" then
			table.insert(children, ui.text("Default harness"))
			table.insert(children, chip(type(s.settings.default_harness) == "string" and fmt.agent_label(s.settings.default_harness) or "Ask every time", "default-picker", (s.settings_focus or 1) == 1))
			table.insert(children, ui.text("Harness icons"))
			table.insert(children, chip(s.settings.icon_style == "monochrome" and "Monochrome" or "Color", "icon-style", s.settings_focus == 2))
			table.insert(children, chip("Archived chats", "archived", s.settings_focus == 3))
		else
			if s.mode == "projects" then
				table.insert(children, ui.row({chip("My Projects", "view=mine", s.view == "mine"), chip("Discovered", "view=discovered", s.view == "discovered"), ui.node("icon", {name = "plus", aria = "Create project", title = "Create project", actions = {click = "create"}}), chip("New chat", "new-chat"), chip("Refresh", "refresh")}))
			else
				table.insert(children, ui.row({chip("Workspace", "scope=workspace", s.scope == "workspace"), chip("Repository", "scope=repository", s.scope == "repository"), chip("All", "scope=all", s.scope == "all")}))
				table.insert(children, chip(s.project_id and "My project: " .. (s.project_label or s.project_id) or "All projects", "filter", s.project_id ~= nil))
				table.insert(children, ui.node("input", {key = "query", text = s.query.text, cursor = tf.utf16(s.query), anchor = s.query_all and 0 or nil, placeholder = "Search chats"}))
				table.insert(children, ui.row({ui.text(tostring(s.total) .. " chats"), chip(s.sort == "updated" and "Last updated" or "Relevance", "sort")}))
			end
			local items = {}
			for i = s.page * 150 + 1, math.min(#s.order, (s.page + 1) * 150) do
				local key = s.order[i]; local row = s.rows[key]; local r = row.ref
				local props: any = {key = key, role = row.parent and "tern-projects.project-chat" or nil}
				if row.kind == "session" then
					props.icon = fmt.agent_icon(r.agent or ""); props.label = r.title or r.uid
					props.detail = s.mode == "search" and s.query.text ~= "" and r.snippet and fmt.snippet_spans(r.snippet) or r.last_assistant
					props.value = fmt.age(r.updated_ms or 0)
					props.actions = {menu = menu(s, key)}
					props.role = row.parent and "tern-projects.project-chat" or "tern-projects.chat"
					props.tone = fmt.agent_tone(r.agent, s.settings.icon_style)
				elseif row.kind == "project" then
					props.icon = s.view == "mine" and (r.icon or "folder") or (s.expanded[key] and "folder-open" or "folder")
					props.label = r.name or r.label or r.root; props.value = tostring(r.count or 0)
					props.detail = r.folders and table.concat(r.folders, " · ") or r.root
					props.actions = {click = "activate=" .. fmt.pct(key), dblclick = "ignore", menu = menu(s, key)}
				else
					props.label = row.kind == "header" and r.label or (row.kind == "more" and "Show more chats" or (row.kind == "older" and "Older history — search" or "Create project"))
					props.disabled = row.kind == "header"; props.actions = {click = "activate=" .. fmt.pct(key)}
				end
				table.insert(items, ui.node("item", props))
			end
			table.insert(children, ui.node("list", {key = "rows", role = "tern-projects.rows", max = 1, selected = s.sel and "main.rows." .. s.sel or nil, empty = {ui.span("No chats", "muted")}}, items))
			if s.mode == "search" then
				table.insert(children, ui.row({chip("Previous", "search-page=prev", false, s.loading or s.offset == 0), ui.text(tostring(s.total == 0 and 0 or s.offset + 1) .. "–" .. tostring(math.min(s.total, s.offset + #s.results))), chip("Next", "search-page=next", false, s.loading or s.offset + 50 >= s.total)}))
			elseif #s.order > 150 then table.insert(children, ui.row({chip("Previous", "page=prev", false, s.page == 0), ui.text("Page " .. tostring(s.page + 1)), chip("Next", "page=next", false, (s.page + 1) * 150 >= #s.order)})) end
		end
		local result: any = {main = ui.col(children)}
		local row = s.rows[s.sel or ""]
		if row and (row.kind == "session" or row.kind == "project") then
			local dock = {ui.text(row.ref.title or row.ref.name or row.ref.label or row.ref.uid or row.ref.root), chip("Actions", "actions=" .. fmt.pct(s.sel))}
			if row.kind == "session" then
				table.insert(dock, ui.node("text", {wrap = "char", text = fmt.agent_label(row.ref.agent) .. " · " .. tostring(row.ref.messages or 0) .. " msgs · " .. (row.ref.cwd or "")}))
				table.insert(dock, ui.row({chip("Resume", "resume"), chip("Copy command", "copycommand"), chip("Copy ID", "copyid")}))
			end
			result.dock = ui.col(dock)
		end
		if s.editor then result.layer = ui.col({editor.view(s.editor, s.busy or (s.editor.browser and s.editor.browser.busy))})
		elseif s.sheet then
			local sh = s.sheet; local body = {}
			if sh.detail then table.insert(body, ui.node("text", {text = sh.detail, wrap = "char"})) end
			for i, choice in sh.choices do
				local tone = if choice.agent then fmt.agent_tone(choice.agent, s.settings.icon_style) else (i == sh.selected and "accent" or "neutral")
				table.insert(body, ui.node("item", {key = tostring(i), label = choice.label, icon = choice.icon, detail = choice.detail, tone = tone, value = i == sh.selected and "›" or nil, disabled = choice.disabled or s.busy, actions = {click = "choice=" .. tostring(i)}}))
			end
			result.layer = ui.col({ui.node("overlay", {key = "sheet", modal = true, size = "md", head = sh.title}, body)})
		elseif s.pending_request then
			result.layer = ui.col({ui.node("overlay", {key = "pending", modal = true, size = "md", head = "Loading…"}, {ui.text("Preparing choices…"), chip("Cancel", "cancel")})})
		end
		return result
	end,
	key = function(s, k, _cx)
		if s.busy then return true end
		if s.editor then
			local handled, act, value = editor.key(s.editor, k)
			if act then editor_action(s, act, value) end
			return handled
		end
		if s.sheet then
			if k.name == "escape" then cancel_pending(s); return true end
			local sh = s.sheet
			if k.name == "tab" or k.name == "up" or k.name == "down" then
				local dir = (k.name == "up" or (k.name == "tab" and k.shift)) and -1 or 1
				sh.selected = (sh.selected - 1 + dir) % #sh.choices + 1
			elseif k.name == "enter" then local c = sh.choices[sh.selected]; if not c.disabled then action(s, c.act, c.value) end end
			return true
		end
		if s.pending_request then
			if k.name == "escape" then cancel_pending(s) end
			return true
		end
		if k.name == "tab" then local modes = {"projects", "search", "settings"}; local i = table.find(modes, s.mode) or 1; change_mode(s, modes[(i - 1 + (k.shift and -1 or 1)) % 3 + 1]); return true end
		if s.mode == "settings" then
			if k.name == "up" or k.name == "down" then s.settings_focus = ((s.settings_focus or 1) - 1 + (k.name == "up" and -1 or 1)) % 3 + 1; return true end
			if k.name == "enter" then action(s, ({"default-picker", "icon-style", "archived"})[s.settings_focus or 1], nil); return true end
		end
		if k.name == "f10" and k.shift then action(s, "actions", nil); return true end
		if k.name == "up" or k.name == "down" then nav(s, k.name == "up" and -1 or 1); return true end
		if k.name == "page_up" or k.name == "page_down" then for _ = 1, 10 do nav(s, k.name == "page_up" and -1 or 1) end; return true end
		if k.name == "enter" then activate(s, s.sel); return true end
		if k.name == "r" and k.ctrl then refresh(s); return true end
		if k.name == "y" and k.ctrl then action(s, "copycommand", nil); return true end
		if s.mode == "projects" then
			if k.name == "/" then change_mode(s, "search"); return true end
			if k.name == "left" or k.name == "right" then local row = s.rows[s.sel or ""]; if row and row.kind == "project" then s.expanded[s.sel] = k.name == "right"; rebuild(s) end; return true end
		elseif s.mode == "search" then
			local before = s.query.text
			if k.name == "escape" then tf.set(s.query, ""); s.query_all = false else local handled, all = input.key(s.query, s.query_all, k, tern.runtime.os); s.query_all = all; if not handled then return false end end
			if before ~= s.query.text then search_changed(s) end
			return true
		end
		return false
	end,
	event = function(s, ev, cx)
		local id, name = ev.id or "", ev.ev
		if name == "focus" or name == "edit" then
			if s.busy then return end
			local field, owner
			if s.editor then
				owner = s.editor.browser or s.editor
				local key = s.editor.browser and "path" or "name"
				if id ~= "layer.editor." .. key then return end
				if name == "focus" then owner.focus = key end
				if owner.focus ~= key then return end
				field = s.editor.browser and s.editor.browser.path or s.editor.name
			elseif not s.sheet and not s.pending_request and s.mode == "search" and id == "main.query" then field = s.query; owner = s end
			if not field then return end
			if name == "focus" then cx:frame({{"focus", id}})
			else
				local before = field.text
				if input.edit(field, ev) then
					owner.selected_all = false
					if field == s.query then
						s.query_all = false
						if before ~= field.text then search_changed(s) end
					elseif s.editor.browser and before ~= field.text then
						s.editor.browser.valid_path = nil
						s.editor.browser.gen += 1
						s.editor.browser.busy = false
					end
				end
			end
		elseif name == "select" or name == "activate" then
			if s.editor or s.sheet or s.pending_request then return end
			if id == "main.rows" then local key = string.match(ev.item or "", "^main%.rows%.(.+)$") or ev.item; s.sel = key; if name == "activate" then activate(s, key) end end
		elseif name == "action" then
			local act, value = ev.act or "", ev.value
			if value == nil then
				local target = string.match(id, "^main%.rows%.(.+)$")
				if target and s.rows[target] and table.find(menu(s, target), act) then value = fmt.pct(target) end
			end
			if act == "open" or string.sub(act, 1, 5) == "open=" then
				local payload = string.sub(act, 1, 5) == "open=" and string.sub(act, 6) or value or ""
				local mode, cwd = string.match(payload, "^([^:]+):?(.*)")
				if mode then s.editor = nil; s.sheet = nil; if cwd ~= "" then s.ctx_cwd = cwd end; change_mode(s, mode) end
			elseif s.editor then if act == "editor" then action(s, act, value) end
			elseif s.sheet then
				if act == "choice" then local c = s.sheet.choices[tonumber(value) or 0]; if c and not c.disabled then action(s, c.act, c.value) end end
			elseif s.pending_request then
				if act == "cancel" then cancel_pending(s) end
			else action(s, act, value) end
		end
		cx:render()
	end,
}
return panel
