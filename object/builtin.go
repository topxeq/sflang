package object

// BuiltinFunction is the type for built-in functions in Sflang.
// Built-in functions are implemented in Go and can be called from Sflang code.
type BuiltinFunction func(args ...Object) Object

// Builtin wraps a built-in function and implements the Object interface.
type Builtin struct {
	Fn BuiltinFunction
}

// Type returns the ObjectType for Builtin.
func (b *Builtin) Type() ObjectType { return BUILTIN_OBJ }

// Inspect returns a string representation of the built-in function.
func (b *Builtin) Inspect() string { return "builtin function" }

// TypeCode returns the fixed numeric type code for Builtin.
func (b *Builtin) TypeCode() int { return TypeCodeBuiltin }

// TypeName returns the human-readable type name.
func (b *Builtin) TypeName() string { return "builtin" }

// NewBuiltin creates a new Builtin object wrapping the given function.
func NewBuiltin(fn BuiltinFunction) *Builtin {
	return &Builtin{Fn: fn}
}

// Builtins holds all registered built-in functions.
// This will be populated by the builtin package.
var Builtins []*Builtin

// RegisterBuiltins registers built-in functions.
func RegisterBuiltins(builtins []*Builtin) {
	Builtins = builtins
}
