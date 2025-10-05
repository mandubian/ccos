# Plan Generation Examples

## ✅ Simple Plan (single prompt)

**Intent**: Get user's name

```lisp
(plan
  :name "simple_user_input"
  :language rtfs20
  :body (do
    (step "Get Name"
      (call :ccos.user.ask "What is your name?")))
)
```

## ✅ Capture and Reuse (single step with let)

**Intent**: Greet user by name

```lisp
(plan
  :name "personalized_greeting"
  :language rtfs20
  :body (do
    (step "Greet User"
      (let [name (call :ccos.user.ask "What is your name?")]
        (call :ccos.echo {:message (str "Hello, " name "!")})
        name)))
  :annotations {:returns "string" :category "greeting"}
)
```

## ✅ Multiple Prompts with Structured Result

**Intent**: Collect user survey data

```lisp
(plan
  :name "user_survey"
  :language rtfs20
  :body (do
    (step "Collect Survey Data"
      (let [name (call :ccos.user.ask "What is your name?")
            age (call :ccos.user.ask "How old are you?")
            hobby (call :ccos.user.ask "What is your hobby?")]
        (call :ccos.echo {:message (str "Summary: " name ", age " age ", enjoys " hobby)})
        {:user/name name :user/age age :user/hobby hobby})))
  :annotations {:returns "map" :category "data_collection"}
)
```

## ✅ Conditional Branching (if for yes/no)

**Intent**: Check pizza preference

```lisp
(plan
  :name "pizza_preference"
  :language rtfs20
  :body (do
    (step "Pizza Check"
      (let [likes (call :ccos.user.ask "Do you like pizza? (yes/no)")]
        (if (= likes "yes")
          (call :ccos.echo {:message "Great! Pizza is delicious!"})
          (call :ccos.echo {:message "Maybe try it sometime!"}))
        {:preference/pizza likes})))
  :annotations {:returns "map" :category "preference"}
)
```

## ✅ Multiple Choice (match for many options)

**Intent**: Show hello world in chosen language

```lisp
(plan
  :name "language_hello_world"
  :language rtfs20
  :body (do
    (step "Language Hello World"
      (let [lang (call :ccos.user.ask "Choose: rust, python, or javascript")]
        (match lang
          "rust" (call :ccos.echo {:message "println!(\"Hello\")"})
          "python" (call :ccos.echo {:message "print('Hello')"})
          "javascript" (call :ccos.echo {:message "console.log('Hello')"})
          _ (call :ccos.echo {:message "Unknown language"}))
        {:language/choice lang})))
  :annotations {:returns "map" :category "programming"}
)
```

## ✅ Math Operation with Return Value

**Intent**: Calculate sum of two numbers

```lisp
(plan
  :name "calculate_sum"
  :language rtfs20
  :body (do
    (step "Calculate and Return Sum"
      (let [result (call :ccos.math.add 5 3)]
        (call :ccos.echo {:message (str "Sum is: " result)})
        {:math/result result :math/operation "addition"})))
  :annotations {:returns "map" :operation "addition"}
)
```

## ✅ Complex Multi-Step Plan (trip planning)

**Intent**: Plan a trip

```lisp
(plan
  :name "plan_trip"
  :language rtfs20
  :body (do
    (step "Collect Trip Preferences"
      (let [destination (call :ccos.user.ask "Where would you like to travel?")
            duration (call :ccos.user.ask "How many days will you stay?")
            interests (call :ccos.user.ask "What activities interest you?")]
        (call :ccos.echo {:message (str "Planning trip to " destination " for " duration " days")})
        {:trip/destination destination
         :trip/duration duration
         :trip/interests interests})))
  :annotations {:returns "map" :category "planning"}
)
```

## ✅ Complex Cultural Trip Planning (all variables in scope)

**Intent**: Plan a cultural trip to Paris

```lisp
(plan
  :name "plan_cultural_trip"
  :language rtfs20
  :body (do
    (step "Collect Cultural Trip Details"
      (let [destination (call :ccos.user.ask "Where would you like to travel?")
            duration (call :ccos.user.ask "How many days will you stay?")
            arrival (call :ccos.user.ask "What's your arrival date?")
            departure (call :ccos.user.ask "What's your departure date?")
            budget (call :ccos.user.ask "What's your total budget?")
            art_preference (call :ccos.user.ask "What type of art interests you most? (classical/modern/contemporary)")
            museum_priority (call :ccos.user.ask "Which museums would you prioritize? (Louvre/Orsay/Pompidou/other)")
            daily_budget (call :ccos.user.ask "What's your preferred daily cultural budget in EUR?")
            walking_tolerance (call :ccos.user.ask "How much walking are you comfortable with? (low/medium/high)")]
        (call :ccos.echo {:message (str "Planning your " duration "-day cultural trip to " destination " from " arrival " to " departure " with " budget " budget")})
        {:trip/destination destination
         :trip/duration duration
         :trip/arrival arrival
         :trip/departure departure
         :trip/budget budget
         :cultural/art_preference art_preference
         :cultural/museum_priority museum_priority
         :budget/daily daily_budget
         :activity/walking walking_tolerance})))
  :annotations {:returns "map" :category "cultural_planning"}
)
```

## ❌ WRONG - Variables Across Steps

```lisp
; DON'T DO THIS - variables don't cross step boundaries!
(plan
  :name "broken_plan"
  :language rtfs20
  :body (do
    (step "Get Name" 
      (let [name (call :ccos.user.ask "Name?")] 
        name))
    (step "Use Name" 
      (call :ccos.echo {:message name}))))  ; ERROR: name not in scope!
```

## ❌ WRONG - Let Without Body

```lisp
; DON'T DO THIS - let must have a body expression!
(plan
  :name "broken_let"
  :language rtfs20
  :body (do
    (step "Bad" 
      (let [name (call :ccos.user.ask "Name?")]))))  ; ERROR: missing body!
```