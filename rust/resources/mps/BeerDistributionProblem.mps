NAME        BeerDistributionProblem
ROWS
 N  Sum_of_Transporting_Costs
 G  Sum_of_Products_into_Bar1
 G  Sum_of_Products_into_Bar2
 G  Sum_of_Products_into_Bar3
 G  Sum_of_Products_into_Bar4
 G  Sum_of_Products_into_Bar5
 L  Sum_of_Products_out_of_Warehouse_A
 L  Sum_of_Products_out_of_Warehouse_B
 L  Sum_of_Products_out_of_Warehouse_C
COLUMNS
    MARK0000  'MARKER'                 'INTORG'
    Route_A_1  Sum_of_Transporting_Costs  2
    Route_A_1  Sum_of_Products_into_Bar1  1
    Route_A_1  Sum_of_Products_out_of_Warehouse_A  1
    Route_A_2  Sum_of_Transporting_Costs  4
    Route_A_2  Sum_of_Products_into_Bar2  1
    Route_A_2  Sum_of_Products_out_of_Warehouse_A  1
    Route_A_3  Sum_of_Transporting_Costs  5
    Route_A_3  Sum_of_Products_into_Bar3  1
    Route_A_3  Sum_of_Products_out_of_Warehouse_A  1
    Route_A_4  Sum_of_Transporting_Costs  2
    Route_A_4  Sum_of_Products_into_Bar4  1
    Route_A_4  Sum_of_Products_out_of_Warehouse_A  1
    Route_A_5  Sum_of_Transporting_Costs  1
    Route_A_5  Sum_of_Products_into_Bar5  1
    Route_A_5  Sum_of_Products_out_of_Warehouse_A  1
    Route_B_1  Sum_of_Transporting_Costs  3
    Route_B_1  Sum_of_Products_into_Bar1  1
    Route_B_1  Sum_of_Products_out_of_Warehouse_B  1
    Route_B_2  Sum_of_Transporting_Costs  1
    Route_B_2  Sum_of_Products_into_Bar2  1
    Route_B_2  Sum_of_Products_out_of_Warehouse_B  1
    Route_B_3  Sum_of_Transporting_Costs  3
    Route_B_3  Sum_of_Products_into_Bar3  1
    Route_B_3  Sum_of_Products_out_of_Warehouse_B  1
    Route_B_4  Sum_of_Transporting_Costs  2
    Route_B_4  Sum_of_Products_into_Bar4  1
    Route_B_4  Sum_of_Products_out_of_Warehouse_B  1
    Route_B_5  Sum_of_Transporting_Costs  3
    Route_B_5  Sum_of_Products_into_Bar5  1
    Route_B_5  Sum_of_Products_out_of_Warehouse_B  1
    Route_C_1  Sum_of_Products_into_Bar1  1
    Route_C_1  Sum_of_Products_out_of_Warehouse_C  1
    Route_C_2  Sum_of_Products_into_Bar2  1
    Route_C_2  Sum_of_Products_out_of_Warehouse_C  1
    Route_C_3  Sum_of_Products_into_Bar3  1
    Route_C_3  Sum_of_Products_out_of_Warehouse_C  1
    Route_C_4  Sum_of_Products_into_Bar4  1
    Route_C_4  Sum_of_Products_out_of_Warehouse_C  1
    Route_C_5  Sum_of_Products_into_Bar5  1
    Route_C_5  Sum_of_Products_out_of_Warehouse_C  1
    MARK0001  'MARKER'                 'INTEND'
RHS
    RHS_V     Sum_of_Products_into_Bar1  500
    RHS_V     Sum_of_Products_into_Bar2  900
    RHS_V     Sum_of_Products_into_Bar3  1800
    RHS_V     Sum_of_Products_into_Bar4  200
    RHS_V     Sum_of_Products_into_Bar5  700
    RHS_V     Sum_of_Products_out_of_Warehouse_A  1000
    RHS_V     Sum_of_Products_out_of_Warehouse_B  4000
    RHS_V     Sum_of_Products_out_of_Warehouse_C  100
BOUNDS
 LI BOUND     Route_A_1  0
 LI BOUND     Route_A_2  0
 LI BOUND     Route_A_3  0
 LI BOUND     Route_A_4  0
 LI BOUND     Route_A_5  0
 LI BOUND     Route_B_1  0
 LI BOUND     Route_B_2  0
 LI BOUND     Route_B_3  0
 LI BOUND     Route_B_4  0
 LI BOUND     Route_B_5  0
 LI BOUND     Route_C_1  0
 LI BOUND     Route_C_2  0
 LI BOUND     Route_C_3  0
 LI BOUND     Route_C_4  0
 LI BOUND     Route_C_5  0
ENDATA
