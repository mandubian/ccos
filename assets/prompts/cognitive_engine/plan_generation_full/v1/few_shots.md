# Full Plan Examples

## ✅ Simple Plan (no reuse)

```lisp
(plan
  :name "simple_greeting"
  :language rtfs20
  :body (do
    (step "Get Name" 
      (call :ccos.user.ask "What is your name?"))))
```

## ✅ Capture and Reuse (CORRECT - single step)

```lisp
(plan
  :name "personalized_greeting"
  :language rtfs20
  :body (do
    (step "Greet User" 
      (let [name (call :ccos.user.ask "What is your name?")]
        (call :ccos.echo {:message (str "Hello, " name "!")})))))
```

## ✅ Multiple Prompts with Summary

```lisp
(plan
  :name "user_survey"
  :language rtfs20
  :body (do
    (step "Survey" 
      (let [name (call :ccos.user.ask "What is your name?")
            age (call :ccos.user.ask "How old are you?")
            hobby (call :ccos.user.ask "What is your hobby?")]
        (call :ccos.echo {:message (str "Summary: " name ", age " age ", enjoys " hobby)}))))
  :annotations {:priority "high" :category "user_onboarding"})
```

## ✅ Conditional Branching

```lisp
(plan
  :name "pizza_preference"
  :language rtfs20
  :body (do
    (step "Pizza Check" 
      (let [likes (call :ccos.user.ask "Do you like pizza? (yes/no)")]
        (if (= likes "yes")
          (call :ccos.echo {:message "Great! Pizza is delicious!"})
          (call :ccos.echo {:message "Maybe try it sometime!"})))))
```

## ✅ Multiple Choice with Match

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
          _ (call :ccos.echo {:message "Unknown language"})))))
  :annotations {:category "educational"})
```

## ✅ Math Operation

```lisp
(plan
  :name "calculate_sum"
  :language rtfs20
  :body (do
    (step "Calculate"
      (call :ccos.math.add 5 3))
    (step "Display"
      (call :ccos.echo {:message "Sum calculated"})))
  :annotations {:operation_type "arithmetic"})
```
