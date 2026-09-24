\ Semi-integer variables (CPLEX semantics): a variable listed in both a
\ generals/integers section and the semi-continuous section is either zero
\ or an integer within its bounds.
Maximize
 obj: - x + 0.5 y + 2 z
Subject To
 c1: y - x <= 0.5
 c2: z + x + w <= 7.5
Bounds
 2 <= x <= 10
 z <= 4
 3 <= w <= 9
Generals
 x z
Semi-Continuous
 x w
Integers
 w
End
