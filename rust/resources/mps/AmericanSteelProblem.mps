NAME        AmericanSteelProblem
ROWS
 N  Total_Cost_of_Transport
 G  Steel_Flow_Conservation_in_Node_Albany
 G  Steel_Flow_Conservation_in_Node_Chicago
 G  Steel_Flow_Conservation_in_Node_Cincinatti
 G  Steel_Flow_Conservation_in_Node_Gary
 G  Steel_Flow_Conservation_in_Node_Houston
 G  Steel_Flow_Conservation_in_Node_Kansas_City
 G  Steel_Flow_Conservation_in_Node_Pittsburgh
 G  Steel_Flow_Conservation_in_Node_Tempe
 G  Steel_Flow_Conservation_in_Node_Youngstown
COLUMNS
    MARK0000  'MARKER'                 'INTORG'
    Route_('Chicago',_'Gary')  Total_Cost_of_Transport  0.12
    Route_('Chicago',_'Gary')  Steel_Flow_Conservation_in_Node_Chicago  -1
    Route_('Chicago',_'Gary')  Steel_Flow_Conservation_in_Node_Gary  1
    Route_('Chicago',_'Tempe')  Total_Cost_of_Transport  0.6
    Route_('Chicago',_'Tempe')  Steel_Flow_Conservation_in_Node_Chicago  -1
    Route_('Chicago',_'Tempe')  Steel_Flow_Conservation_in_Node_Tempe  1
    Route_('Cincinatti',_'Albany')  Total_Cost_of_Transport  0.35
    Route_('Cincinatti',_'Albany')  Steel_Flow_Conservation_in_Node_Albany  1
    Route_('Cincinatti',_'Albany')  Steel_Flow_Conservation_in_Node_Cincinatti  -1
    Route_('Cincinatti',_'Houston')  Total_Cost_of_Transport  0.55
    Route_('Cincinatti',_'Houston')  Steel_Flow_Conservation_in_Node_Cincinatti  -1
    Route_('Cincinatti',_'Houston')  Steel_Flow_Conservation_in_Node_Houston  1
    Route_('Kansas_City',_'Houston')  Total_Cost_of_Transport  0.375
    Route_('Kansas_City',_'Houston')  Steel_Flow_Conservation_in_Node_Houston  1
    Route_('Kansas_City',_'Houston')  Steel_Flow_Conservation_in_Node_Kansas_City  -1
    Route_('Kansas_City',_'Tempe')  Total_Cost_of_Transport  0.65
    Route_('Kansas_City',_'Tempe')  Steel_Flow_Conservation_in_Node_Kansas_City  -1
    Route_('Kansas_City',_'Tempe')  Steel_Flow_Conservation_in_Node_Tempe  1
    Route_('Pittsburgh',_'Chicago')  Total_Cost_of_Transport  0.4
    Route_('Pittsburgh',_'Chicago')  Steel_Flow_Conservation_in_Node_Chicago  1
    Route_('Pittsburgh',_'Chicago')  Steel_Flow_Conservation_in_Node_Pittsburgh  -1
    Route_('Pittsburgh',_'Cincinatti')  Total_Cost_of_Transport  0.35
    Route_('Pittsburgh',_'Cincinatti')  Steel_Flow_Conservation_in_Node_Cincinatti  1
    Route_('Pittsburgh',_'Cincinatti')  Steel_Flow_Conservation_in_Node_Pittsburgh  -1
    Route_('Pittsburgh',_'Gary')  Total_Cost_of_Transport  0.45
    Route_('Pittsburgh',_'Gary')  Steel_Flow_Conservation_in_Node_Gary  1
    Route_('Pittsburgh',_'Gary')  Steel_Flow_Conservation_in_Node_Pittsburgh  -1
    Route_('Pittsburgh',_'Kansas_City')  Total_Cost_of_Transport  0.45
    Route_('Pittsburgh',_'Kansas_City')  Steel_Flow_Conservation_in_Node_Kansas_City  1
    Route_('Pittsburgh',_'Kansas_City')  Steel_Flow_Conservation_in_Node_Pittsburgh  -1
    Route_('Youngstown',_'Albany')  Total_Cost_of_Transport  0.5
    Route_('Youngstown',_'Albany')  Steel_Flow_Conservation_in_Node_Albany  1
    Route_('Youngstown',_'Albany')  Steel_Flow_Conservation_in_Node_Youngstown  -1
    Route_('Youngstown',_'Chicago')  Total_Cost_of_Transport  0.375
    Route_('Youngstown',_'Chicago')  Steel_Flow_Conservation_in_Node_Chicago  1
    Route_('Youngstown',_'Chicago')  Steel_Flow_Conservation_in_Node_Youngstown  -1
    Route_('Youngstown',_'Cincinatti')  Total_Cost_of_Transport  0.35
    Route_('Youngstown',_'Cincinatti')  Steel_Flow_Conservation_in_Node_Cincinatti  1
    Route_('Youngstown',_'Cincinatti')  Steel_Flow_Conservation_in_Node_Youngstown  -1
    Route_('Youngstown',_'Kansas_City')  Total_Cost_of_Transport  0.45
    Route_('Youngstown',_'Kansas_City')  Steel_Flow_Conservation_in_Node_Kansas_City  1
    Route_('Youngstown',_'Kansas_City')  Steel_Flow_Conservation_in_Node_Youngstown  -1
    MARK0001  'MARKER'                 'INTEND'
RHS
    RHS_V     Steel_Flow_Conservation_in_Node_Albany  3000
    RHS_V     Steel_Flow_Conservation_in_Node_Gary  6000
    RHS_V     Steel_Flow_Conservation_in_Node_Houston  7000
    RHS_V     Steel_Flow_Conservation_in_Node_Pittsburgh  -15000
    RHS_V     Steel_Flow_Conservation_in_Node_Tempe  4000
    RHS_V     Steel_Flow_Conservation_in_Node_Youngstown  -10000
BOUNDS
 UI BOUND     Route_('Chicago',_'Gary')  4000
 UI BOUND     Route_('Chicago',_'Tempe')  2000
 LI BOUND     Route_('Cincinatti',_'Albany')  1000
 UI BOUND     Route_('Cincinatti',_'Albany')  5000
 UI BOUND     Route_('Cincinatti',_'Houston')  6000
 UI BOUND     Route_('Kansas_City',_'Houston')  4000
 UI BOUND     Route_('Kansas_City',_'Tempe')  4000
 UI BOUND     Route_('Pittsburgh',_'Chicago')  4000
 UI BOUND     Route_('Pittsburgh',_'Cincinatti')  2000
 UI BOUND     Route_('Pittsburgh',_'Gary')  2000
 LI BOUND     Route_('Pittsburgh',_'Kansas_City')  2000
 UI BOUND     Route_('Pittsburgh',_'Kansas_City')  3000
 UI BOUND     Route_('Youngstown',_'Albany')  1000
 UI BOUND     Route_('Youngstown',_'Chicago')  5000
 UI BOUND     Route_('Youngstown',_'Cincinatti')  3000
 LI BOUND     Route_('Youngstown',_'Kansas_City')  1000
 UI BOUND     Route_('Youngstown',_'Kansas_City')  5000
ENDATA
