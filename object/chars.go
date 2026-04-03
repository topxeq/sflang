package object

import "fmt"

// Chars represents a character array ([]rune) in Sflang.
// It wraps a Go []rune slice and implements the Object interface.
type Chars struct {
	Value []rune
}

// Type returns the ObjectType for Chars.
func (c *Chars) Type() ObjectType { return CHARS_OBJ }

// Inspect returns a string representation of the character array.
func (c *Chars) Inspect() string {
	if len(c.Value) == 0 {
		return "chars[]"
	}
	result := "chars["
	for i, r := range c.Value {
		if i > 0 {
			result += ", "
		}
		if i >= 10 {
			result += "..."
			break
		}
		if r >= 32 && r <= 126 {
			result += fmt.Sprintf("'%c'", r)
		} else {
			result += fmt.Sprintf("'\\u%04X'", r)
		}
	}
	result += "]"
	return result
}

// TypeCode returns the fixed numeric type code for Chars.
func (c *Chars) TypeCode() int { return TypeCodeChars }

// TypeName returns the human-readable type name.
func (c *Chars) TypeName() string { return "chars" }

// Len returns the length of the character array.
func (c *Chars) Len() int { return len(c.Value) }

// Get returns the character at the given index, or nil if out of bounds.
func (c *Chars) Get(index int) Object {
	if index < 0 || index >= len(c.Value) {
		return NULL
	}
	return &Char{Value: c.Value[index]}
}

// Set sets the character at the given index. Returns false if out of bounds.
func (c *Chars) Set(index int, value Object) bool {
	if index < 0 || index >= len(c.Value) {
		return false
	}
	if charVal, ok := value.(*Char); ok {
		c.Value[index] = charVal.Value
		return true
	}
	if strVal, ok := value.(*String); ok && len(strVal.Value) > 0 {
		for _, r := range strVal.Value {
			c.Value[index] = r
			break
		}
		return true
	}
	return false
}

// Append adds characters to the end of the array.
func (c *Chars) Append(elements ...Object) {
	for _, elem := range elements {
		switch v := elem.(type) {
		case *Char:
			c.Value = append(c.Value, v.Value)
		case *String:
			for _, r := range v.Value {
				c.Value = append(c.Value, r)
			}
		}
	}
}