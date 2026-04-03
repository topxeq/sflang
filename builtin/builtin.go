// Package builtin provides built-in functions for Sflang.
package builtin

import (
	"fmt"
	"math"
	"os"
	"strconv"
	"strings"
	"time"
	"unicode/utf8"

	"github.com/topxeq/sflang/object"
)

// Builtins contains all built-in functions.
var Builtins = []struct {
	Name string
	Fn   object.BuiltinFunction
}{
	{"print", builtinPrint},
	{"println", builtinPrintln},
	{"pl", builtinPl},
	{"len", builtinLen},
	{"typeCode", builtinTypeCode},
	{"typeName", builtinTypeName},
	{"str", builtinStr},
	{"int", builtinInt},
	{"float", builtinFloat},
	{"bool", builtinBool},
	{"abs", builtinAbs},
	{"min", builtinMin},
	{"max", builtinMax},
	{"floor", builtinFloor},
	{"ceil", builtinCeil},
	{"sqrt", builtinSqrt},
	{"pow", builtinPow},
	{"sin", builtinSin},
	{"cos", builtinCos},
	{"push", builtinPush},
	{"pop", builtinPop},
	{"shift", builtinShift},
	{"slice", builtinSlice},
	{"join", builtinJoin},
	{"concat", builtinConcat},
	{"split", builtinSplit},
	{"trim", builtinTrim},
	{"upper", builtinUpper},
	{"lower", builtinLower},
	{"contains", builtinContains},
	{"indexOf", builtinIndexOf},
	{"replace", builtinReplace},
	{"time", builtinTime},
	{"sleep", builtinSleep},
	{"exit", builtinExit},
	{"keys", builtinKeys},
	{"values", builtinValues},
	{"delete", builtinDelete},
	{"has", builtinHas},
	{"range", builtinRange},
	{"append", builtinAppend},
	{"error", builtinError},
	{"loadText", builtinLoadText},
	{"saveText", builtinSaveText},
	{"getSwitch", builtinGetSwitch},
	{"fatalf", builtinFatalf},
	{"checkErr", builtinCheckErr},
	{"subStr", builtinSubStr},
}

// GetBuiltinByName returns a built-in function by name.
func GetBuiltinByName(name string) *object.Builtin {
	for _, b := range Builtins {
		if b.Name == name {
			return &object.Builtin{Fn: b.Fn}
		}
	}
	return nil
}

// GetBuiltinNames returns all built-in function names.
func GetBuiltinNames() []string {
	names := make([]string, len(Builtins))
	for i, b := range Builtins {
		names[i] = b.Name
	}
	return names
}

// Built-in function implementations

func builtinPrint(args ...object.Object) object.Object {
	for _, arg := range args {
		fmt.Print(arg.Inspect())
	}
	return object.NULL
}

func builtinPrintln(args ...object.Object) object.Object {
	for _, arg := range args {
		fmt.Print(arg.Inspect())
	}
	fmt.Println()
	return object.NULL
}

