\* copynumber *\
Minimize
OBJ: PenaltyTree_48
Subject To
_C1: - A_0 + A_3 - over_0 <= 0
_C10: - B_3 + B_7 + under_9 >= 0
_C11: - D_0 + D_3 - over_10 <= 0
_C12: - D_0 + D_3 + under_11 >= 0
_C13: - N_0 + N_3 - over_12 <= 0
_C14: - N_0 + N_3 + under_13 >= 0
_C15: - N_3 + N_7 - over_14 <= 0
_C16: - N_3 + N_7 + under_15 >= 0
_C17: A_7 - B_7 - over_16 <= 0
_C18: A_7 - B_7 + under_17 >= 0
_C19: - A_7 + N_7 - over_18 <= 0
_C2: - A_0 + A_3 + under_1 >= 0
_C20: - A_7 + N_7 + under_19 >= 0
_C21: C_7 - N_7 - over_20 <= 0
_C22: C_7 - N_7 + under_21 >= 0
_C23: B_7 - C_7 - over_22 <= 0
_C24: B_7 - C_7 + under_23 >= 0
_C25: B_3 - N_3 - over_24 <= 0
_C26: B_3 - N_3 + under_25 >= 0
_C27: A_3 - B_3 - over_26 <= 0
_C28: A_3 - B_3 + under_27 >= 0
_C29: - A_3 + N_3 - over_28 <= 0
_C3: - A_3 + A_7 - over_2 <= 0
_C30: - A_3 + N_3 + under_29 >= 0
_C31: A_0 - N_0 - over_30 <= 0
_C32: A_0 - N_0 + under_31 >= 0
_C33: - A_0 + D_0 - over_32 <= 0
_C34: - A_0 + D_0 + under_33 >= 0
_C35: B_0 - D_0 - over_34 <= 0
_C36: B_0 - D_0 + under_35 >= 0
_C37: - B_0 + N_0 - over_36 <= 0
_C38: - B_0 + N_0 + under_37 >= 0
_C39: D_3 - over_38 <= 1.24660824696
_C4: - A_3 + A_7 + under_3 >= 0
_C40: D_3 + under_39 >= 1.24660824696
_C41: A_7 + B_7 + C_7 + N_7 - over_40 <= 9.80665154278
_C42: A_7 + B_7 + C_7 + N_7 + under_41 >= 9.80665154278
_C43: C_0 - over_42 <= 2.06580795211
_C44: C_0 + under_43 >= 2.06580795211
_C45: A_3 + B_3 + N_3 - over_44 <= 7.1056670077
_C46: A_3 + B_3 + N_3 + under_45 >= 7.1056670077
_C47: A_0 + B_0 + D_0 + N_0 - over_46 <= 7.89518556411
_C48: A_0 + B_0 + D_0 + N_0 + under_47 >= 7.89518556411
_C49: - PenaltyTree_48 + 0.5 over_0 + 0.5 over_10 + 0.5 over_12 + 0.5 over_14
 + 0.02 over_16 + 0.02 over_18 + 0.5 over_2 + 0.02 over_20 + 0.02 over_22
 + 0.02 over_24 + 0.02 over_26 + 0.02 over_28 + 0.02 over_30 + 0.02 over_32
 + 0.02 over_34 + 0.02 over_36 + 4 over_38 + 0.5 over_4 + 4 over_40
 + 4 over_42 + 4 over_44 + 4 over_46 + 0.5 over_6 + 0.5 over_8 + 0.5 under_1
 + 0.5 under_11 + 0.5 under_13 + 0.5 under_15 + 0.02 under_17 + 0.02 under_19
 + 0.02 under_21 + 0.02 under_23 + 0.02 under_25 + 0.02 under_27
 + 0.02 under_29 + 0.5 under_3 + 0.02 under_31 + 0.02 under_33 + 0.02 under_35
 + 0.02 under_37 + 4 under_39 + 4 under_41 + 4 under_43 + 4 under_45
 + 4 under_47 + 0.5 under_5 + 0.5 under_7 + 0.5 under_9 = 0
_C5: - C_0 + C_7 - over_4 <= 0
_C6: - C_0 + C_7 + under_5 >= 0
_C7: - B_0 + B_3 - over_6 <= 0
_C8: - B_0 + B_3 + under_7 >= 0
_C9: - B_3 + B_7 - over_8 <= 0
Bounds
0 <= A_0 <= 4
0 <= A_3 <= 4
0 <= A_7 <= 4
0 <= B_0 <= 4
0 <= B_3 <= 4
0 <= B_7 <= 4
0 <= C_0 <= 4
0 <= C_7 <= 4
0 <= D_0 <= 4
0 <= D_3 <= 4
0 <= N_0 <= 4
0 <= N_3 <= 4
0 <= N_7 <= 4
PenaltyTree_48 free
Generals
A_0
A_3
A_7
B_0
B_3
B_7
C_0
C_7
D_0
D_3
N_0
N_3
N_7
End
