\ Gurobi general constraints: `resultant = FUNCTION ( arguments )`, with the
\ parentheses and commas separated by spaces as Gurobi writes them.
Maximize
 obj: r1 + r2 - r3 + b3
Subject To
 c1: x1 + x2 <= 10
 c2: x1 - x2 >= -4
Bounds
 -5 <= x1 <= 5
 x2 <= 8
Binaries
 b1 b2 b3 b4
General Constraints
 gc_max: r1 = MAX ( x1 , x2 , 3 )
 gc_min: r2 = MIN ( x1 , x2 , -1.5 )
 gc_abs: r3 = ABS ( x1 )
 gc_and: b3 = AND ( b1 , b2 )
 b4 = OR ( b1 , b2 )
End
