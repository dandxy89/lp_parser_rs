; Copied from https://github.com/dandxy89/treesitter-lp/blob/main/queries/folds.scm (not exported by the crate).
; Fold section bodies
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
] @fold
