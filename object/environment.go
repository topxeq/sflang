package object

// Environment represents a scope for variable bindings.
// It supports nested scopes through the outer field for lexical scoping.
type Environment struct {
	store map[string]Object
	outer *Environment
}

// NewEnvironment creates a new empty environment.
func NewEnvironment() *Environment {
	s := make(map[string]Object)
	return &Environment{store: s, outer: nil}
}

// NewEnclosedEnvironment creates a new environment enclosed by an outer environment.
// This is used for function calls to create a new scope that can access
// variables from the outer (enclosing) scope.
func NewEnclosedEnvironment(outer *Environment) *Environment {
	env := NewEnvironment()
	env.outer = outer
	return env
}

// Get retrieves a variable value by name.
// It searches in the current scope first, then in outer scopes.
// Returns the value and true if found, nil and false otherwise.
func (e *Environment) Get(name string) (Object, bool) {
	obj, ok := e.store[name]
	if !ok && e.outer != nil {
		obj, ok = e.outer.Get(name)
	}
	return obj, ok
}

// Set stores a variable value in the current scope.
// Returns the stored value.
func (e *Environment) Set(name string, val Object) Object {
	e.store[name] = val
	return val
}

// Delete removes a variable from the current scope.
// Returns true if the variable was found and deleted.
func (e *Environment) Delete(name string) bool {
	if _, exists := e.store[name]; exists {
		delete(e.store, name)
		return true
	}
	return false
}

// HasLocal checks if a variable exists in the current (local) scope only.
func (e *Environment) HasLocal(name string) bool {
	_, ok := e.store[name]
	return ok
}

// Outer returns the enclosing environment.
func (e *Environment) Outer() *Environment {
	return e.outer
}
