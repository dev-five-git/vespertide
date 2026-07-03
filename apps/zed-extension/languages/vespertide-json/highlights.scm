; Vespertide Model — JSON highlights.
;
; Strategy: distinguish keys from value strings, and give Vespertide-specific
; structural keys (name/columns/constraints/...) extra emphasis so the schema
; layout reads at a glance instead of looking like flat text.

; ----------------------------------------------------------------------------
; Structural keys — strongest emphasis.
((pair
   key: (string) @keyword)
 (#match? @keyword "^\"(\\$schema|name|columns|constraints|indexes|foreign_key|primary_key)\"$"))

; Column-modifier keys — softer than structural.
((pair
   key: (string) @attribute)
 (#match? @attribute "^\"(type|kind|nullable|unique|index|default|comment|length|precision|scale|values|custom_type|ref_table|ref_columns|on_delete|on_update)\"$"))

; Generic pair keys.
(pair
  key: (string) @property)

; ----------------------------------------------------------------------------
; Values.
(pair
  value: (string) @string)

(array
  (string) @string)

(number) @number

(true) @boolean
(false) @boolean
(null) @constant.builtin

(escape_sequence) @string.escape

; Punctuation.
[
  ","
  ":"
] @punctuation.delimiter

[
  "["
  "]"
  "{"
  "}"
] @punctuation.bracket

(ERROR) @comment.error
