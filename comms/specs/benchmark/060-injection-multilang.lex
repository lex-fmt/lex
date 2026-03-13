Injection Test Document

This document tests tree-sitter injection highlighting for embedded code in verbatim blocks.

1. Python Example

    Python Code:
        def hello(name):
            # A simple greeting function
            message = f"Hello, {name}!"
            print(message)
            return True

        class Calculator:
            def __init__(self):
                self.value = 0

            def add(self, n):
                self.value += n
                return self
    :: python ::

2. JavaScript Example

    JavaScript Code:
        // Fetch data from an API
        async function fetchData(url) {
            const response = await fetch(url);
            const data = response.json();
            return data;
        }

        const numbers = [1, 2, 3, 4, 5];
        const doubled = numbers.map(n => n * 2);
        console.log("Result:", doubled);
    :: javascript ::

3. JSON Example

    Config:
        {
            "name": "lex-vscode",
            "version": "1.0.0",
            "enabled": true,
            "count": 42,
            "tags": ["editor", "syntax"],
            "nested": {
                "key": null
            }
        }
    :: json ::

4. Rust Example

    Rust Code:
        use std::collections::HashMap;

        fn main() {
            let mut map: HashMap<String, i32> = HashMap::new();
            map.insert("hello".to_string(), 42);

            for (key, value) in &map {
                println!("{}: {}", key, value);
            }
        }

        struct Point {
            x: f64,
            y: f64,
        }
    :: rust ::

5. Grouped Verbatim (multiple subjects, shared annotation)

    Install:
        $ brew install lex
    Run:
        $ lex help
    :: bash ::

6. Plain Verbatim (no injection)

    Plain Text:
        This is just plain text in a verbatim block.
        No language annotation, so no injection highlighting.
