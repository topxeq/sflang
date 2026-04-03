package object

import "fmt"

// Char represents a single Unicode character (rune) in Sflang.
// It wraps a Go rune value and implements the Object interface.
type Char struct {
	Value rune
}

// Type returns the ObjectType for Char.
func (c *Char) Type() ObjectType { return CHAR_OBJ }

// Inspect returns a string representation of the character.
// Uses single quotes for printable characters, Unicode escape for others.
func (c *Char) Inspect() string {
	if c.Value >= 32 && c.Value <= 126 {
		return fmt.Sprintf("'%c'", c.Value)
	}
	return fmt.Sprintf("'\\u%04X'", c.Value)
}

// TypeCode returns the fixed numeric type code for Char.
func (c *Char) TypeCode() int { return TypeCodeChar }

// TypeName returns the human-readable type name.
func (c *Char) TypeName() string { return "char" }

// HashKey returns a HashKey for using chars as map keys.
func (c *Char) HashKey() HashKey {
	return HashKey{Type: CHAR_OBJ, Value: uint64(c.Value)}
}