// builtinPl is a format printing function similar to fmt.Printf.
// It takes a format string and arguments, replaces format specifiers,
// and prints the result. Supported format specifiers:
//   %v - default format (value inspection)
//   %s - string
//   %d - integer (decimal)
//   %f - float
//   %t - boolean
//   %x - hexadecimal (integer)
//   %o - octal (integer)
//   %c - character (integer to rune)
//   %% - literal percent sign
func builtinPl(args ...object.Object) object.Object {
	if len(args) < 1 {
		return object.NewError("wrong number of arguments for pl: got=%d, want>=1", len(args))
	}

	formatStr, ok := args[0].(*object.String)
	if !ok {
		return object.NewError("first argument to 'pl' must be string, got %s", args[0].Type())
	}

	format := formatStr.Value
	argIndex := 1
	var result strings.Builder

	for i := 0; i < len(format); i++ {
		if format[i] == '%' && i+1 < len(format) {
			spec := format[i+1]
			switch spec {
			case 'v':
				if argIndex >= len(args) {
					result.WriteString("%v")
				} else {
					result.WriteString(args[argIndex].Inspect())
					argIndex++
				}
				i++
			case 's':
				if argIndex >= len(args) {
					result.WriteString("%s")
				} else {
					result.WriteString(args[argIndex].Inspect())
					argIndex++
				}
				i++
			case 'd':
				if argIndex >= len(args) {
					result.WriteString("%d")
				} else {
					switch arg := args[argIndex].(type) {
					case *object.Integer:
						result.WriteString(strconv.FormatInt(arg.Value, 10))
					case *object.Float:
						result.WriteString(strconv.FormatInt(int64(arg.Value), 10))
					default:
						result.WriteString(arg.Inspect())
					}
					argIndex++
				}
				i++
			case 'f':
				if argIndex >= len(args) {
					result.WriteString("%f")
				} else {
					switch arg := args[argIndex].(type) {
					case *object.Float:
						result.WriteString(strconv.FormatFloat(arg.Value, 'f', -1, 64))
					case *object.Integer:
						result.WriteString(strconv.FormatFloat(float64(arg.Value), 'f', -1, 64))
					default:
						result.WriteString(arg.Inspect())
					}
					argIndex++
				}
				i++
			case 't':
				if argIndex >= len(args) {
					result.WriteString("%t")
				} else {
					switch arg := args[argIndex].(type) {
					case *object.Boolean:
						if arg.Value {
							result.WriteString("true")
						} else {
							result.WriteString("false")
						}
					default:
						result.WriteString(arg.Inspect())
					}
					argIndex++
				}
				i++
			case 'x':
				if argIndex >= len(args) {
					result.WriteString("%x")
				} else {
					switch arg := args[argIndex].(type) {
					case *object.Integer:
						result.WriteString(strconv.FormatInt(arg.Value, 16))
					case *object.Float:
						result.WriteString(strconv.FormatInt(int64(arg.Value), 16))
					default:
						result.WriteString(arg.Inspect())
					}
					argIndex++
				}
				i++
			case 'o':
				if argIndex >= len(args) {
					result.WriteString("%o")
				} else {
					switch arg := args[argIndex].(type) {
					case *object.Integer:
						result.WriteString(strconv.FormatInt(arg.Value, 8))
					case *object.Float:
						result.WriteString(strconv.FormatInt(int64(arg.Value), 8))
					default:
						result.WriteString(arg.Inspect())
					}
					argIndex++
				}
				i++
			case 'c':
				if argIndex >= len(args) {
					result.WriteString("%c")
				} else {
					switch arg := args[argIndex].(type) {
					case *object.Integer:
						result.WriteRune(rune(arg.Value))
					default:
						result.WriteString(arg.Inspect())
					}
					argIndex++
				}
				i++
			case '%':
				result.WriteByte('%')
				i++
			default:
				result.WriteByte(format[i])
			}
		} else {
			result.WriteByte(format[i])
		}
	}

	fmt.Println(result.String())
	return object.NULL
}

func builtinLen(args ...object.Object) object.Object {
	if len(args) != 1 {
		return object.NewError("wrong number of arguments for len: got=%d, want=1", len(args))
	}

	switch arg := args[0].(type) {
	case *object.String:
		return &object.Integer{Value: int64(utf8.RuneCountInString(arg.Value))}
	case *object.Array:
		return &object.Integer{Value: int64(len(arg.Elements))}
	case *object.Map:
		return &object.Integer{Value: int64(len(arg.Pairs))}
	default:
		return object.NewError("argument to 'len' not supported, got %s", arg.Type())
	}
}

func builtinTypeCode(args ...object.Object) object.Object {
	if len(args) != 1 {
		return object.NewError("wrong number of arguments for typeCode: got=%d, want=1", len(args))
	}
	return &object.Integer{Value: int64(args[0].TypeCode())}
}

func builtinTypeName(args ...object.Object) object.Object {
	if len(args) != 1 {
		return object.NewError("wrong number of arguments for typeName: got=%d, want=1", len(args))
	}
	return &object.String{Value: args[0].TypeName()}
}

func builtinStr(args ...object.Object) object.Object {
	if len(args) != 1 {
		return object.NewError("wrong number of arguments for str: got=%d, want=1", len(args))
	}
	return &object.String{Value: args[0].Inspect()}
}

func builtinInt(args ...object.Object) object.Object {
	if len(args) != 1 {
		return object.NewError("wrong number of arguments for int: got=%d, want=1", len(args))
	}

	switch arg := args[0].(type) {
	case *object.Integer:
		return arg
	case *object.Float:
		return &object.Integer{Value: int64(arg.Value)}
	case *object.String:
		val, err := strconv.ParseInt(arg.Value, 10, 64)
		if err != nil {
			return object.NewError("cannot convert '%s' to integer", arg.Value)
		}
		return &object.Integer{Value: val}
	case *object.Boolean:
		if arg.Value {
			return &object.Integer{Value: 1}
		}
		return &object.Integer{Value: 0}
	default:
		return object.NewError("cannot convert %s to integer", arg.Type())
	}
}

