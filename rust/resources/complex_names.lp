\\ Testing complex variable/constraint names and whitespace handling
Minimize
 total_cost_2025: 100 Production_A_1 + 200 Production_B_2
    + 150 Transport_X_Y
    + 300 Storage_Facility_1






Subject To
 Capacity_Constraint_1:
    Production_A_1 + Production_B_2 <= 500
 Demand_Region_X:
    Transport_X_Y + Storage_Facility_1 >= 200




Bounds
 0 <= Production_A_1 <= 300
 0 <= Production_B_2 <= 400
 0 <= Transport_X_Y <= +infinity
 0 <= Storage_Facility_1 <= 1000
End
