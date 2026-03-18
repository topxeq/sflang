package object

import "fmt"

// Error represents an error value in Sflang.
// It wraps an error message and implements the Object interface.
type Error struct {
	Message string
}

// Type returns the ObjectType for Error.
func (e *Error) Type() ObjectType { return ERROR_OBJ }

// Inspect returns a formatted error message with "ERROR: " prefix.
func (e *Error) Inspect() string { return fmt.Sprintf("ERROR: %s", e.Message) }

// TypeCode returns the fixed numeric type code for Error.
func (e *Error) TypeCode() int { return TypeCodeError }

// TypeName returns the human-readable type name.
func (e *Error) TypeName() string { return "error" }

// NewError creates a new Error object with the given message.
func NewError(format string, args ...interface{}) *Error {
	return &Error{Message: fmt.Sprintf(format, args...)}
}
