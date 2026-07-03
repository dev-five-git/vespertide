; Vespertide Model — YAML highlights.

; Structural keys — strongest emphasis.
((block_mapping_pair
   key: (flow_node) @keyword)
 (#match? @keyword "^(\\$schema|name|columns|constraints|indexes|foreign_key|primary_key)$"))
((flow_pair
   key: (flow_node) @keyword)
 (#match? @keyword "^(\\$schema|name|columns|constraints|indexes|foreign_key|primary_key)$"))

; Column-modifier keys.
((block_mapping_pair
   key: (flow_node) @attribute)
 (#match? @attribute "^(type|kind|nullable|unique|index|default|comment|length|precision|scale|values|custom_type|ref_table|ref_columns|on_delete|on_update)$"))
((flow_pair
   key: (flow_node) @attribute)
 (#match? @attribute "^(type|kind|nullable|unique|index|default|comment|length|precision|scale|values|custom_type|ref_table|ref_columns|on_delete|on_update)$"))

; Generic keys.
(block_mapping_pair
  key: (flow_node) @property)
(flow_pair
  key: (flow_node) @property)

; String values.
[
  (double_quote_scalar)
  (single_quote_scalar)
  (block_scalar)
  (string_scalar)
] @string

(escape_sequence) @string.escape

(boolean_scalar) @boolean
(null_scalar) @constant.builtin

(integer_scalar) @number
(float_scalar) @number

[
  ","
  ":"
  "-"
  "?"
  "|"
  ">"
] @punctuation.delimiter

[
  "["
  "]"
  "{"
  "}"
] @punctuation.bracket

(comment) @comment

(anchor_name) @label
(alias_name) @label

(tag) @type

(yaml_directive) @keyword
(tag_directive) @keyword
(reserved_directive) @keyword
