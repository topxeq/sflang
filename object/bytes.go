package object

import (
	"encoding/hex"
	"fmt"
)

// Bytes represents a byte array ([]byte) in Sflang.
// It wraps a Go []byte slice and implements the Object interface.
type Bytes struct {
	Value []byte
}

// Type returns the ObjectType for Bytes.
func (b *Bytes) Type() ObjectType { return BYTES_OBJ }

// Inspect returns a string representation of the byte array.
// Shows as hex representation for readability.
func (b *Bytes) Inspect() string {
	if len(b.Value) == 0 {
		return "bytes[]"
	}
	if len(b.Value) <= 32 {
		return fmt.Sprintf("bytes[%s]", hex.EncodeToString(b.Value))
	}
	return fmt.Sprintf("bytes[%s...](len=%d)", hex.EncodeToString(b.Value[:32]), len(b.Value))
}

// TypeCode returns the fixed numeric type code for Bytes.
func (b *Bytes) TypeCode() int { return TypeCodeBytes }

// TypeName returns the human-readable type name.
func (b *Bytes) TypeName() string { return "bytes" }

// Len returns the length of the byte array.
func (b *Bytes) Len() int { return len(b.Value) }

// Get returns the byte at the given index, or nil if out of bounds.
func (b *Bytes) Get(index int) Object {
	if index < 0 || index >= len(b.Value) {
		return NULL
	}
	return &Byte{Value: b.Value[index]}
}

// Set sets the byte at the given index. Returns false if out of bounds.
func (b *Bytes) Set(index int, value Object) bool {
	if index < 0 || index >= len(b.Value) {
		return false
	}
	if byteVal, ok := value.(*Byte); ok {
		b.Value[index] = byteVal.Value
		return true
	}
	if intVal, ok := value.(*Integer); ok {
		if intVal.Value >= 0 && intVal.Value <= 255 {
			b.Value[index] = byte(intVal.Value)
			return true
		}
	}
	return false
}

// Append adds bytes to the end of the array.
func (b *Bytes) Append(elements ...Object) {
	for _, elem := range elements {
		switch v := elem.(type) {
		case *Byte:
			b.Value = append(b.Value, v.Value)
		case *Integer:
			if v.Value >= 0 && v.Value <= 255 {
				b.Value = append(b.Value, byte(v.Value))
			}
		}
	}
}