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
- 大整数（任意精度，后缀 `n`）
- 大浮点数（任意精度，后缀 `m`）
- 字符串（UTF-8）
- 布尔值
- 数组
- 映射/对象（Map/Object）
- 空值（Null）
- 函数
- 错误（Error）
- 字节（Byte, uint8）
- 字符（Char, rune）
- 字节数组（Bytes）
- 字符数组（Chars）
- 时间（Time）
- 文件（File）

## 安装

### 一键安装

**Linux / macOS:**
```bash
curl -fsSL https://raw.githubusercontent.com/topxeq/sflang/main/install.sh | bash
```

**Windows (PowerShell):**
```powershell
irm https://raw.githubusercontent.com/topxeq/sflang/main/install.ps1 | iex
```

### 下载预编译二进制文件

从 [Releases](https://github.com/topxeq/sflang/releases) 下载最新版本：

| 平台 | 架构 | 文件 |
|------|------|------|
| Windows | x64 | `sf-windows-amd64.zip` |
| Linux | x64 | `sf-linux-amd64.tar.gz` |
| Linux | ARM64 | `sf-linux-arm64.tar.gz` |
| macOS | x64 | `sf-darwin-amd64.tar.gz` |
| macOS | M1/M2 | `sf-darwin-arm64.tar.gz` |

解压后将 `sf`（Windows 为 `sf.exe`）放入 PATH 目录即可。

### 从源码构建

```bash
git clone https://github.com/topxeq/sflang.git
cd sflang
go build -o sf
```

### 环境要求

- Go 1.21 或更高版本（仅从源码构建时需要）

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
func add(a, b) {
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

### 字符串

```sflang
// 普通字符串，支持转义序列
let s1 = "Hello\nWorld"
let s2 = '单引号也可以'

// Raw 字符串（反引号）- 不处理转义，可跨多行
let raw = `第一行
第二行
第三行`

// 适合文件路径和正则表达式
let path = `C:\Users\name\file.txt`
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
├── examples/   # 示例脚本
├── lexer/      # 词法分析器
├── object/     # 运行时对象
├── parser/     # 语法解析器
├── repl/       # 交互式环境
├── vm/         # 虚拟机
└── main.go     # 程序入口
```

## 性能基准测试

基准测试环境：Windows 11, Go 1.21, Python 3.14。

| 测试项 | Sflang | Python 3.14 | 比率 |
|--------|--------|-------------|------|
| 斐波那契(35) | 938 ms | 825 ms | 1.14x |
| 循环累加(1000万) | 425 ms | 332 ms | 1.28x |
| 数组测试(10万) | 15 ms | 8 ms | 1.88x |
| 嵌套循环(1000×1000) | 38 ms | 28 ms | 1.36x |
| 字符串拼接(1万) | 11 ms | 1 ms | 11x |

> **说明**：Sflang设计目标是简单和快速启动。虽然Python 3.14+对循环和内置函数做了大量优化，但Sflang作为嵌入式脚本语言仍具有竞争力，且占用资源更小。

### 基准测试代码

```sflang
// 斐波那契
func fib(n) {
    if (n < 2) { return n }
    return fib(n - 1) + fib(n - 2)
}

// 循环累加
func loop_sum(n) {
    let sum = 0
    for (let i = 0; i < n; i++) {
        sum = sum + i
    }
    return sum
}
```

## 内置函数

### 输入输出函数
- `loadText(path)` - 读取文本文件，返回字符串或错误
- `saveText(path, content)` - 写入文本文件，返回null或错误

### 字符串函数
- `subStr(s, start, len)` - 提取子字符串（支持Unicode）
- `split(str, sep)` - 按分隔符分割字符串
- `join(array, sep)` - 用分隔符连接数组元素
- `trim(str)` - 去除首尾空白
- `upper(str)` - 转换为大写
- `lower(str)` - 转换为小写
- `contains(str, substr)` - 检查是否包含子字符串
- `indexOf(str, substr)` - 查找子字符串位置
- `replace(str, old, new)` - 替换字符串

### 数组函数
- `push(array, value)` - 向数组添加元素
- `pop(array)` - 从数组弹出元素
- `shift(array)` - 移除并返回第一个元素
- `slice(array, start, end)` - 提取数组切片
- `concat(arrays...)` - 连接多个数组
- `append(array, values...)` - 追加元素到数组
- `range(end)` 或 `range(start, end)` - 生成整数数组

### 类型函数
- `typeCode(value)` - 获取数值类型码
- `typeName(value)` - 获取类型名称字符串
- `str(value)` - 转换为字符串
- `int(value)` - 转换为整数
- `float(value)` - 转换为浮点数
- `bool(value)` - 转换为布尔值

### 数学函数
- `abs(x)` - 绝对值
- `min(values...)` - 最小值
- `max(values...)` - 最大值
- `floor(x)` - 向下取整
- `ceil(x)` - 向上取整
- `sqrt(x)` - 平方根
- `pow(x, y)` - 幂运算
- `sin(x)`, `cos(x)` - 三角函数

### 映射函数
- `keys(map)` - 获取所有键
- `values(map)` - 获取所有值
- `has(map, key)` - 检查键是否存在
- `delete(map, key)` - 删除键

### 系统函数
- `print(...)` - 输出值到标准输出
- `println(...)` - 输出值并换行
- `pl(format, args...)` - 格式化打印
- `len(value)` - 获取字符串、数组或映射的长度
- `time()` - 获取当前时间（毫秒）
- `sleep(ms)` - 休眠指定毫秒数
- `exit(code)` - 退出程序

### 错误处理
- `error(msg)` - 创建错误对象
- `checkErr(err)` - 检查错误，非空则抛出
- `fatalf(format, args...)` - 打印错误并退出

### 命令行
- `argsG` - 全局变量，包含命令行参数
- `getSwitch(args, name, default)` - 从参数数组提取开关值

## 示例

示例脚本位于 `examples/` 目录：

- `anonymousFunc.sf` - 匿名函数和闭包
- `addBom.sf` - 为文本文件添加UTF-8 BOM

## 许可证

MIT许可证 - 详见 [LICENSE](LICENSE)

## 贡献

欢迎贡献！请随时提交问题和拉取请求。

## 作者

topxeq