func builtinFloat(args ...object.Object) object.Object {
	if len(args) != 1 {
		return object.NewError("wrong number of arguments for float: got=%d, want=1", len(args))
	}

	switch arg := args[0].(type) {
	case *object.Float:
		return arg
	case *object.Integer:
		return &object.Float{Value: float64(arg.Value)}
	case *object.String:
		val, err := strconv.ParseFloat(arg.Value, 64)
		if err != nil {
			return object.NewError("cannot convert '%s' to float", arg.Value)
		}
		return &object.Float{Value: val}
	default:
		return object.NewError("cannot convert %s to float", arg.Type())
	}
}

func builtinBool(args ...object.Object) object.Object {
	if len(args) != 1 {
		return object.NewError("wrong number of arguments for bool: got=%d, want=1", len(args))
	}

	switch arg := args[0].(type) {
	case *object.Boolean:
		return arg
	case *object.Integer:
		return object.NativeBoolToBooleanObject(arg.Value != 0)
	case *object.Float:
		return object.NativeBoolToBooleanObject(arg.Value != 0.0)
	case *object.String:
		return object.NativeBoolToBooleanObject(len(arg.Value) > 0)
	default:
		return object.NativeBoolToBooleanObject(true)
	}
}

func builtinAbs(args ...object.Object) object.Object {
	if len(args) != 1 {
		return object.NewError("wrong number of arguments for abs: got=%d, want=1", len(args))
	}

	switch arg := args[0].(type) {
	case *object.Integer:
		if arg.Value < 0 {
			return &object.Integer{Value: -arg.Value}
		}
		return arg
	case *object.Float:
		return &object.Float{Value: math.Abs(arg.Value)}
	default:
		return object.NewError("argument to 'abs' must be numeric, got %s", arg.Type())
	}
}

func builtinMin(args ...object.Object) object.Object {
	if len(args) < 2 {
		return object.NewError("wrong number of arguments for min: got=%d, want>=2", len(args))
	}

	var minVal float64
	var isInt bool = true

	for i, arg := range args {
		var val float64
		switch a := arg.(type) {
		case *object.Integer:
			val = float64(a.Value)
		case *object.Float:
			val = a.Value
			isInt = false
		default:
			return object.NewError("argument %d to 'min' must be numeric, got %s", i, a.Type())
		}

		if i == 0 || val < minVal {
			minVal = val
		}
	}

	if isInt {
		return &object.Integer{Value: int64(minVal)}
	}
	return &object.Float{Value: minVal}
}

func builtinMax(args ...object.Object) object.Object {
	if len(args) < 2 {
		return object.NewError("wrong number of arguments for max: got=%d, want>=2", len(args))
	}

	var maxVal float64
	var isInt bool = true

	for i, arg := range args {
		var val float64
		switch a := arg.(type) {
		case *object.Integer:
			val = float64(a.Value)
		case *object.Float:
			val = a.Value
			isInt = false
		default:
			return object.NewError("argument %d to 'max' must be numeric, got %s", i, a.Type())
		}

		if i == 0 || val > maxVal {
			maxVal = val
		}
	}

	if isInt {
		return &object.Integer{Value: int64(maxVal)}
	}
	return &object.Float{Value: maxVal}
}

func builtinFloor(args ...object.Object) object.Object {
	if len(args) != 1 {
		return object.NewError("wrong number of arguments for floor: got=%d, want=1", len(args))
	}

	switch arg := args[0].(type) {
	case *object.Integer:
		return arg
	case *object.Float:
		return &object.Integer{Value: int64(math.Floor(arg.Value))}
	default:
		return object.NewError("argument to 'floor' must be numeric, got %s", arg.Type())
	}
}

func builtinCeil(args ...object.Object) object.Object {
	if len(args) != 1 {
		return object.NewError("wrong number of arguments for ceil: got=%d, want=1", len(args))
	}

	switch arg := args[0].(type) {
	case *object.Integer:
		return arg
	case *object.Float:
		return &object.Integer{Value: int64(math.Ceil(arg.Value))}
	default:
		return object.NewError("argument to 'ceil' must be numeric, got %s", arg.Type())
	}
}

