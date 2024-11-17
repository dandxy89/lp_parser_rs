\ Test case for scientific notation in coefficients
MINIMIZE
obj: 1e-03 x1 + 2.5e+02 x2 + 3.14e-02 x3 - 1.2E+01 x4
Subject To
 c1: 2e-01 x1 + 1.5e+00 x2 + 3E-03 x3 <= 100
 c2: 1.23e-02 x1 - 4.56E+01 x2 + 7.89e-03 x3 >= 0
Bounds
 0 <= x1 <= 1e+03
 0 <= x2 <= 2e+02
 0 <= x3 <= 3.5e+01
 0 <= x4 <= 4E+02
End
