\ Indicator constraints (CPLEX / Gurobi): the linear constraint after `->`
\ must hold whenever the binary indicator variable takes the given value.
Minimize
 obj: x + y + 10 b1 + 5 b2
Subject To
 demand: x + y >= 4
 cap_x: b1 = 1 -> x <= 3
 cap_y: b2 = 0 -> y <= 0
 b1 = 1 -> 2 <= x
Lazy Constraints
 lazy_ind: b2 = 1 -> x + y <= 10
Binaries
 b1 b2
End