func builtinSqrt(args ...object.Object) object.Object {
	if len(args) != 1 {
		return object.NewError("wrong number of arguments for sqrt: got=%d, want=1", len(args))
	}

	var val float64
	switch arg := args[0].(type) {
	case *object.Integer:
		val = float64(arg.Value)
	case *object.Float:
		val = arg.Value
	default:
		return object.NewError("argument to 'sqrt' must be numeric, got %s", arg.Type())
	}

	return &object.Float{Value: math.Sqrt(val)}
}

func builtinPow(args ...object.Object) object.Object {
	if len(args) != 2 {
		return object.NewError("wrong number of arguments for pow: got=%d, want=2", len(args))
	}

	var base, exp float64

	switch arg := args[0].(type) {
	case *object.Integer:
		base = float64(arg.Value)
	case *object.Float:
		base = arg.Value
	default:
		return object.NewError("first argument to 'pow' must be numeric, got %s", arg.Type())
	}

	switch arg := args[1].(type) {
	case *object.Integer:
		exp = float64(arg.Value)
	case *object.Float:
		exp = arg.Value
	default:
		return object.NewError("second argument to 'pow' must be numeric, got %s", arg.Type())
	}

	return &object.Float{Value: math.Pow(base, exp)}
}

func builtinSin(args ...object.Object) object.Object {
	if len(args) != 1 {
		return object.NewError("wrong number of arguments for sin: got=%d, want=1", len(args))
	}

	var val float64
	switch arg := args[0].(type) {
	case *object.Integer:
		val = float64(arg.Value)
	case *object.Float:
		val = arg.Value
	default:
		return object.NewError("argument to 'sin' must be numeric, got %s", arg.Type())
	}

	return &object.Float{Value: math.Sin(val)}
}

func builtinCos(args ...object.Object) object.Object {
	if len(args) != 1 {
		return object.NewError("wrong number of arguments for cos: got=%d, want=1", len(args))
	}

	var val float64
	switch arg := args[0].(type) {
	case *object.Integer:
		val = float64(arg.Value)
	case *object.Float:
		val = arg.Value
	default:
		return object.NewError("argument to 'cos' must be numeric, got %s", arg.Type())
	}

	return &object.Float{Value: math.Cos(val)}
}

func builtinPush(args ...object.Object) object.Object {
	if len(args) != 2 {
		return object.NewError("wrong number of arguments for push: got=%d, want=2", len(args))
	}

	arr, ok := args[0].(*object.Array)
	if !ok {
		return object.NewError("first argument to 'push' must be array, got %s", args[0].Type())
	}

	arr.Elements = append(arr.Elements, args[1])
	return arr
}

func builtinPop(args ...object.Object) object.Object {
	if len(args) != 1 {
		return object.NewError("wrong number of arguments for pop: got=%d, want=1", len(args))
	}

	arr, ok := args[0].(*object.Array)
	if !ok {
		return object.NewError("argument to 'pop' must be array, got %s", args[0].Type())
	}

	if len(arr.Elements) == 0 {
		return object.NULL
	}

	last := arr.Elements[len(arr.Elements)-1]
	arr.Elements = arr.Elements[:len(arr.Elements)-1]
	return last
}

func builtinShift(args ...object.Object) object.Object {
	if len(args) != 1 {
		return object.NewError("wrong number of arguments for shift: got=%d, want=1", len(args))
	}

	arr, ok := args[0].(*object.Array)
	if !ok {
		return object.NewError("argument to 'shift' must be array, got %s", args[0].Type())
	}

	if len(arr.Elements) == 0 {
		return object.NULL
	}

	first := arr.Elements[0]
	arr.Elements = arr.Elements[1:]
	return first
}

func builtinSlice(args ...object.Object) object.Object {
	if len(args) < 2 || len(args) > 3 {
		return object.NewError("wrong number of arguments for slice: got=%d, want=2 or 3", len(args))
	}

	arr, ok := args[0].(*object.Array)
	if !ok {
		return object.NewError("first argument to 'slice' must be array, got %s", args[0].Type())
	}

	start, ok := args[1].(*object.Integer)
	if !ok {
		return object.NewError("second argument to 'slice' must be integer, got %s", args[1].Type())
	}

	startIdx := int(start.Value)
	if startIdx < 0 {
		startIdx = len(arr.Elements) + startIdx
	}

	endIdx := len(arr.Elements)
	if len(args) == 3 {
		end, ok := args[2].(*object.Integer)
		if !ok {
			return object.NewError("third argument to 'slice' must be integer, got %s", args[2].Type())
		}
		endIdx = int(end.Value)
		if endIdx < 0 {
			endIdx = len(arr.Elements) + endIdx
		}
	}

	if startIdx < 0 {
		startIdx = 0
	}
	if endIdx > len(arr.Elements) {
		endIdx = len(arr.Elements)
	}

	return &object.Array{Elements: arr.Elements[startIdx:endIdx]}
}

