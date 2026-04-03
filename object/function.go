package object

import (
	"bytes"
	"strings"

	"github.com/topxeq/sflang/ast"
)

// Function represents a user-defined function in Sflang.
// It captures the function body as an AST block statement and
// the environment (scope) where the function was defined.
type Function struct {
	Parameters []*ast.Identifier
	Body       *ast.BlockStatement
	Env        *Environment
	Name       string // Optional function name for debugging
}

// Type returns the ObjectType for Function.
func (f *Function) Type() ObjectType { return FUNCTION_OBJ }

// Inspect returns a string representation of the function signature.
// Format: func(param1, param2, ...) { ... }
func (f *Function) Inspect() string {
	var out bytes.Buffer
	params := make([]string, 0, len(f.Parameters))
	for _, p := range f.Parameters {
		params = append(params, p.String())
	}
	out.WriteString("func(")
	out.WriteString(strings.Join(params, ", "))
	out.WriteString(") {\n")
	out.WriteString(f.Body.String())
	out.WriteString("\n}")
	return out.String()
}

// TypeCode returns the fixed numeric type code for Function.
func (f *Function) TypeCode() int { return TypeCodeFunction }

// TypeName returns the human-readable type name.
func (f *Function) TypeName() string { return "function" }
