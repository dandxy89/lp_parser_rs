\ Gurobi multi-objective syntax: `multi-objectives` after the sense, then per
\ objective optional Priority / Weight / AbsTol / RelTol attributes followed
\ by the expression.
Minimize multi-objectives
 Cost: Priority=2 Weight=1 AbsTol=0.5 RelTol=0.01
  3 x + 2 y
 Time: Priority=1 Weight=-0.5
  x + 4 y
 Plain:
  x - y
Subject To
 c1: x + y >= 2
End