func builtinJoin(args ...object.Object) object.Object {
	if len(args) != 2 {
		return object.NewError("wrong number of arguments for join: got=%d, want=2", len(args))
	}

	arr, ok := args[0].(*object.Array)
	if !ok {
		return object.NewError("first argument to 'join' must be array, got %s", args[0].Type())
	}

	sep, ok := args[1].(*object.String)
	if !ok {
		return object.NewError("second argument to 'join' must be string, got %s", args[1].Type())
	}

	strs := make([]string, len(arr.Elements))
	for i, el := range arr.Elements {
		strs[i] = el.Inspect()
	}

	return &object.String{Value: strings.Join(strs, sep.Value)}
}

func builtinConcat(args ...object.Object) object.Object {
	if len(args) < 2 {
		return object.NewError("wrong number of arguments for concat: got=%d, want>=2", len(args))
	}

	var result []object.Object
	for _, arg := range args {
		arr, ok := arg.(*object.Array)
		if !ok {
			return object.NewError("arguments to 'concat' must be arrays, got %s", arg.Type())
		}
		result = append(result, arr.Elements...)
	}

	return &object.Array{Elements: result}
}

func builtinSplit(args ...object.Object) object.Object {
	if len(args) != 2 {
		return object.NewError("wrong number of arguments for split: got=%d, want=2", len(args))
	}

	str, ok := args[0].(*object.String)
	if !ok {
		return object.NewError("first argument to 'split' must be string, got %s", args[0].Type())
	}

	sep, ok := args[1].(*object.String)
	if !ok {
		return object.NewError("second argument to 'split' must be string, got %s", args[1].Type())
	}

	parts := strings.Split(str.Value, sep.Value)
	elements := make([]object.Object, len(parts))
	for i, p := range parts {
		elements[i] = &object.String{Value: p}
	}

	return &object.Array{Elements: elements}
}

func builtinTrim(args ...object.Object) object.Object {
	if len(args) != 1 {
		return object.NewError("wrong number of arguments for trim: got=%d, want=1", len(args))
	}

	str, ok := args[0].(*object.String)
	if !ok {
		return object.NewError("argument to 'trim' must be string, got %s", args[0].Type())
	}

	return &object.String{Value: strings.TrimSpace(str.Value)}
}

func builtinUpper(args ...object.Object) object.Object {
	if len(args) != 1 {
		return object.NewError("wrong number of arguments for upper: got=%d, want=1", len(args))
	}

	str, ok := args[0].(*object.String)
	if !ok {
		return object.NewError("argument to 'upper' must be string, got %s", args[0].Type())
	}

	return &object.String{Value: strings.ToUpper(str.Value)}
}

func builtinLower(args ...object.Object) object.Object {
	if len(args) != 1 {
		return object.NewError("wrong number of arguments for lower: got=%d, want=1", len(args))
	}

	str, ok := args[0].(*object.String)
	if !ok {
		return object.NewError("argument to 'lower' must be string, got %s", args[0].Type())
	}

	return &object.String{Value: strings.ToLower(str.Value)}
}

func builtinContains(args ...object.Object) object.Object {
	if len(args) != 2 {
		return object.NewError("wrong number of arguments for contains: got=%d, want=2", len(args))
	}

	str, ok := args[0].(*object.String)
	if !ok {
		return object.NewError("first argument to 'contains' must be string, got %s", args[0].Type())
	}

	substr, ok := args[1].(*object.String)
	if !ok {
		return object.NewError("second argument to 'contains' must be string, got %s", args[1].Type())
	}

	return object.NativeBoolToBooleanObject(strings.Contains(str.Value, substr.Value))
}

