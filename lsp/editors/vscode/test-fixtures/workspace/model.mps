* A small free-format MPS model
NAME          TINY
ROWS
 N  obj
 L  c1
 G  c2
COLUMNS
    MARKER                 'MARKER'                 'INTORG'
    x         obj                  1.0   c1                   1.0
    x         c2                   1.0
    MARKER                 'MARKER'                 'INTEND'
    y         obj                  2.0   c1                   1.0
RHS
    RHS       c1                   4.0   c2                   1.0
BOUNDS
 UP BND       x                    3.0
 FR BND       y
ENDATA
