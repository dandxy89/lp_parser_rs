NAME        complex_names
ROWS
 N  total_cost_2025
 L  Capacity_Constraint_1
 G  Demand_Region_X
COLUMNS
    Production_A_1  total_cost_2025  100
    Production_A_1  Capacity_Constraint_1  1
    Production_B_2  total_cost_2025  200
    Production_B_2  Capacity_Constraint_1  1
    Transport_X_Y  total_cost_2025  150
    Transport_X_Y  Demand_Region_X  1
    Storage_Facility_1  total_cost_2025  300
    Storage_Facility_1  Demand_Region_X  1
RHS
    RHS_V     Capacity_Constraint_1  500
    RHS_V     Demand_Region_X  200
BOUNDS
 UP BOUND     Production_A_1  300
 UP BOUND     Production_B_2  400
 UP BOUND     Storage_Facility_1  1000
ENDATA