func builtinIndexOf(args ...object.Object) object.Object {
	if len(args) != 2 {
		return object.NewError("wrong number of arguments for indexOf: got=%d, want=2", len(args))
	}

	switch arg := args[0].(type) {
	case *object.String:
		substr, ok := args[1].(*object.String)
		if !ok {
			return object.NewError("second argument to 'indexOf' must be string, got %s", args[1].Type())
		}
		return &object.Integer{Value: int64(strings.Index(arg.Value, substr.Value))}

	case *object.Array:
		for i, el := range arg.Elements {
			if objectEqual(el, args[1]) {
				return &object.Integer{Value: int64(i)}
			}
		}
		return &object.Integer{Value: -1}

	default:
		return object.NewError("first argument to 'indexOf' must be string or array, got %s", arg.Type())
	}
}

func builtinReplace(args ...object.Object) object.Object {
	if len(args) < 3 || len(args) > 4 {
		return object.NewError("wrong number of arguments for replace: got=%d, want=3 or 4", len(args))
	}

	str, ok := args[0].(*object.String)
	if !ok {
		return object.NewError("first argument to 'replace' must be string, got %s", args[0].Type())
	}

	old, ok := args[1].(*object.String)
	if !ok {
		return object.NewError("second argument to 'replace' must be string, got %s", args[1].Type())
	}

	new, ok := args[2].(*object.String)
	if !ok {
		return object.NewError("third argument to 'replace' must be string, got %s", args[2].Type())
	}

	if len(args) == 4 {
		n, ok := args[3].(*object.Integer)
		if !ok {
			return object.NewError("fourth argument to 'replace' must be integer, got %s", args[3].Type())
		}
		return &object.String{Value: strings.Replace(str.Value, old.Value, new.Value, int(n.Value))}
	}

	return &object.String{Value: strings.ReplaceAll(str.Value, old.Value, new.Value)}
}

func builtinTime(args ...object.Object) object.Object {
	return &object.Integer{Value: time.Now().UnixNano() / 1000000}
}

func builtinSleep(args ...object.Object) object.Object {
	if len(args) != 1 {
		return object.NewError("wrong number of arguments for sleep: got=%d, want=1", len(args))
	}

	var ms int64
	switch arg := args[0].(type) {
	case *object.Integer:
		ms = arg.Value
	case *object.Float:
		ms = int64(arg.Value)
	default:
		return object.NewError("argument to 'sleep' must be numeric, got %s", arg.Type())
	}

	time.Sleep(time.Duration(ms) * time.Millisecond)
	return object.NULL
}

func builtinExit(args ...object.Object) object.Object {
	code := 0
	if len(args) > 0 {
		if c, ok := args[0].(*object.Integer); ok {
			code = int(c.Value)
		}
	}
	os.Exit(code)
	return object.NULL
}

func builtinKeys(args ...object.Object) object.Object {
	if len(args) != 1 {
		return object.NewError("wrong number of arguments for keys: got=%d, want=1", len(args))
	}

	m, ok := args[0].(*object.Map)
	if !ok {
		return object.NewError("argument to 'keys' must be map, got %s", args[0].Type())
	}

	keys := make([]object.Object, 0, len(m.Pairs))
	for _, pair := range m.Pairs {
		keys = append(keys, pair.Key)
	}

	return &object.Array{Elements: keys}
}

func builtinValues(args ...object.Object) object.Object {
	if len(args) != 1 {
		return object.NewError("wrong number of arguments for values: got=%d, want=1", len(args))
	}

	m, ok := args[0].(*object.Map)
	if !ok {
		return object.NewError("argument to 'values' must be map, got %s", args[0].Type())
	}

	values := make([]object.Object, 0, len(m.Pairs))
	for _, pair := range m.Pairs {
		values = append(values, pair.Value)
	}

	return &object.Array{Elements: values}
}

func builtinDelete(args ...object.Object) object.Object {
	if len(args) != 2 {
		return object.NewError("wrong number of arguments for delete: got=%d, want=2", len(args))
	}

	m, ok := args[0].(*object.Map)
	if !ok {
		return object.NewError("first argument to 'delete' must be map, got %s", args[0].Type())
	}

	hashable, ok := args[1].(object.Hashable)
	if !ok {
		return object.NewError("second argument to 'delete' must be hashable, got %s", args[1].Type())
	}

	delete(m.Pairs, hashable.HashKey())
	return object.NULL
}

