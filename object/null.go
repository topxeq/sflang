package object

// Null represents the null/undefined value in Sflang.
// It is a singleton object representing the absence of a value.
type Null struct{}

// Type returns the ObjectType for Null.
func (n *Null) Type() ObjectType { return NULL_OBJ }

// Inspect returns the string representation "null".
func (n *Null) Inspect() string { return "null" }

// TypeCode returns the fixed numeric type code for Null.
func (n *Null) TypeCode() int { return TypeCodeNull }

// TypeName returns the human-readable type name.
func (n *Null) TypeName() string { return "null" }

// NULL is the singleton Null object.
// Use this instead of creating new Null objects.
var NULL = &Null{}

// GetNull returns the singleton NULL object.
func GetNull() *Null {
	return NULL
}
