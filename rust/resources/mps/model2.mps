NAME        model2
ROWS
 N  obj
 G  CON1
 G  CON2
 L  CON3
 L  CON4
COLUMNS
    MARK0000  'MARKER'                 'INTORG'
    V4        obj       -1
    MARK0001  'MARKER'                 'INTEND'
    V5        obj       1
    V5        CON4      1
    V1        CON1      1
    V2        CON2      1
    V3        CON3      1
    V6        CON4      1
    V7        CON4      1
    V8        obj       0
RHS
    RHS_V     CON2      2
    RHS_V     CON3      2.5
    RHS_V     CON4      1
BOUNDS
 LI BOUND     V4        5.5
 UP BOUND     V5        1
 MI BOUND     V1
 UP BOUND     V1        3
 MI BOUND     V2
 UP BOUND     V2        3
 MI BOUND     V3
 UP BOUND     V3        3
 UP BOUND     V6        1
 UP BOUND     V7        1
 BV BOUND     V8
ENDATA
