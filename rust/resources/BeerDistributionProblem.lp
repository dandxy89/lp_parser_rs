\* Beer_Distribution_Problem *\
Minimize
Sum_of_Transporting_Costs: 2 Route_A_1 + 4 Route_A_2 + 5 Route_A_3
 + 2 Route_A_4 + Route_A_5 + 3 Route_B_1 + Route_B_2 + 3 Route_B_3
 + 2 Route_B_4 + 3 Route_B_5
Subject To
Sum_of_Products_into_Bar1: Route_A_1 + Route_B_1 + Route_C_1 >= 500
Sum_of_Products_into_Bar2: Route_A_2 + Route_B_2 + Route_C_2 >= 900
Sum_of_Products_into_Bar3: Route_A_3 + Route_B_3 + Route_C_3 >= 1800
Sum_of_Products_into_Bar4: Route_A_4 + Route_B_4 + Route_C_4 >= 200
Sum_of_Products_into_Bar5: Route_A_5 + Route_B_5 + Route_C_5 >= 700
Sum_of_Products_out_of_Warehouse_A: Route_A_1 + Route_A_2 + Route_A_3
 + Route_A_4 + Route_A_5 <= 1000
Sum_of_Products_out_of_Warehouse_B: Route_B_1 + Route_B_2 + Route_B_3
 + Route_B_4 + Route_B_5 <= 4000
Sum_of_Products_out_of_Warehouse_C: Route_C_1 + Route_C_2 + Route_C_3
 + Route_C_4 + Route_C_5 <= 100
Bounds
 0 <= Route_A_1
 0 <= Route_A_2
 0 <= Route_A_3
 0 <= Route_A_4
 0 <= Route_A_5
 0 <= Route_B_1
 0 <= Route_B_2
 0 <= Route_B_3
 0 <= Route_B_4
 0 <= Route_B_5
 0 <= Route_C_1
 0 <= Route_C_2
 0 <= Route_C_3
 0 <= Route_C_4
 0 <= Route_C_5
Generals
Route_A_1
Route_A_2
Route_A_3
Route_A_4
Route_A_5
Route_B_1
Route_B_2
Route_B_3
Route_B_4
Route_B_5
Route_C_1
Route_C_2
Route_C_3
Route_C_4
Route_C_5
End
