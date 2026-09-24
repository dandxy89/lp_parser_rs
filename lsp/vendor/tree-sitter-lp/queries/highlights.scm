; Keywords
(sense) @keyword.directive

(subject_to_keyword) @keyword.conditional

[
  (bounds_keyword)
  (generals_keyword)
  (integers_keyword)
  (binaries_keyword)
  (semi_continuous_keyword)
  (sos_keyword)
  (lazy_constraints_keyword)
  (user_cuts_keyword)
  (general_constraints_keyword)
] @keyword.type

[
  (multi_objectives_keyword)
  (free_keyword)
] @keyword.modifier

(end_marker) @keyword.return

(sos_type) @type

; Operators and punctuation
(comparison_operator) @operator

[
  "="
  "->"
  "+"
  "-"
  "/"
  "^"
  "*"
] @operator

[
  "["
  "]"
  "("
  ")"
] @punctuation.bracket

[
  ":"
  "::"
  ","
] @punctuation.delimiter

; Literals
(number) @number

(infinity) @constant.builtin

; Names (objective/constraint/SOS names are aliased, so bare identifiers are variables)
[
  (objective_name)
  (constraint_name)
  (sos_name)
] @label

(attribute_name) @property

(function_name) @function.builtin

(identifier) @variable

; Comments
[
  (line_comment)
  (block_comment)
] @comment
