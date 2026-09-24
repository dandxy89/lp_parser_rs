; Variable definitions in bounds/type sections
(bound_declaration
  (identifier) @local.definition)

(generals_section
  (identifier) @local.definition)

(integers_section
  (identifier) @local.definition)

(binaries_section
  (identifier) @local.definition)

(semi_continuous_section
  (identifier) @local.definition)

; Variable references in objectives and constraints
(term
  (identifier) @local.reference)

(quadratic_term
  (identifier) @local.reference)

(indicator
  (identifier) @local.reference)

(general_constraint
  (identifier) @local.reference)

(sos_entry
  (identifier) @local.reference)

; Scopes
(source_file) @local.scope
