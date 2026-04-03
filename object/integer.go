package object

import "strconv"

// IntegerPoolSize defines the range of pre-allocated integers.
// Integers from -IntegerPoolSize to +IntegerPoolSize are cached.
const IntegerPoolSize = 256

// integerPool holds pre-allocated Integer objects for small values.
var integerPool [2*IntegerPoolSize + 1]*Integer

// init initializes the integer pool.
func init() {
	for i := -IntegerPoolSize; i <= IntegerPoolSize; i++ {
		integerPool[i+IntegerPoolSize] = &Integer{Value: int64(i)}
	}
}

// GetInteger returns an Integer object, using the pool for small values.
// This avoids allocation for commonly used integers.
func GetInteger(value int64) *Integer {
	if value >= -IntegerPoolSize && value <= IntegerPoolSize {
		return integerPool[value+IntegerPoolSize]
	}
	return &Integer{Value: value}
}

// Integer represents an integer value in Sflang.
// It wraps a Go int64 value and implements the Object interface.
type Integer struct {
	Value int64
}

// Type returns the ObjectType for Integer.
func (i *Integer) Type() ObjectType { return INTEGER_OBJ }

// Inspect returns a string representation of the integer value.
func (i *Integer) Inspect() string {	return strconv.FormatInt(i.Value, 10) }

// TypeCode returns the fixed numeric type code for Integer.
func (i *Integer) TypeCode() int { return TypeCodeInteger }

// TypeName returns the human-readable type name.
func (i *Integer) TypeName() string { return "int" }

// HashKey returns a HashKey for using integers as map keys.
func (i *Integer) HashKey() HashKey {
	return HashKey{Type: INTEGER_OBJ, Value: uint64(i.Value)}
}
