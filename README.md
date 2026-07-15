# Sflang

Sflang is a lightweight, fast interpreted programming language implemented in Rust. It uses a bytecode VM architecture for execution speed while maintaining simplicity.

## Features

- **Bytecode VM**: Compiled to bytecode, executed by a stack-based virtual machine
- **Fast**: Performance target between Rust and Python, closer to Rust
- **19 data types**: undefined, int, float, bool, byte, string, bytes, byteArray, array, object, map, function, builtin, error, native, bigInt, bigFloat, datetime, file
- **Lightweight OOP**: Constructor functions + automatic `self` binding + prototype chain
- **Concurrency**: `run` keyword launches real OS threads; channels, mutex, rwlock, waitgroup, semaphore, once
- **Rich builtins**: **~680 built-in functions** across 30+ categories (see below)
- **Control flow**: `if/elif/else`, `for` (C-style / for-in / infinite), `while`, `switch`, `try/catch/finally`, `defer`, labeled `break`/`continue`
- **Error handling**: try/catch/finally + defer
- **Closures**: Full closure support with shared captured variables
- **Default params**: `func f(a, b="x", c=a+1) { }` with references to earlier params
- **String interpolation**: `"Hello, ${name}! Count = ${n+1}"` in double-quoted and multi-line strings
- **Operators**: Full operator set including `??` `?:` `++` `--` `+=` bitwise `&` `|` `^` `~` `<<` `>>` slice `[:]`; `+` auto-concatenates string with any type
- **Self-documenting**: `help()` builtin + `sf --list-builtins` for AI-friendly introspection
- **Script embedding**: Usable as a Rust library or standalone CLI (`sf`)
- **Standalone executable**: `sf --build script.sf` packs a script into a single executable

## Built-in Function Categories

~680 builtins organized by prefix. Use `sf --list-builtins` to list all, or `help("regFind")` to inspect a single function.

| Category | Prefix / Examples | Coverage |
|----------|-------------------|----------|
| String | `str*` (strToUpper, strSplit, strReplace, strTrim, ...) | 35 |
| Array | `arr*`, sort/contains/slice/concat | 13 |
| Math | `math*`, abs/floor/sqrt/pow/random | 24 |
| Regex | `reg*` (regFind, regMatch, regReplace, ...) | 13 |
| File IO | `fs*`, readFile/writeFile/openFile/getFileList | 32 |
| JSON | `json*` (encode/decode/format) | 9 |
| Encoding | base64/url/html encode & decode | 10 |
| Hash/Crypto | md5/sha*/hmac/aes*/jwt | 20 |
| Datetime | `now*`/`dt*` (format/parse/add) | 19 |
| System | `sys*` (env/dir/exec/clipboard) | 26 |
| Database | `db*` (SQLite/MySQL/PostgreSQL/MSSQL/Oracle) | 12 |
| HTTP/Network | httpServer/getWeb*/webSocket | 52 |
| TCP | tcpListen/Connect/Pipe | 12 |
| Image | `image*` (load/save/resize/canvas) + `imageGen*` (procedural) | 82 |
| Excel | `excel*` (read/write xlsx) | 14 |
| Compression | `zip*`, gzip/compressBytes | 13 |
| SSH/FTP/S3 | ssh*/ftp*/s3* | 46 |
| Email | sendMail (SMTP/SSL/attachments) | 1 |
| Concurrency | channel/mutex/rwlock/waitgroup/semaphore/once | 6 |
| GUI | `gui*` (WebView2 window) | 8 |
| Containers | stack/queue/ring/seq | 30 |
| Others | print/len/keys/range/sleep/uuid/typeCode/le*/xml*/csv*/docx*/proxy*/xxci*/pinyin* | ~140 |

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

// Concurrency (run launches a real OS thread; callee must be a named function)
var ch = newChannel()
sender := func() { chanSend(ch, 42) }
run sender()
println(chanRecv(ch))  // 42

// Big integers
println(bigInt("99999999999999999999") * bigInt("99999999999999999999"))

// Regex
println(regFindAll("\\d+", "a1b22c333"))  // ["1", "22", "333"]

// switch (equality match, no fallthrough, break optional)
switch osName() {
    case "windows" {
        println("running on Windows")
    }
    case "linux" {
        println("running on Linux")
    }
    default {
        println("other OS")
    }
}

// String interpolation ${expr} and default params
func greet(name, prefix="Mr") {
    return "${prefix}. ${name}"
}
println(greet("Smith"))  // Mr. Smith

// String + any type (auto to_str)
println("count: " + 42)  // count: 42

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