func builtinHas(args ...object.Object) object.Object {
	if len(args) != 2 {
		return object.NewError("wrong number of arguments for has: got=%d, want=2", len(args))
	}

	m, ok := args[0].(*object.Map)
	if !ok {
		return object.NewError("first argument to 'has' must be map, got %s", args[0].Type())
	}

	hashable, ok := args[1].(object.Hashable)
	if !ok {
		return object.NewError("second argument to 'has' must be hashable, got %s", args[1].Type())
	}

	_, exists := m.Pairs[hashable.HashKey()]
	return object.NativeBoolToBooleanObject(exists)
}

func builtinRange(args ...object.Object) object.Object {
	if len(args) < 1 || len(args) > 2 {
		return object.NewError("wrong number of arguments for range: got=%d, want=1 or 2", len(args))
	}

	var start, end int64
	start = 0

	switch arg := args[0].(type) {
	case *object.Integer:
		end = arg.Value
	default:
		return object.NewError("argument to 'range' must be integer, got %s", arg.Type())
	}

	if len(args) == 2 {
		switch arg := args[0].(type) {
		case *object.Integer:
			start = arg.Value
		}
		if s, ok := args[1].(*object.Integer); ok {
			end = s.Value
		}
	}

	elements := make([]object.Object, end-start)
	for i := start; i < end; i++ {
		elements[i-start] = &object.Integer{Value: i}
	}

	return &object.Array{Elements: elements}
}

func builtinAppend(args ...object.Object) object.Object {
	if len(args) < 2 {
		return object.NewError("wrong number of arguments for append: got=%d, want>=2", len(args))
	}

	arr, ok := args[0].(*object.Array)
	if !ok {
		return object.NewError("first argument to 'append' must be array, got %s", args[0].Type())
	}

	newArr := &object.Array{Elements: make([]object.Object, len(arr.Elements))}
	copy(newArr.Elements, arr.Elements)
	newArr.Elements = append(newArr.Elements, args[1:]...)

	return newArr
}

func builtinError(args ...object.Object) object.Object {
	if len(args) != 1 {
		return object.NewError("wrong number of arguments for error: got=%d, want=1", len(args))
	}

	return object.NewError(args[0].Inspect())
}

// builtinLoadText reads a text file and returns its contents as a string.
// loadText(path) - reads the file at path and returns a String or Error.
func builtinLoadText(args ...object.Object) object.Object {
	if len(args) != 1 {
		return object.NewError("wrong number of arguments for loadText: got=%d, want=1", len(args))
	}

	path, ok := args[0].(*object.String)
	if !ok {
		return object.NewError("argument to 'loadText' must be string, got %s", args[0].Type())
	}

	content, err := os.ReadFile(path.Value)
	if err != nil {
		return object.NewError("failed to read file '%s': %s", path.Value, err.Error())
	}

	return &object.String{Value: string(content)}
}

// builtinSaveText writes content to a text file.
// saveText(path, content) - writes content to path, returns NULL or Error.
func builtinSaveText(args ...object.Object) object.Object {
	if len(args) != 2 {
		return object.NewError("wrong number of arguments for saveText: got=%d, want=2", len(args))
	}

	path, ok := args[0].(*object.String)
	if !ok {
		return object.NewError("first argument to 'saveText' must be string, got %s", args[0].Type())
	}

	content, ok := args[1].(*object.String)
	if !ok {
		return object.NewError("second argument to 'saveText' must be string, got %s", args[1].Type())
	}

	err := os.WriteFile(path.Value, []byte(content.Value), 0644)
	if err != nil {
		return object.NewError("failed to write file '%s': %s", path.Value, err.Error())
	}

	return object.NULL
}

// builtinGetSwitch extracts a switch value from an arguments array.
// getSwitch(args, switchName, default) - looks for "--name=value" or "-name=value" pattern.
func builtinGetSwitch(args ...object.Object) object.Object {
	if len(args) != 3 {
		return object.NewError("wrong number of arguments for getSwitch: got=%d, want=3", len(args))
	}

	arr, ok := args[0].(*object.Array)
	if !ok {
		return object.NewError("first argument to 'getSwitch' must be array, got %s", args[0].Type())
	}

	switchName, ok := args[1].(*object.String)
	if !ok {
		return object.NewError("second argument to 'getSwitch' must be string, got %s", args[1].Type())
	}

	defaultValue := args[2]

	// Look for switchName in the array
	for _, elem := range arr.Elements {
		if str, ok := elem.(*object.String); ok {
			// Check if the string starts with the switch name
			if strings.HasPrefix(str.Value, switchName.Value) {
				// Extract the value after the switch name
				return &object.String{Value: strings.TrimPrefix(str.Value, switchName.Value)}
			}
		}
	}

	return defaultValue
}

