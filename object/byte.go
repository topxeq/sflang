package object

import "fmt"

// Byte represents a single byte value (uint8) in Sflang.
// It wraps a Go uint8 value and implements the Object interface.
type Byte struct {
	Value uint8
}

// Type returns the ObjectType for Byte.
func (b *Byte) Type() ObjectType { return BYTE_OBJ }

// Inspect returns a string representation of the byte value.
// Uses hexadecimal format for better readability.
func (b *Byte) Inspect() string { return fmt.Sprintf("0x%02X", b.Value) }

// TypeCode returns the fixed numeric type code for Byte.
func (b *Byte) TypeCode() int { return TypeCodeByte }

// TypeName returns the human-readable type name.
func (b *Byte) TypeName() string { return "byte" }

// HashKey returns a HashKey for using bytes as map keys.
func (b *Byte) HashKey() HashKey {
	return HashKey{Type: BYTE_OBJ, Value: uint64(b.Value)}
}