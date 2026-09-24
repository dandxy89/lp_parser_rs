\ CPLEX lazy constraints and user cuts: ordinary constraints that a solver
\ may add lazily (lazy) or use only to tighten the relaxation (user cuts).
Maximize
 obj: 3 x + 2 y + z
Subject To
 c1: x + y + z <= 10
 c2: x - y >= -2
Lazy Constraints
 l1: x + 2 y <= 12
 l2: 1 <= x - z <= 6
User Cuts
 u1: x + y <= 9
 2 x + z <= 15
Bounds
 x <= 8
Generals
 x y
End
