package object

// ReturnValue wraps an object to signal a return statement.
// The VM uses this to propagate return values up the call stack.
type ReturnValue struct {
	Value Object
}

// Type returns the ObjectType for ReturnValue.
func (rv *ReturnValue) Type() ObjectType { return RETURN_OBJ }

// Inspect returns the string representation of the wrapped value.
func (rv *ReturnValue) Inspect() string { return rv.Value.Inspect() }

// TypeCode returns the fixed numeric type code for ReturnValue.
func (rv *ReturnValue) TypeCode() int { return TypeCodeReturn }

// TypeName returns the human-readable type name.
func (rv *ReturnValue) TypeName() string { return "return" }
