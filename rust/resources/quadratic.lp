\ Quadratic objective and constraints (CPLEX / Gurobi syntax). An objective's
\ quadratic block is always divided by 2; a constraint's is not.
Minimize
 obj: 2 x + 3 y + [ x ^ 2 + 4 x * y + 2 y^2 ] / 2 + 1
Subject To
 c1: x + y >= 1
 q1: x + [ x ^ 2 + y ^ 2 ] <= 4
 q2: [ - x * y + 3 z ^ 2 ] >= -2
 10 >= [ z * x ] + y
Bounds
 -5 <= z <= 5
End
