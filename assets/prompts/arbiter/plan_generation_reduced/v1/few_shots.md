# Plan Generation Examples

## ✅ Simple Plan (no reuse)

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

```lisp
(plan
  :name "personalized_greeting"
  :language rtfs20
  :body (do
    (step "Greet User"
      (let [name (call :ccos.user.ask "What is your name?")]
        (call :ccos.echo {:message (str "Hello, " name "!")})
        ; Return the name for potential reuse
        name))
  )
  :annotations {:returns "string" :category "greeting"}
)
```

## ✅ Multiple Prompts with Summary (sequential bindings in one step)

```lisp
(plan
  :name "user_survey"
  :language rtfs20
  :body (do
    (step "Survey"
      (let [name (call :ccos.user.ask "What is your name?")
            age (call :ccos.user.ask "How old are you?")
            hobby (call :ccos.user.ask "What is your hobby?")]
        (call :ccos.echo {:message (str "Summary: " name ", age " age ", enjoys " hobby)})
        ; Return the collected survey data for reuse
        {:name name :age age :hobby hobby}))
  )
  :annotations {:returns "map" :category "data_collection"}
)
```

## ✅ Conditional Branching (if for yes/no)

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
        ; Return the user's preference for reuse
        likes))
  )
  :annotations {:returns "string" :category "preference"}
)
```

## ✅ Multiple Choice (match for many options)

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
        ; Return the selected language for reuse
        lang))
  )
  :annotations {:returns "string" :category "programming"}
)
```

## ✅ Math Operation with Return Value

```lisp
(plan
  :name "calculate_sum"
  :language rtfs20
  :body (do
    (step "Calculate and Return Sum"
      (let [result (call :ccos.math.add 5 3)]
        (call :ccos.echo {:message (str "Sum is: " result)})
        ; Return the calculated result for reuse
        result))
  )
  :annotations {:returns "number" :operation "addition"}
)
```
