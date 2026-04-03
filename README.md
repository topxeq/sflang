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
- Error
- Byte (uint8)
- Char (rune)
- Bytes (byte array)
- Chars (rune array)
- Time
- File

## Installation

### One-line Install

**Linux / macOS:**
```bash
curl -fsSL https://raw.githubusercontent.com/topxeq/sflang/main/install.sh | bash
```

**Windows (PowerShell):**
```powershell
irm https://raw.githubusercontent.com/topxeq/sflang/main/install.ps1 | iex
```

### Download Pre-built Binaries

Download the latest release for your platform from [Releases](https://github.com/topxeq/sflang/releases):

| Platform | Architecture | File |
|----------|--------------|------|
| Windows | x64 | `sf-windows-amd64.zip` |
| Linux | x64 | `sf-linux-amd64.tar.gz` |
| Linux | ARM64 | `sf-linux-arm64.tar.gz` |
| macOS | x64 | `sf-darwin-amd64.tar.gz` |
| macOS | M1/M2 | `sf-darwin-arm64.tar.gz` |

Extract and place `sf` (or `sf.exe` on Windows) in your PATH.

### Build from Source

```bash
git clone https://github.com/topxeq/sflang.git
cd sflang
go build -o sf
```

### Requirements

- Go 1.21 or higher (for building from source)

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
func add(a, b) {
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

### Strings

```sflang
// Normal string with escape sequences
let s1 = "Hello\nWorld"
let s2 = 'Single quotes work too'

// Raw string (backticks) - no escape processing, can span multiple lines
let raw = `Line 1
Line 2
Line 3`

// Useful for file paths and regex
let path = `C:\Users\name\file.txt`
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
├── examples/   # Example scripts
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
func fib(n) {
    if (n < 2) { return n }
    return fib(n - 1) + fib(n - 2)
}

// Loop sum
func loop_sum(n) {
    let sum = 0
    for (let i = 0; i < n; i++) {
        sum = sum + i
    }
    return sum
}
```

## Built-in Functions

### I/O Functions
- `loadText(path)` - Read text file, returns string or error
- `saveText(path, content)` - Write text file, returns null or error

### String Functions
- `subStr(s, start, len)` - Extract substring (Unicode-aware)
- `split(str, sep)` - Split string by separator
- `join(array, sep)` - Join array elements with separator
- `trim(str)` - Remove leading/trailing whitespace
- `upper(str)` - Convert to uppercase
- `lower(str)` - Convert to lowercase
- `contains(str, substr)` - Check if string contains substring
- `indexOf(str, substr)` - Find substring position
- `replace(str, old, new)` - Replace occurrences

### Array Functions
- `push(array, value)` - Push value to array
- `pop(array)` - Pop value from array
- `shift(array)` - Remove and return first element
- `slice(array, start, end)` - Extract array slice
- `concat(arrays...)` - Concatenate arrays
- `append(array, values...)` - Append values to array
- `range(end)` or `range(start, end)` - Generate integer array

### Type Functions
- `typeCode(value)` - Get numeric type code
- `typeName(value)` - Get type name string
- `str(value)` - Convert to string
- `int(value)` - Convert to integer
- `float(value)` - Convert to float
- `bool(value)` - Convert to boolean

### Math Functions
- `abs(x)` - Absolute value
- `min(values...)` - Minimum value
- `max(values...)` - Maximum value
- `floor(x)` - Floor value
- `ceil(x)` - Ceiling value
- `sqrt(x)` - Square root
- `pow(x, y)` - Power
- `sin(x)`, `cos(x)` - Trigonometric functions

### Map Functions
- `keys(map)` - Get map keys
- `values(map)` - Get map values
- `has(map, key)` - Check if key exists
- `delete(map, key)` - Delete key from map

### System Functions
- `print(...)` - Print values to stdout
- `println(...)` - Print values with newline
- `pl(format, args...)` - Format printing
- `len(value)` - Get length of string, array, or map
- `time()` - Get current time in milliseconds
- `sleep(ms)` - Sleep for milliseconds
- `exit(code)` - Exit program

### Error Handling
- `error(msg)` - Create error object
- `checkErr(err)` - Check error and panic if not null
- `fatalf(format, args...)` - Print error and exit

### Command Line
- `argsG` - Global variable containing command line arguments
- `getSwitch(args, name, default)` - Extract switch value from args

## Examples

Example scripts are available in the `examples/` directory:

- `anonymousFunc.sf` - Anonymous functions and closures
- `addBom.sf` - Add UTF-8 BOM to text files

## License

MIT License - see [LICENSE](LICENSE) for details.

## Contributing

Contributions are welcome! Please feel free to submit issues and pull requests.

## Author

topxeq
