package object

import (
	"bytes"
	"fmt"
)

// CompiledFunction represents a compiled function ready for VM execution.
// It contains the compiled bytecode instructions and references to
// any free variables captured from outer scopes.
type CompiledFunction struct {
	Instructions  []byte
	NumLocals     int
	NumParameters int
}

// Type returns the ObjectType for CompiledFunction.
func (cf *CompiledFunction) Type() ObjectType { return FUNCTION_OBJ }

// Inspect returns a string representation of the compiled function.
func (cf *CompiledFunction) Inspect() string {
	return fmt.Sprintf("CompiledFunction[%d locals, %d params]", cf.NumLocals, cf.NumParameters)
}

// TypeCode returns the fixed numeric type code for CompiledFunction.
func (cf *CompiledFunction) TypeCode() int { return TypeCodeFunction }

// TypeName returns the human-readable type name.
func (cf *CompiledFunction) TypeName() string { return "function" }

// Closure represents a function closure with captured free variables.
// When a function references variables from outer scopes, those
// variables are captured and stored in the closure.
type Closure struct {
	Fn      *CompiledFunction
	Free    []Object
}

// Type returns the ObjectType for Closure.
func (c *Closure) Type() ObjectType { return CLOSURE_OBJ }

// Inspect returns a string representation of the closure.
func (c *Closure) Inspect() string {
	var out bytes.Buffer
	out.WriteString("Closure[")
	out.WriteString(c.Fn.Inspect())
	out.WriteString(fmt.Sprintf(", %d free vars]", len(c.Free)))
	return out.String()
}

// TypeCode returns the fixed numeric type code for Closure.
func (c *Closure) TypeCode() int { return TypeCodeClosure }

// TypeName returns the human-readable type name.
func (c *Closure) TypeName() string { return "closure" }
