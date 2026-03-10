NAME        ComputerPlantProblem
ROWS
 N  Total_Costs
 G  Sum_of_Products_into_Stores_Barstow
 G  Sum_of_Products_into_Stores_Dallas
 G  Sum_of_Products_into_Stores_San_Diego
 G  Sum_of_Products_into_Stores_Tucson
 L  Sum_of_Products_out_of_Plant_Denver
 L  Sum_of_Products_out_of_Plant_Los_Angeles
 L  Sum_of_Products_out_of_Plant_Phoenix
 L  Sum_of_Products_out_of_Plant_San_Francisco
COLUMNS
    MARK0000  'MARKER'                 'INTORG'
    BuildaPlant_Denver  Total_Costs  70000
    BuildaPlant_Denver  Sum_of_Products_out_of_Plant_Denver  -2000
    BuildaPlant_Los_Angeles  Total_Costs  70000
    BuildaPlant_Los_Angeles  Sum_of_Products_out_of_Plant_Los_Angeles  -2000
    BuildaPlant_Phoenix  Total_Costs  65000
    BuildaPlant_Phoenix  Sum_of_Products_out_of_Plant_Phoenix  -1700
    BuildaPlant_San_Francisco  Total_Costs  70000
    BuildaPlant_San_Francisco  Sum_of_Products_out_of_Plant_San_Francisco  -1700
    Route_Denver_Barstow  Total_Costs  8
    Route_Denver_Barstow  Sum_of_Products_into_Stores_Barstow  1
    Route_Denver_Barstow  Sum_of_Products_out_of_Plant_Denver  1
    Route_Denver_Dallas  Total_Costs  5
    Route_Denver_Dallas  Sum_of_Products_into_Stores_Dallas  1
    Route_Denver_Dallas  Sum_of_Products_out_of_Plant_Denver  1
    Route_Denver_San_Diego  Total_Costs  9
    Route_Denver_San_Diego  Sum_of_Products_into_Stores_San_Diego  1
    Route_Denver_San_Diego  Sum_of_Products_out_of_Plant_Denver  1
    Route_Denver_Tucson  Total_Costs  6
    Route_Denver_Tucson  Sum_of_Products_into_Stores_Tucson  1
    Route_Denver_Tucson  Sum_of_Products_out_of_Plant_Denver  1
    Route_Los_Angeles_Barstow  Total_Costs  7
    Route_Los_Angeles_Barstow  Sum_of_Products_into_Stores_Barstow  1
    Route_Los_Angeles_Barstow  Sum_of_Products_out_of_Plant_Los_Angeles  1
    Route_Los_Angeles_Dallas  Total_Costs  10
    Route_Los_Angeles_Dallas  Sum_of_Products_into_Stores_Dallas  1
    Route_Los_Angeles_Dallas  Sum_of_Products_out_of_Plant_Los_Angeles  1
    Route_Los_Angeles_San_Diego  Total_Costs  4
    Route_Los_Angeles_San_Diego  Sum_of_Products_into_Stores_San_Diego  1
    Route_Los_Angeles_San_Diego  Sum_of_Products_out_of_Plant_Los_Angeles  1
    Route_Los_Angeles_Tucson  Total_Costs  8
    Route_Los_Angeles_Tucson  Sum_of_Products_into_Stores_Tucson  1
    Route_Los_Angeles_Tucson  Sum_of_Products_out_of_Plant_Los_Angeles  1
    Route_Phoenix_Barstow  Total_Costs  5
    Route_Phoenix_Barstow  Sum_of_Products_into_Stores_Barstow  1
    Route_Phoenix_Barstow  Sum_of_Products_out_of_Plant_Phoenix  1
    Route_Phoenix_Dallas  Total_Costs  8
    Route_Phoenix_Dallas  Sum_of_Products_into_Stores_Dallas  1
    Route_Phoenix_Dallas  Sum_of_Products_out_of_Plant_Phoenix  1
    Route_Phoenix_San_Diego  Total_Costs  6
    Route_Phoenix_San_Diego  Sum_of_Products_into_Stores_San_Diego  1
    Route_Phoenix_San_Diego  Sum_of_Products_out_of_Plant_Phoenix  1
    Route_Phoenix_Tucson  Total_Costs  3
    Route_Phoenix_Tucson  Sum_of_Products_into_Stores_Tucson  1
    Route_Phoenix_Tucson  Sum_of_Products_out_of_Plant_Phoenix  1
    Route_San_Francisco_Barstow  Total_Costs  3
    Route_San_Francisco_Barstow  Sum_of_Products_into_Stores_Barstow  1
    Route_San_Francisco_Barstow  Sum_of_Products_out_of_Plant_San_Francisco  1
    Route_San_Francisco_Dallas  Total_Costs  6
    Route_San_Francisco_Dallas  Sum_of_Products_into_Stores_Dallas  1
    Route_San_Francisco_Dallas  Sum_of_Products_out_of_Plant_San_Francisco  1
    Route_San_Francisco_San_Diego  Total_Costs  5
    Route_San_Francisco_San_Diego  Sum_of_Products_into_Stores_San_Diego  1
    Route_San_Francisco_San_Diego  Sum_of_Products_out_of_Plant_San_Francisco  1
    Route_San_Francisco_Tucson  Total_Costs  2
    Route_San_Francisco_Tucson  Sum_of_Products_into_Stores_Tucson  1
    Route_San_Francisco_Tucson  Sum_of_Products_out_of_Plant_San_Francisco  1
    MARK0001  'MARKER'                 'INTEND'
RHS
    RHS_V     Sum_of_Products_into_Stores_Barstow  1000
    RHS_V     Sum_of_Products_into_Stores_Dallas  1200
    RHS_V     Sum_of_Products_into_Stores_San_Diego  1700
    RHS_V     Sum_of_Products_into_Stores_Tucson  1500
BOUNDS
 BV BOUND     BuildaPlant_Denver
 BV BOUND     BuildaPlant_Los_Angeles
 BV BOUND     BuildaPlant_Phoenix
 BV BOUND     BuildaPlant_San_Francisco
 LI BOUND     Route_Denver_Barstow  0
 LI BOUND     Route_Denver_Dallas  0
 LI BOUND     Route_Denver_San_Diego  0
 LI BOUND     Route_Denver_Tucson  0
 LI BOUND     Route_Los_Angeles_Barstow  0
 LI BOUND     Route_Los_Angeles_Dallas  0
 LI BOUND     Route_Los_Angeles_San_Diego  0
 LI BOUND     Route_Los_Angeles_Tucson  0
 LI BOUND     Route_Phoenix_Barstow  0
 LI BOUND     Route_Phoenix_Dallas  0
 LI BOUND     Route_Phoenix_San_Diego  0
 LI BOUND     Route_Phoenix_Tucson  0
 LI BOUND     Route_San_Francisco_Barstow  0
 LI BOUND     Route_San_Francisco_Dallas  0
 LI BOUND     Route_San_Francisco_San_Diego  0
 LI BOUND     Route_San_Francisco_Tucson  0
ENDATA
