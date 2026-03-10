NAME        cplex
OBJSENSE
  MAX
ROWS
 N  obj
 L  c1
 L  c2
 E  c3
COLUMNS
    x1        obj       1
    x1        c1        -1
    x1        c2        1
    x2        obj       2
    x2        c1        1
    x2        c2        -3
    x2        c3        1
    x3        obj       3
    x3        c1        1
    x3        c2        1
    MARK0000  'MARKER'                 'INTORG'
    x4        obj       1
    x4        c1        10
    x4        c3        -3.5
    MARK0001  'MARKER'                 'INTEND'
RHS
    RHS_V     c1        20
    RHS_V     c2        30
BOUNDS
 UP BOUND     x1        40
 LI BOUND     x4        2
 UI BOUND     x4        3
ENDATA
