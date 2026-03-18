package object

import (
	"bytes"
	"strings"
)

// Array represents an array of objects in Sflang.
// It wraps a Go slice of Object values and implements the Object interface.
// Arrays are dynamic and can grow or shrink in size.
type Array struct {
	Elements []Object
}

// Type returns the ObjectType for Array.
func (a *Array) Type() ObjectType { return ARRAY_OBJ }

// Inspect returns a string representation of the array elements.
// Format: [element1, element2, ...]
func (a *Array) Inspect() string {
	var out bytes.Buffer
	elements := make([]string, 0, len(a.Elements))
	for _, e := range a.Elements {
		elements = append(elements, e.Inspect())
	}
	out.WriteString("[")
	out.WriteString(strings.Join(elements, ", "))
	out.WriteString("]")
	return out.String()
}

// TypeCode returns the fixed numeric type code for Array.
func (a *Array) TypeCode() int { return TypeCodeArray }

// TypeName returns the human-readable type name.
func (a *Array) TypeName() string { return "array" }

// Len returns the number of elements in the array.
func (a *Array) Len() int {
	return len(a.Elements)
}

// Get returns the element at the given index.
// Returns nil if the index is out of bounds.
func (a *Array) Get(index int) Object {
	if index < 0 || index >= len(a.Elements) {
		return nil
	}
	return a.Elements[index]
}

// Set sets the element at the given index.
// Returns false if the index is out of bounds.
func (a *Array) Set(index int, value Object) bool {
	if index < 0 || index >= len(a.Elements) {
		return false
	}
	a.Elements[index] = value
	return true
}

// Append adds elements to the end of the array.
func (a *Array) Append(elements ...Object) {
	a.Elements = append(a.Elements, elements...)
}
