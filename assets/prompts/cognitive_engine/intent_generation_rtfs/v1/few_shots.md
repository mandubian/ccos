# Few-Shot Examples

Input: "Ask the user for their name and greet them politely"
Output:
```
(intent "greet_user" :goal "Ask user name then greet" :constraints {:interaction_mode "single_turn"} :preferences {:tone "friendly"} :success-criteria "User greeted with provided name")
```

Input: "Add 5 and 7 ensure exact arithmetic"
Output:
```
(intent "add_two_numbers" :goal "Compute exact sum of 5 and 7" :constraints {:operand1 "5" :operand2 "7" :accuracy "exact"} :success-criteria "Result equals 12")
```

Input: "Plan a trip to Paris for me"
Output:
```
(intent "plan_trip" :goal "Create personalized trip itinerary for Paris" :constraints {:destination "Paris"} :preferences {:duration "flexible" :budget "moderate" :interests "culture"} :success-criteria "Complete itinerary with activities and recommendations provided")
```

Input: "I need help organizing my schedule for next week"
Output:
```
(intent "organize_schedule" :goal "Create organized weekly schedule based on user needs" :constraints {:timeframe "next_week"} :preferences {:flexibility "high" :priority "work_life_balance"} :success-criteria "Structured schedule with time blocks and priorities assigned")
```
