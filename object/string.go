package object

import (
	"fmt"
	"hash/fnv"
)

// String represents a string value in Sflang.
// It wraps a Go string value and implements the Object interface.
// All strings in Sflang are UTF-8 encoded.
type String struct {
	Value string
}

// Type returns the ObjectType for String.
func (s *String) Type() ObjectType { return STRING_OBJ }

// Inspect returns a quoted string representation.
func (s *String) Inspect() string { return fmt.Sprintf("%q", s.Value) }

// TypeCode returns the fixed numeric type code for String.
func (s *String) TypeCode() int { return TypeCodeString }

// TypeName returns the human-readable type name.
func (s *String) TypeName() string { return "string" }

// HashKey returns a HashKey for using strings as map keys.
// Uses FNV-1a hash algorithm for consistent hashing.
func (s *String) HashKey() HashKey {
	h := fnv.New64a()
	h.Write([]byte(s.Value))
	return HashKey{Type: STRING_OBJ, Value: h.Sum64()}
}
