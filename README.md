# Sflang

Sflang is a lightweight, fast interpreted programming language implemented in Rust. It uses a bytecode VM architecture for execution speed while maintaining simplicity.

## Features

- **Bytecode VM**: Compiled to bytecode, executed by a stack-based virtual machine
- **Fast**: Performance target between Rust and Python, closer to Rust
- **19 data types**: undefined, int, float, bool, byte, string, bytes, byteArray, array, object, map, function, builtin, error, native, bigInt, bigFloat, datetime, file
- **Lightweight OOP**: Constructor functions + automatic `self` binding + prototype chain
- **Concurrency**: `run` keyword launches real OS threads; channels, mutex, rwlock, waitgroup, semaphore, once
- **Rich builtins**: ~200 built-in functions covering string, array, math, file IO, JSON, regex, hash, encoding, datetime, system, and more
- **Error handling**: try/catch/finally + defer
- **Closures**: Full closure support with shared captured variables
- **Operators**: Full operator set including `??` `?:` `++` `--` `+=` bitwise `&` `|` `^` `~` `<<` `>>` slice `[:]`
- **Script embedding**: Usable as a Rust library or standalone CLI (`sf`)

## Quick Start

```bash
# Build
cargo build --release

# Run a script
sf script.sf

# Eval code
sf -e "println(\"Hello, Sflang!\")"

# REPL
sf
```

## Examples

```sflang
// Hello World
println("Hello, Sflang!")

// Variables and functions
func factorial(n) {
    if n <= 1 { return 1 }
    return n * factorial(n - 1)
}
println(factorial(10))

// OOP with automatic self binding
// obj.method(args) auto-injects obj as implicit first param
func Counter(startA) {
    var self = {value: startA}
    self.inc = func(self, n) { self.value = self.value + n; return self.value }
    self.get = func(self) { return self.value }
    return self
}
var c = Counter(10)
println(c.inc(5))  // 15 — no need to pass c manually

// Ordered Map (insertion-ordered, pure data)
var config = map{"host": "localhost", "port": 8080}
for k, v in config {
    println(k, "=", v)  // insertion order guaranteed
}

// Concurrency
var ch = newChannel()
run func() { chanSend(ch, 42) }
println(chanRecv(ch))  // 42

// Big integers
println(bigInt("99999999999999999999") * bigInt("99999999999999999999"))

// Regex
println(regFindAll("\\d+", "a1b22c333"))  // ["1", "22", "333"]

// File IO
var f = openFile("data.txt", "r")
defer close(f)
println(readLine(f))
```

## Operators

```
Priority (low to high):
  = += -= *= /= %= ??= &= |= ^= <<= >>=
  ?:                     (ternary)
  ??                     (null coalescing)
  ||                     (logical or)
  &&                     (logical and)
  |                      (bitwise or)
  ^                      (bitwise xor)
  &                      (bitwise and)
  == !=                  (equality)
  < <= > >=              (comparison)
  << >>                  (shift)
  + -                    (additive)
  * / %                  (multiplicative)
  - ! ~ ++ --            (unary / prefix)
  . [] () ?: postfix++   (postfix)
```

## Types

| Type | Description |
|------|-------------|
| `undefined` | Null value (nil is removed, use undefined) |
| `int` | 64-bit signed integer |
| `float` | 64-bit double |
| `bool` | Boolean (true / false) |
| `byte` | 0-255 with wrapping arithmetic |
| `string` | UTF-8 string (char-indexed) |
| `bytes` | Immutable byte sequence |
| `byteArray` | Mutable byte sequence |
| `array` | Dynamic array |
| `object` | HashMap + prototype chain (OOP) |
| `map` | Ordered map (insertion-ordered, pure data) |
| `function` | User function / closure |
| `builtin` | Built-in function |
| `error` | Error value (throw/catch) |
| `native` | Host-embedded Rust value |
| `bigInt` | Arbitrary precision integer |
| `bigFloat` | Arbitrary precision decimal |
| `datetime` | Date/time (millis + timezone) |
| `file` | File handle (streaming / random access) |

## Project Structure

```
sflang/     # Core library (lexer, parser, compiler, VM, builtins)
sf/         # CLI binary (interpreter + REPL)
examples/   # Example scripts (sample01-03)
```

## License

MIT
