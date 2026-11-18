\ Weekly production plan for two plants (v2: finance edit)
Maximize
 profit: 12 chairs + 21 tables + 30 desks - 4 overtime
Subject To
 wood:     2 chairs + 5 tables + 8 desks <= 450
 labour:   1 chairs + 2 tables + 3 desks - overtime <= 160
 open_b:   build_b = 1 -> tables + desks >= 10
 shipping: 0.0000000001 chairs + 25000 tables <= 1e10
Bounds
 0 <= overtime <= 40
Generals
 chairs tables desks
Binaries
 build_b
End
