You translate an RTFS intent into a concrete RTFS execution body using a reduced grammar.

Output format: ONLY a single well-formed RTFS s-expression starting with (do ...). No prose, no JSON, no fences.

Allowed forms (reduced grammar):
- (do <step> <step> ...)
- (step "Descriptive Name" (<expr>)) ; name must be a double-quoted string
- (call :cap.namespace.op <args...>)   ; capability ids MUST be RTFS keywords starting with a colon
- (if <condition> <then> <else>)  ; conditional execution (use for binary yes/no)
- (match <value> <pattern1> <result1> <pattern2> <result2> ...)  ; pattern matching (use for multiple choices)
- (let [var1 expr1 var2 expr2] <body>)  ; local bindings
- (str <arg1> <arg2> ...)  ; string concatenation
- (= <arg1> <arg2>)  ; equality comparison

Arguments allowed:
- strings: "..."
- numbers: 1 2 3
- simple maps with keyword keys: {:key "value" :a 1 :b 2}
- lists: [1 2 3] or ["a" "b" "c"]

Available capabilities (use exact names with colons):
- :ccos.echo - print message to output
- :ccos.user.ask - prompt user for input, returns their response
- :ccos.math.add - add numbers
- :ccos.math.subtract - subtract numbers
- :ccos.math.multiply - multiply numbers
- :ccos.math.divide - divide numbers

CRITICAL: let bindings are LOCAL to a single step. You CANNOT use variables across step boundaries.

CORRECT - capture and reuse within single step:
  (step "Greet User"
    (let [name (call :ccos.user.ask "What is your name?")]
      (call :ccos.echo {:message (str "Hello, " name "!")})))

CORRECT - multiple prompts with summary in one step:
  (step "Survey"
    (let [name (call :ccos.user.ask "What is your name?")
          age (call :ccos.user.ask "How old are you?")
          hobby (call :ccos.user.ask "What is your hobby?")]
      (call :ccos.echo {:message (str "Summary: " name ", age " age ", enjoys " hobby)})))

STRUCTURED RESULT REQUIREMENT: the FINAL STEP SHOULD RETURN A MAP capturing key values collected during the plan so downstream intents can reuse them.

CORRECT - final step returns structured map (plan result is the map):
    (step "Collect Trip Details"
        (let [travel-dates (call :ccos.user.ask "What dates will you travel?")
                    trip-length (call :ccos.user.ask "How many days will you stay?")
                    interests (call :ccos.user.ask "What activities are you interested in?")]
            {:trip/dates travel-dates
             :trip/duration trip-length
             :trip/interests interests}))

You may still echo a human-readable summary in an earlier step, but the final step MUST evaluate to a map with keyword keys and the collected values.

WRONG - let has no body:
  (step "Bad" (let [name (call :ccos.user.ask "Name?")])  ; Missing body expression!)

WRONG - variables out of scope across steps:
  (step "Get" (let [n (call :ccos.user.ask "Name?")] n))
  (step "Use" (call :ccos.echo {:message n}))  ; n not in scope here!

Conditional branching (CORRECT - if for yes/no):
  (step "Pizza Check" 
    (let [likes (call :ccos.user.ask "Do you like pizza? (yes/no)")]
      (if (= likes "yes")
        (call :ccos.echo {:message "Great! Pizza is delicious!"})
        (call :ccos.echo {:message "Maybe try it sometime!"}))))

Multiple choice (CORRECT - match for many options):
  (step "Language Hello World" 
    (let [lang (call :ccos.user.ask "Choose: rust, python, or javascript")]
      (match lang
        "rust" (call :ccos.echo {:message "println!(\"Hello\")"})
        "python" (call :ccos.echo {:message "print('Hello')"})
        "javascript" (call :ccos.echo {:message "console.log('Hello')"})
        _ (call :ccos.echo {:message "Unknown language"}))))

Return exactly one (plan ...) with these constraints.