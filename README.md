# Sflang

A lightweight, fast interpreted programming language implemented in Go.

## Overview

Sflang is a bytecode VM-based interpreted language designed for simplicity and speed. It aims to deliver performance between Go and Python, closer to Go.

### Features

- **Fast Execution**: Bytecode VM architecture for efficient execution
- **Lightweight**: Minimal dependencies, easy to embed
- **Cross-Platform**: Supports Windows and Linux
- **Simple Syntax**: Clean and intuitive syntax
- **Embedded**: Can be embedded in Go applications as a library
- **Extensible**: WASM plugin support for extending functionality

### Data Types

- Integer (64-bit)
- Float (64-bit)
- String (UTF-8)
- Boolean
- Array
- Map (Object)
- Null
- Function

## Installation

### Build from Source

```bash
git clone https://github.com/topxeq/sflang.git
cd sflang
go build -o sf
```

### Requirements

- Go 1.21 or higher

## Usage

### REPL

Start the interactive REPL:

```bash
./sf
```

### Run a Script

```bash
./sf script.sf
```

### Command Line Options

```
  -ast        Print the AST and exit
  -bc         Print the bytecode and exit
  -version    Print version information
  -help       Print help information
```

## Language Syntax

### Variables

```sflang
let x = 10
let name = "Sflang"
let pi = 3.14159
```

### Functions

```sflang
fn add(a, b) {
    return a + b
}

let result = add(5, 3)  // 8
```

### Control Flow

```sflang
// If-else
if (x > 0) {
    print("positive")
} else {
    print("non-positive")
}

// Loops
let i = 0
while (i < 10) {
    print(i)
    i = i + 1
}
```

### Arrays and Maps

```sflang
// Array
let arr = [1, 2, 3, 4, 5]
print(arr[0])  // 1

// Map
let person = {"name": "Alice", "age": 30}
print(person["name"])  // Alice
```

## Embedding in Go

```go
package main

import (
    "github.com/topxeq/sflang/lexer"
    "github.com/topxeq/sflang/parser"
    "github.com/topxeq/sflang/compiler"
    "github.com/topxeq/sflang/vm"
)

func main() {
    source := `print("Hello from Sflang!")`

    l := lexer.New(source)
    p := parser.New(l)
    program := p.ParseProgram()

    c := compiler.New()
    c.Compile(program)

    machine := vm.New(c.Bytecode())
    machine.Run()
}
```

## Project Structure

```
sflang/
├── ast/        # Abstract Syntax Tree definitions
├── builtin/    # Built-in functions
├── compiler/   # Bytecode compiler
├── lexer/      # Lexical analyzer
├── object/     # Runtime objects
├── parser/     # Parser
├── repl/       # Read-Eval-Print Loop
├── vm/         # Virtual Machine
└── main.go     # Entry point
```

## Built-in Functions

- `print(...)` - Print values to stdout
- `len(value)` - Get length of string, array, or map
- `type(value)` - Get type of value
- `push(array, value)` - Push value to array
- `pop(array)` - Pop value from array
- `input([prompt])` - Read input from stdin
- And more...

## License

MIT License - see [LICENSE](LICENSE) for details.

## Contributing

Contributions are welcome! Please feel free to submit issues and pull requests.

## Author

topxeq
