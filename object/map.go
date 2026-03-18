package object

import (
	"bytes"
	"fmt"
	"strings"
)

// MapKeyPair stores the hash key and the original key object.
type MapKeyPair struct {
	Key   Object
	Value Object
}

// Map represents a hash map of key-value pairs in Sflang.
// Keys must be hashable (implement the Hashable interface).
type Map struct {
	Pairs map[HashKey]MapKeyPair
}

// Type returns the ObjectType for Map.
func (m *Map) Type() ObjectType { return MAP_OBJ }

// Inspect returns a string representation of the map pairs.
// Format: {key1: value1, key2: value2, ...}
func (m *Map) Inspect() string {
	var out bytes.Buffer
	pairs := make([]string, 0, len(m.Pairs))
	for _, pair := range m.Pairs {
		pairs = append(pairs, fmt.Sprintf("%s: %s", pair.Key.Inspect(), pair.Value.Inspect()))
	}
	out.WriteString("{")
	out.WriteString(strings.Join(pairs, ", "))
	out.WriteString("}")
	return out.String()
}

// TypeCode returns the fixed numeric type code for Map.
func (m *Map) TypeCode() int { return TypeCodeMap }

// TypeName returns the human-readable type name.
func (m *Map) TypeName() string { return "map" }

// Get returns the value for the given key.
// Returns nil and false if the key is not found.
func (m *Map) Get(key Object) (Object, bool) {
	hashable, ok := key.(Hashable)
	if !ok {
		return nil, false
	}
	hashKey := hashable.HashKey()
	pair, ok := m.Pairs[hashKey]
	if !ok {
		return nil, false
	}
	return pair.Value, true
}

// Set sets the value for the given key.
// Returns false if the key is not hashable.
func (m *Map) Set(key Object, value Object) bool {
	hashable, ok := key.(Hashable)
	if !ok {
		return false
	}
	hashKey := hashable.HashKey()
	if m.Pairs == nil {
		m.Pairs = make(map[HashKey]MapKeyPair)
	}
	m.Pairs[hashKey] = MapKeyPair{Key: key, Value: value}
	return true
}

// Delete removes the key from the map.
// Returns true if the key was found and removed.
func (m *Map) Delete(key Object) bool {
	hashable, ok := key.(Hashable)
	if !ok {
		return false
	}
	hashKey := hashable.HashKey()
	if _, exists := m.Pairs[hashKey]; exists {
		delete(m.Pairs, hashKey)
		return true
	}
	return false
}

// Len returns the number of pairs in the map.
func (m *Map) Len() int {
	return len(m.Pairs)
}
