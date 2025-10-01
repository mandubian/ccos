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
