# Sflang

一个轻量级、快速的解释型编程语言，使用Go语言实现。

## 概述

Sflang是一个基于字节码虚拟机的解释型语言，设计目标是简单和快速。它的执行速度介于Go和Python之间，更接近Go。

### 特性

- **快速执行**：采用字节码虚拟机架构，执行效率高
- **轻量级**：依赖极少，易于嵌入
- **跨平台**：支持Windows和Linux
- **简洁语法**：清晰直观的语法设计
- **可嵌入**：可作为库嵌入到Go应用程序中
- **可扩展**：支持WASM插件扩展功能

### 数据类型

- 整数（64位）
- 浮点数（64位）
- 字符串（UTF-8）
- 布尔值
- 数组
- 映射/对象（Map/Object）
- 空值（Null）
- 函数

## 安装

### 从源码构建

```bash
git clone https://github.com/topxeq/sflang.git
cd sflang
go build -o sf
```

### 环境要求

- Go 1.21 或更高版本

## 使用

### REPL交互模式

启动交互式REPL：

```bash
./sf
```

### 运行脚本

```bash
./sf script.sf
```

### 命令行选项

```
  -ast        打印抽象语法树并退出
  -bc         打印字节码并退出
  -version    打印版本信息
  -help       打印帮助信息
```

## 语言语法

### 变量

```sflang
let x = 10
let name = "Sflang"
let pi = 3.14159
```

### 函数

```sflang
fn add(a, b) {
    return a + b
}

let result = add(5, 3)  // 8
```

### 控制流

```sflang
// 条件语句
if (x > 0) {
    print("正数")
} else {
    print("非正数")
}

// 循环
let i = 0
while (i < 10) {
    print(i)
    i = i + 1
}
```

### 数组和映射

```sflang
// 数组
let arr = [1, 2, 3, 4, 5]
print(arr[0])  // 1

// 映射
let person = {"name": "Alice", "age": 30}
print(person["name"])  // Alice
```

## 在Go中嵌入使用

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

## 项目结构

```
sflang/
├── ast/        # 抽象语法树定义
├── builtin/    # 内置函数
├── compiler/   # 字节码编译器
├── lexer/      # 词法分析器
├── object/     # 运行时对象
├── parser/     # 语法解析器
├── repl/       # 交互式环境
├── vm/         # 虚拟机
└── main.go     # 程序入口
```

## 内置函数

- `print(...)` - 输出值到标准输出
- `len(value)` - 获取字符串、数组或映射的长度
- `type(value)` - 获取值的类型
- `push(array, value)` - 向数组添加元素
- `pop(array)` - 从数组弹出元素
- `input([prompt])` - 从标准输入读取
- 更多...

## 许可证

MIT许可证 - 详见 [LICENSE](LICENSE)

## 贡献

欢迎贡献！请随时提交问题和拉取请求。

## 作者

topxeq
