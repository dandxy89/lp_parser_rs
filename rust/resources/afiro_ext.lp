\Problem name: afiro_ext.mps

Minimize
 OBJ1: - 0.4 X02 - 0.32 X14 - 0.6 X23 - 0.48 X36 + 10 X39
 OBJ2: X03 + 2.5 X06 - 5 X08
 OBJ3: -10.4 X03 - 2.5 X07 + 2.1 X10
 OBJECTIV: - 0.23519 PBOSHNL0 - 0.23519 PBOSHNL1 + 0.02469 PBOSHNL7
           + 0.02665 PBOSHNL8 - 0.13092 PBOSLAX0 - 0.13092 PBOSLAX1
           + 0.03462 PBOSLAX7 - 0.12596 PBOSSEA0 - 0.12596 PBOSSEA1
           - 0.12596 PBOSSEA2 - 0.13481 PBOSSFO0 - 0.13481 PBOSSFO1
           + 0.05629 PBOSTPE1 + 0.0528 PBOSTPE2 + 0.04525 PBOSTYO1
Subject To
 R09: - X01 + X02 + X03  = 0
 R10: - 1.06 X01 + X04  = 0
 X05: X01 <= 80
 X21: - X02 + 1.4 X14 <= 0
 R12: - X06 - X07 - X08 - X09 + X14 + X15  = 0
 R13: - 1.06 X06 - 1.06 X07 - 0.96 X08 - 0.86 X09 + X16  = 0
 X17: X06 - X10 <= 80
 X18: X07 - X11 <= 0
 X19: X08 - X12 <= 0
 X20: X09 - X13 <= 0
 R19: - X22 + X23 + X24 + X25  = 0
 R20: - 0.43 X22 + X26  = 0
 X27: X22 <= 500
 X44: - X23 + 1.4 X36 <= 0
 R22: - 0.43 X28 - 0.43 X29 - 0.39 X30 - 0.37 X31 + X38  = 0
 R23: X28 + X29 + X30 + X31 - X36 + X37 + X39  = 44
 X40: X28 - X32 <= 500
 X41: X29 - X33 <= 0
 X42: X30 - X34 <= 0
 X43: X31 - X35 <= 0
 X45: 2.364 X10 + 2.386 X11 + 2.408 X12 + 2.429 X13 - X25 + 2.191 X32
      + 2.219 X33 + 2.249 X34 + 2.279 X35 <= 0
 X46: - X03 + 0.109 X22 <= 0
 X47: - X15 + 0.109 X28 + 0.108 X29 + 0.108 X30 + 0.107 X31 <= 0
 X48: 0.301 X01 - X24 <= 0
 X49: 0.301 X06 + 0.313 X07 + 0.313 X08 + 0.326 X09 - X37 <= 0
 X50: X04 + X26 <= 310
 X51: X16 + X38 <= 1300
Bounds
0 <= X01 <= 1
X02 <= 1.2
X06 >= 3
X07 free
X09 >= -Inf
Integers
X31
X32
Generals
X16
Binaries
X38
End
