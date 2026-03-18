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

## Performance Benchmarks

Benchmarks run on Windows 11, Go 1.21, Python 3.14.

| Test | Sflang | Python 3.14 | Ratio |
|------|--------|-------------|-------|
| Fibonacci(35) | 938 ms | 825 ms | 1.14x |
| Loop Sum(10M) | 425 ms | 332 ms | 1.28x |
| Array Test(100K) | 15 ms | 8 ms | 1.88x |
| Nested Loop(1K×1K) | 38 ms | 28 ms | 1.36x |
| String Concat(10K) | 11 ms | 1 ms | 11x |

> **Note**: Sflang is designed for simplicity and fast startup. While Python 3.14+ has highly optimized loops and built-ins, Sflang delivers competitive performance for an embedded scripting language with a small footprint.

### Benchmark Code

```sflang
// Fibonacci
fn fib(n) {
    if (n < 2) { return n }
    return fib(n - 1) + fib(n - 2)
}

// Loop sum
fn loop_sum(n) {
    let sum = 0
    for (let i = 0; i < n; i++) {
        sum = sum + i
    }
    return sum
}
```

## Built-in Functions

- `print(...)` - Print values to stdout
- `println(...)` - Print values with newline
- `len(value)` - Get length of string, array, or map
- `typeCode(value)` - Get numeric type code
- `typeName(value)` - Get type name string
- `str(value)` - Convert to string
- `int(value)` - Convert to integer
- `float(value)` - Convert to float
- `bool(value)` - Convert to boolean
- `push(array, value)` - Push value to array
- `pop(array)` - Pop value from array
- `time()` - Get current time in milliseconds
- `sleep(ms)` - Sleep for milliseconds
- And 30+ more functions...

## License

MIT License - see [LICENSE](LICENSE) for details.

## Contributing

Contributions are welcome! Please feel free to submit issues and pull requests.

## Author

topxeq
