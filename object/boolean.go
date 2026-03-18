package object

import "fmt"

// Boolean represents a boolean value in Sflang.
// It wraps a Go bool value and implements the Object interface.
type Boolean struct {
	Value bool
}

// Type returns the ObjectType for Boolean.
func (b *Boolean) Type() ObjectType { return BOOLEAN_OBJ }

// Inspect returns a string representation of the boolean value.
func (b *Boolean) Inspect() string { return fmt.Sprintf("%t", b.Value) }

// TypeCode returns the fixed numeric type code for Boolean.
func (b *Boolean) TypeCode() int { return TypeCodeBoolean }

// TypeName returns the human-readable type name.
func (b *Boolean) TypeName() string { return "boolean" }

// HashKey returns a HashKey for using booleans as map keys.
func (b *Boolean) HashKey() HashKey {
	var value uint64
	if b.Value {
		value = 1
	}
	return HashKey{Type: BOOLEAN_OBJ, Value: value}
}

// Pre-defined boolean singletons for efficiency.
// Use these instead of creating new Boolean objects.
var (
	TRUE  = &Boolean{Value: true}
	FALSE = &Boolean{Value: false}
)

// NativeBoolToBooleanObject converts a Go bool to a Sflang Boolean object.
// Returns the singleton TRUE or FALSE objects for efficiency.
func NativeBoolToBooleanObject(input bool) *Boolean {
	if input {
		return TRUE
	}
	return FALSE
}
