// Package object defines the Object interface and all data types for the Sflang language.
// All values in Sflang implement the Object interface, enabling uniform handling
// of different data types throughout the interpreter.
package object

// ObjectType represents the type of an Object as a string identifier.
type ObjectType string

// Type constants define the string identifiers for each object type.
const (
	INTEGER_OBJ  ObjectType = "INTEGER"
	FLOAT_OBJ    ObjectType = "FLOAT"
	STRING_OBJ   ObjectType = "STRING"
	BOOLEAN_OBJ  ObjectType = "BOOLEAN"
	NULL_OBJ     ObjectType = "NULL"
	ARRAY_OBJ    ObjectType = "ARRAY"
	MAP_OBJ      ObjectType = "MAP"
	FUNCTION_OBJ ObjectType = "FUNCTION"
	BUILTIN_OBJ  ObjectType = "BUILTIN"
	ERROR_OBJ    ObjectType = "ERROR"
	RETURN_OBJ   ObjectType = "RETURN"
	CLOSURE_OBJ  ObjectType = "CLOSURE"
)

// TypeCode constants define fixed numeric codes for each type.
// These are fixed values (not using iota) for version compatibility.
// Type codes 1-20 are reserved for built-in types.
const (
	TypeCodeInteger  = 1
	TypeCodeFloat    = 2
	TypeCodeString   = 3
	TypeCodeBoolean  = 4
	TypeCodeNull     = 5
	TypeCodeArray    = 6
	TypeCodeMap      = 7
	TypeCodeFunction = 8
	TypeCodeBuiltin  = 9
	TypeCodeError    = 10
	TypeCodeReturn   = 11
	TypeCodeClosure  = 12
)

// typeNameMap maps type codes to their string names for fast lookup.
var typeNameMap = map[int]string{
	TypeCodeInteger:  "integer",
	TypeCodeFloat:    "float",
	TypeCodeString:   "string",
	TypeCodeBoolean:  "boolean",
	TypeCodeNull:     "null",
	TypeCodeArray:    "array",
	TypeCodeMap:      "map",
	TypeCodeFunction: "function",
	TypeCodeBuiltin:  "builtin",
	TypeCodeError:    "error",
	TypeCodeReturn:   "return",
	TypeCodeClosure:  "closure",
}

// GetTypeName returns the string name for a given type code.
// Returns "unknown" if the type code is not recognized.
func GetTypeName(code int) string {
	if name, ok := typeNameMap[code]; ok {
		return name
	}
	return "unknown"
}

// Object is the interface that all Sflang values must implement.
// This provides a uniform way to handle different data types throughout the interpreter.
type Object interface {
	// Type returns the ObjectType string identifier.
	Type() ObjectType
	// Inspect returns a string representation of the object's value.
	Inspect() string
	// TypeCode returns the fixed numeric type code for fast type discrimination.
	TypeCode() int
	// TypeName returns the human-readable type name string.
	TypeName() string
}

// Hashable is an interface for objects that can be used as map keys.
// Objects implementing this interface can be hashed for use in maps and sets.
type Hashable interface {
	HashKey() HashKey
}

// HashKey represents a hashable key value for use in maps.
type HashKey struct {
	Type  ObjectType
	Value uint64
}