// builtinFatalf prints an error message and exits the program.
// fatalf(format, ...args) - prints formatted message and calls os.Exit(1).
func builtinFatalf(args ...object.Object) object.Object {
	if len(args) < 1 {
		return object.NewError("wrong number of arguments for fatalf: got=%d, want>=1", len(args))
	}

	// Build the error message
	var msg string
	if formatStr, ok := args[0].(*object.String); ok {
		// Use pl-style formatting
		format := formatStr.Value
		argIndex := 1
		var result strings.Builder

		for i := 0; i < len(format); i++ {
			if format[i] == '%' && i+1 < len(format) {
				spec := format[i+1]
				switch spec {
				case 'v', 's':
					if argIndex >= len(args) {
						result.WriteByte(format[i])
					} else {
						result.WriteString(args[argIndex].Inspect())
						argIndex++
					}
					i++
				case 'd':
					if argIndex >= len(args) {
						result.WriteByte(format[i])
					} else {
						switch arg := args[argIndex].(type) {
						case *object.Integer:
							result.WriteString(strconv.FormatInt(arg.Value, 10))
						case *object.Float:
							result.WriteString(strconv.FormatInt(int64(arg.Value), 10))
						default:
							result.WriteString(arg.Inspect())
						}
						argIndex++
					}
					i++
				case '%':
					result.WriteByte('%')
					i++
				default:
					result.WriteByte(format[i])
				}
			} else {
				result.WriteByte(format[i])
			}
		}
		msg = result.String()
	} else {
		msg = args[0].Inspect()
	}

	fmt.Fprintln(os.Stderr, "FATAL:", msg)
	os.Exit(1)
	return object.NULL
}

// builtinCheckErr checks if the argument is an error and panics if so.
// checkErr(err) - if err is an Error type, throws it; otherwise returns NULL.
func builtinCheckErr(args ...object.Object) object.Object {
	if len(args) != 1 {
		return object.NewError("wrong number of arguments for checkErr: got=%d, want=1", len(args))
	}

	// Check if it's an Error type
	if err, ok := args[0].(*object.Error); ok {
		// Panic with the error message
		panic(err.Message)
	}

	return object.NULL
}

// builtinSubStr returns a substring of the given string.
// subStr(s, start, len) - returns substring from start with given length.
// Uses UTF-8 rune counting for proper Unicode support.
func builtinSubStr(args ...object.Object) object.Object {
	if len(args) < 2 || len(args) > 3 {
		return object.NewError("wrong number of arguments for subStr: got=%d, want=2 or 3", len(args))
	}

	str, ok := args[0].(*object.String)
	if !ok {
		return object.NewError("first argument to 'subStr' must be string, got %s", args[0].Type())
	}

	start, ok := args[1].(*object.Integer)
	if !ok {
		return object.NewError("second argument to 'subStr' must be integer, got %s", args[1].Type())
	}

	// Convert string to runes for proper Unicode handling
	runes := []rune(str.Value)
	strLen := len(runes)

	startIdx := int(start.Value)
	if startIdx < 0 {
		startIdx = strLen + startIdx
	}

	endIdx := strLen
	if len(args) == 3 {
		length, ok := args[2].(*object.Integer)
		if !ok {
			return object.NewError("third argument to 'subStr' must be integer, got %s", args[2].Type())
		}
		endIdx = startIdx + int(length.Value)
	}

	// Clamp indices
	if startIdx < 0 {
		startIdx = 0
	}
	if endIdx > strLen {
		endIdx = strLen
	}
	if startIdx > endIdx {
		startIdx = endIdx
	}

	return &object.String{Value: string(runes[startIdx:endIdx])}
}

// Helper function
func objectEqual(a, b object.Object) bool {
	switch a := a.(type) {
	case *object.Integer:
		b, ok := b.(*object.Integer)
		return ok && a.Value == b.Value
	case *object.Float:
		switch b := b.(type) {
		case *object.Float:
			return a.Value == b.Value
		case *object.Integer:
			return a.Value == float64(b.Value)
		}
	case *object.String:
		b, ok := b.(*object.String)
		return ok && a.Value == b.Value
	case *object.Boolean:
		b, ok := b.(*object.Boolean)
		return ok && a.Value == b.Value
	}
	return false
}
