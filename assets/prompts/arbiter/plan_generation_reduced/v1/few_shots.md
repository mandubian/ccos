# Plan Generation Examples

## ✅ Simple (no reuse)

```lisp
(do
  (step "Get Name" 
    (call :ccos.user.ask "What is your name?")))
```

## ✅ Capture and Reuse (CORRECT - single step with let)

```lisp
(do
  (step "Greet User" 
    (let [name (call :ccos.user.ask "What is your name?")]
      (call :ccos.echo {:message (str "Hello, " name "!")}))))
```

## ✅ Multiple Prompts with Summary (CORRECT - sequential bindings in one step)

```lisp
(do
  (step "Survey" 
    (let [name (call :ccos.user.ask "What is your name?")
          age (call :ccos.user.ask "How old are you?")
          hobby (call :ccos.user.ask "What is your hobby?")]
      (call :ccos.echo {:message (str "Summary: " name ", age " age ", enjoys " hobby)}))))
```

## ✅ Conditional Branching (CORRECT - if for yes/no)

```lisp
(do
  (step "Pizza Check" 
    (let [likes (call :ccos.user.ask "Do you like pizza? (yes/no)")]
      (if (= likes "yes")
        (call :ccos.echo {:message "Great! Pizza is delicious!"})
        (call :ccos.echo {:message "Maybe try it sometime!"})))))
```

## ✅ Multiple Choice (CORRECT - match for many options)

```lisp
(do
  (step "Language Hello World" 
    (let [lang (call :ccos.user.ask "Choose: rust, python, or javascript")]
      (match lang
        "rust" (call :ccos.echo {:message "println!(\"Hello\")"})
        "python" (call :ccos.echo {:message "print('Hello')"})
        "javascript" (call :ccos.echo {:message "console.log('Hello')"})
        _ (call :ccos.echo {:message "Unknown language"})))))
```

## ✅ Math Operation

```lisp
(do
  (step "Calculate Sum"
    (call :ccos.math.add 5 3))
  (step "Display Result"
    (call :ccos.echo {:message "Sum calculated"})))
```
