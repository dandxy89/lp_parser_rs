; Indent inside section bodies
[
  (objectives_section)
  (constraints_section)
  (bounds_section)
  (generals_section)
  (integers_section)
  (binaries_section)
  (semi_continuous_section)
  (sos_section)
  (lazy_constraints_section)
  (user_cuts_section)
  (general_constraints_section)
] @indent.begin

; Dedent at section boundaries
(sense) @indent.branch

(end_marker) @indent.branch
