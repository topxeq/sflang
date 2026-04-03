package compiler

// SymbolScope represents the scope of a symbol.
type SymbolScope string

const (
	GlobalScope  SymbolScope = "GLOBAL"
	LocalScope   SymbolScope = "LOCAL"
	BuiltinScope SymbolScope = "BUILTIN"
	FreeScope    SymbolScope = "FREE"
)

// Symbol represents a named binding in the symbol table.
type Symbol struct {
	Name  string
	Scope SymbolScope
	Index int
}

// SymbolTable manages variable scopes and their indices.
type SymbolTable struct {
	Outer          *SymbolTable
	store          map[string]Symbol
	numDefinitions int
	FreeSymbols    []Symbol
}

// NewSymbolTable creates a new symbol table.
func NewSymbolTable() *SymbolTable {
	s := make(map[string]Symbol)
	free := []Symbol{}
	return &SymbolTable{store: s, FreeSymbols: free}
}

// NewEnclosedSymbolTable creates a symbol table enclosed by an outer one.
func NewEnclosedSymbolTable(outer *SymbolTable) *SymbolTable {
	s := NewSymbolTable()
	s.Outer = outer
	return s
}

// NewEnclosedSymbolTableWithOffset creates a symbol table enclosed by an outer one,
// with an initial numDefinitions offset. This is useful for for-in loops that
// don't create new frames but need local variables with non-conflicting indices.
func NewEnclosedSymbolTableWithOffset(outer *SymbolTable, offset int) *SymbolTable {
	s := NewSymbolTable()
	s.Outer = outer
	s.numDefinitions = offset
	return s
}

// Define adds a new symbol to the symbol table.
func (s *SymbolTable) Define(name string) Symbol {
	symbol := Symbol{Name: name, Index: s.numDefinitions}
	if s.Outer == nil {
		symbol.Scope = GlobalScope
	} else {
		symbol.Scope = LocalScope
	}
	s.store[name] = symbol
	s.numDefinitions++
	return symbol
}

// DefineBuiltin adds a built-in function symbol to the symbol table.
func (s *SymbolTable) DefineBuiltin(index int, name string) Symbol {
	symbol := Symbol{Name: name, Index: index, Scope: BuiltinScope}
	s.store[name] = symbol
	return symbol
}

// Resolve looks up a symbol by name, searching outer scopes if necessary.
func (s *SymbolTable) Resolve(name string) (Symbol, bool) {
	obj, ok := s.store[name]
	if !ok && s.Outer != nil {
		obj, ok = s.Outer.Resolve(name)
		if !ok {
			return obj, false
		}
		// If found in outer scope, decide whether to add to free symbols
		switch obj.Scope {
		case GlobalScope:
			// Global symbols are accessed directly via OpGetGlobal
			// Don't add to free symbols
			return obj, true
		case BuiltinScope:
			// Built-in symbols are accessed directly via OpGetBuiltin
			return obj, true
		case LocalScope:
			// Local symbols from outer scope need to be captured as free variables
			// Check if already in free symbols
			for _, fs := range s.FreeSymbols {
				if fs.Name == name {
					return Symbol{Name: name, Scope: FreeScope, Index: fs.Index}, true
				}
			}
			s.FreeSymbols = append(s.FreeSymbols, obj)
			freeSymbol := Symbol{Name: name, Scope: FreeScope, Index: len(s.FreeSymbols) - 1}
			s.store[name] = freeSymbol
			return freeSymbol, true
		case FreeScope:
			// Already a free variable in outer scope, propagate it
			// Check if already in free symbols
			for _, fs := range s.FreeSymbols {
				if fs.Name == name {
					return Symbol{Name: name, Scope: FreeScope, Index: fs.Index}, true
				}
			}
			s.FreeSymbols = append(s.FreeSymbols, obj)
			freeSymbol := Symbol{Name: name, Scope: FreeScope, Index: len(s.FreeSymbols) - 1}
			s.store[name] = freeSymbol
			return freeSymbol, true
		}
		return obj, true
	}
	return obj, ok
}

// NumDefinitions returns the number of definitions in this scope.
func (s *SymbolTable) NumDefinitions() int {
	return s.numDefinitions
}
