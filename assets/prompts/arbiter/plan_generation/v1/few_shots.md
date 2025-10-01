# Few Shots
Input Intent: greet_user
Output:
```rtfs
(do
    (intent "greet_user" :goal "Ask user name then greet")
    (step "ask-name" (call :ccos.user.ask {:prompt "What is your name?"}))
    (step "echo-greeting" (call :ccos.echo {:message "Hello, <name>!"}))
    (edge :DependsOn "echo-greeting" "ask-name")
)
```
