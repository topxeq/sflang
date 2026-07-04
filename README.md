# Sflang

Sflang is a lightweight, fast interpreted programming language implemented in Rust. It uses a bytecode VM architecture for execution speed while maintaining simplicity.

> **Note**: This is the Rust implementation of Sflang. The language was originally implemented in Go (see `charlang`).

## Features

- **Bytecode VM**: Compiled to bytecode, executed by a stack-based virtual machine
- **Fast**: Performance target between Rust and Python, closer to Rust
- **16 data types**: int, float, bool, string, bytes, byteArray, array, object, function, builtin, error, native, bigInt, bigFloat, datetime, undefined
- **Lightweight OOP**: Constructor functions + automatic `self` binding + prototype chain
- **Concurrency**: `run` keyword launches real OS threads; channels, mutex, rwlock, waitgroup, semaphore, once
- **Rich builtins**: ~180 built-in functions covering string, array, math, file IO, JSON, regex, hash, encoding, datetime, and more
- **Error handling**: try/catch/finally + defer
- **Closures**: Full closure support with shared captured variables
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
func Counter(startA) {
    var self = {value: startA}
    self.inc = func(self, n) { self.value = self.value + n; return self.value }
    self.get = func(self) { return self.value }
    return self
}
var c = Counter(10)
println(c.inc(5))  // 15

// Concurrency
var ch = newChannel()
run func() { chanSend(ch, 42) }
println(chanRecv(ch))  // 42

// Big integers
println(bigInt("99999999999999999999") * bigInt("99999999999999999999"))
```

## Project Structure

```
sflang/     # Core library (lexer, parser, compiler, VM, builtins)
sf/         # CLI binary (interpreter + REPL)
```

## License

MIT
