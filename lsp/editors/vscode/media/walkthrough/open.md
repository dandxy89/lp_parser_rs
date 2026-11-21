# Open or create an LP file

The extension recognises files ending in `.lp`. Open one from the Explorer, or use **New LP File** to start with an empty editor.

A minimal model looks like this:

```lp
Maximize
 obj: 3 x + 2 y
Subject To
 c1: x + y <= 10
Bounds
 x <= 8
End
```

Syntax errors and analysis findings appear as you type. MPS files (`.mps`) get highlighting too, and *LP: Convert from MPS* turns one into LP.
