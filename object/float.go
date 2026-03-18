package object

import (
	"fmt"
	"math"
)

// Float represents a floating-point value in Sflang.
// It wraps a Go float64 value and implements the Object interface.
type Float struct {
	Value float64
}

// Type returns the ObjectType for Float.
func (f *Float) Type() ObjectType { return FLOAT_OBJ }

// Inspect returns a string representation of the float value.
func (f *Float) Inspect() string { return fmt.Sprintf("%g", f.Value) }

// TypeCode returns the fixed numeric type code for Float.
func (f *Float) TypeCode() int { return TypeCodeFloat }

// TypeName returns the human-readable type name.
func (f *Float) TypeName() string { return "float" }

// HashKey returns a HashKey for using floats as map keys.
// Uses the binary representation for consistent hashing.
func (f *Float) HashKey() HashKey {
	return HashKey{Type: FLOAT_OBJ, Value: math.Float64bits(f.Value)}
}
