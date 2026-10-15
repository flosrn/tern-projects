--!strict
local tf = require("./textfield")
local input = {}

function input.key(field: tf.Field, selected_all: boolean, k: Key, os: string): (boolean, boolean)
	local name = k.name
	if name == "a" and (if os == "macos" then k.meta else k.ctrl) then
		field.cur = #field.text
		return true, field.text ~= ""
	end
	if selected_all and (name == "left" or name == "right" or name == "home" or name == "end") then
		field.cur = (name == "left" or name == "home") and 0 or #field.text
		return true, false
	end
	if selected_all and (name == "backspace" or name == "delete" or name == "paste" or (k.text ~= nil and k.text ~= "" and not (k.ctrl or k.meta))) then
		tf.set(field, "")
		if name ~= "backspace" and name ~= "delete" then tf.key(field, k, false) end
		return true, false
	end
	if (k.meta and name == "backspace") or (k.ctrl and name == "u") then
		local suffix = string.sub(field.text, field.cur + 1)
		tf.set(field, suffix)
		field.cur = 0
		return true, false
	end
	if (k.ctrl or k.alt) and name == "backspace" then
		tf.word_back(field)
		return true, false
	end
	local handled = tf.key(field, k, false)
	return handled, if handled then false else selected_all
end

-- Offsets are UTF-16, whereas textfield cursors are UTF-8 byte offsets.
local function byte_offset(text: string, offset: any): number?
	if type(offset) ~= "number" or offset < 0 or offset % 1 ~= 0 then return nil end
	local units = 0
	for pos, cp in utf8.codes(text) do
		if units == offset then return pos - 1 end
		units += if cp > 0xffff then 2 else 1
		if units > offset then return nil end
	end
	return if units == offset then #text else nil
end

function input.edit(field: tf.Field, ev: UiEvent): boolean
	local from = byte_offset(field.text, ev.from)
	local to = byte_offset(field.text, ev.to)
	if from == nil or to == nil or from > to or type(ev.text) ~= "string" then return false end
	if type(ev.len) == "number" and ev.len ~= tf.utf16({ text = field.text, cur = #field.text }) then return false end
	local raw = string.sub(field.text, 1, from) .. ev.text .. string.sub(field.text, to + 1)
	local cursor = byte_offset(raw, ev.cursor)
	if cursor == nil then return false end
	local function single_line(text: string): string
		return (string.gsub(string.gsub(text, "\r\n", " "), "[\r\n]", " "))
	end
	field.text = single_line(raw)
	field.cur = #single_line(string.sub(raw, 1, cursor))
	return true
end
return input
