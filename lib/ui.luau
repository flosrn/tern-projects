--!strict
-- Small view builders shared by the panel, pickers and the project editor.
-- Every class is styled by styles.css under the panel surface; `kfocus` is the keyboard focus ring.
local ui = tern.ui
local c = {}

export type ButtonOpts = {
	variant: string?, -- "primary" | "danger" | "ghost" | nil (secondary)
	size: string?, -- "sm" | "xs" | nil
	icon: string?,
	focused: boolean?,
	disabled: boolean?,
	key: string?,
	title: string?,
	pressed: boolean?,
	trailing: string?, -- trailing icon, e.g. "chev"
	grow: boolean?,
	class: string?,
}

local function classes(base: string, o: ButtonOpts): string
	local out = base
	if o.variant then out ..= " " .. o.variant end
	if o.size then out ..= " " .. o.size end
	if o.focused then out ..= " kfocus" end
	if o.disabled then out ..= " off" end
	if o.pressed then out ..= " on" end
	if o.grow then out ..= " grow" end
	if o.class then out ..= " " .. o.class end
	return out
end

-- A text button. `act` nil or `o.disabled` renders an inert button.
function c.button(label: string, act: string?, opts: ButtonOpts?): any
	local o: ButtonOpts = opts or {}
	local kids = {}
	if o.icon then table.insert(kids, ui.node("icon", {name = o.icon})) end
	if label ~= "" then table.insert(kids, ui.el("span", {class = "tp-btn-t", text = label})) end
	if o.trailing then table.insert(kids, ui.node("icon", {name = o.trailing, role = "tern-projects.trail"})) end
	local live = act ~= nil and not o.disabled
	return ui.el("button", {
		key = o.key,
		class = classes("tp-btn", o),
		title = o.title,
		aria = if label == "" then o.title else nil,
		attrs = {["aria-disabled"] = if o.disabled then "true" else nil, ["aria-pressed"] = if o.pressed ~= nil then (if o.pressed then "true" else "false") else nil},
		actions = if live then {click = act} else nil,
	}, kids)
end

-- A square icon-only button; `title` is both tooltip and accessible name.
function c.icon_button(icon: string, act: string?, title: string, opts: ButtonOpts?): any
	local o: ButtonOpts = opts or {}
	local live = act ~= nil and not o.disabled
	return ui.el("button", {
		key = o.key,
		class = classes("tp-btn icon", o),
		title = title,
		aria = title,
		attrs = {["aria-disabled"] = if o.disabled then "true" else nil},
		actions = if live then {click = act} else nil,
	}, {ui.node("icon", {name = icon})})
end

export type SegItem = {id: string, label: string, icon: string?, title: string?}

-- A segmented control. Each segment sends `prefix .. "=" .. id`.
function c.seg(items: {SegItem}, active: string?, prefix: string, opts: {size: string?, key: string?, icons_only: boolean?, focused: string?, class: string?}?): any
	local o = opts or {}
	local kids = {}
	for _, it in items do
		local parts = {}
		if it.icon then table.insert(parts, ui.node("icon", {name = it.icon})) end
		if not (o.icons_only and it.icon) then table.insert(parts, ui.el("span", {class = "tp-seg-t", text = it.label})) end
		local cls = "tp-seg-btn"
		if it.id == active then cls ..= " on" end
		if o.focused == it.id then cls ..= " kfocus" end
		table.insert(kids, ui.el("button", {
			key = it.id,
			class = cls,
			title = it.title or (if o.icons_only then it.label else nil),
			aria = if o.icons_only then it.label else nil,
			attrs = {role = "tab", ["aria-selected"] = if it.id == active then "true" else "false"},
			actions = {click = prefix .. "=" .. it.id},
		}, parts))
	end
	local cls = "tp-seg"
	if o.size then cls ..= " " .. o.size end
	if o.class then cls ..= " " .. o.class end
	return ui.el("div", {key = o.key, class = cls, attrs = {role = "tablist"}}, kids)
end

-- Section headings remain in the scroll flow, without disabled-item opacity.
function c.header(key: string, label: string, value: string?, button: any?): any
	local kids: {any} = {ui.el("span", {class = "tp-sec-l", text = label})}
	if value then table.insert(kids, ui.el("span", {text = value})) end
	if button then table.insert(kids, button) end
	return ui.el("div", {key = key, class = "tp-sec-h", attrs = {role = "heading", ["aria-level"] = "2"}}, kids)
end

-- A centered empty state with optional buttons.
function c.empty(key: string, icon: string, title: string, body: string?, buttons: {any}?, focused: boolean?): any
	local kids: {any} = {
		ui.el("div", {class = "tp-empty-ic"}, {ui.node("icon", {name = icon})}),
		ui.el("div", {class = "tp-empty-t", text = title}),
	}
	if body then table.insert(kids, ui.el("div", {class = "tp-empty-d", text = body})) end
	if buttons and #buttons > 0 then table.insert(kids, ui.el("div", {class = "tp-empty-a"}, buttons)) end
	return ui.el("div", {key = key, class = "tp-empty" .. (if focused then " kfocus" else "")}, kids)
end

-- An inline status or error banner. tone: "error" | "info" | "warning".
function c.banner(key: string, tone: string, icon: string?, text: string, buttons: {any}?, spinning: boolean?): any
	local lead: any = if spinning then ui.node("spinner", {style = "ring", tone = "accent"}) else ui.node("icon", {name = icon or "info"})
	local kids: {any} = {
		ui.el("div", {class = "tp-banner-ic"}, {lead}),
		ui.node("text", {text = text, lines = 4, role = "tern-projects.banner-text"}),
	}
	if buttons and #buttons > 0 then table.insert(kids, ui.el("div", {class = "tp-banner-a"}, buttons)) end
	return ui.el("div", {key = key, class = "tp-banner " .. tone, attrs = {role = if tone == "error" then "alert" else "status"}}, kids)
end

-- A form label above a control.
function c.label(text: string, hint: string?): any
	local kids: {any} = {ui.el("span", {class = "tp-label-t", text = text})}
	if hint then table.insert(kids, ui.el("span", {class = "tp-label-h", text = hint})) end
	return ui.el("div", {class = "tp-label"}, kids)
end

-- A row of right-aligned dialog buttons.
function c.footer(buttons: {any}, leading: {any}?): any
	local kids: {any} = {}
	for _, b in leading or {} do table.insert(kids, b) end
	table.insert(kids, ui.el("div", {class = "tp-grow"}))
	for _, b in buttons do table.insert(kids, b) end
	return ui.el("div", {class = "tp-foot"}, kids)
end

-- A short muted paragraph.
function c.note(text: string, tone: string?): any
	return ui.el("div", {class = "tp-note" .. (if tone then " " .. tone else ""), text = text})
end

function c.grow(): any
	return ui.el("div", {class = "tp-grow"})
end

return c
