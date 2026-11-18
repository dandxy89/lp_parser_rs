\ Weekly production plan for two plants
Maximize
 profit: 12 chairs + 18 tables + 30 desks - 4 overtime
Subject To
 wood:     2 chairs + 5 tables + 8 desks <= 400
 labour:   1 chairs + 2 tables + 3 desks - overtime <= 160
 demand:   desks <= 30
 open_b:   build_b = 1 -> tables + desks >= 10
Bounds
 0 <= overtime <= 40
Generals
 chairs tables desks
Binaries
 build_b
End
