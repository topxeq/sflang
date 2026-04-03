package object

import (
	"hash/fnv"
	"math/big"
	"strings"
)

// BigInt represents an arbitrary-precision integer in Sflang.
// It wraps a Go *big.Int value and implements the Object interface.
// BigInt supports integers of unlimited size (only limited by memory).
type BigInt struct {
	Value *big.Int
}

// Type returns the ObjectType for BigInt.
func (b *BigInt) Type() ObjectType { return BIGINT_OBJ }

// Inspect returns a string representation of the big integer value.
func (b *BigInt) Inspect() string {
	if b.Value == nil {
		return "0n"
	}
	return b.Value.String() + "n"
}

// TypeCode returns the fixed numeric type code for BigInt.
func (b *BigInt) TypeCode() int { return TypeCodeBigInt }

// TypeName returns the human-readable type name.
func (b *BigInt) TypeName() string { return "bigInt" }

// HashKey returns a HashKey for using big integers as map keys.
// Uses FNV-1a hash algorithm on the string representation.
func (b *BigInt) HashKey() HashKey {
	h := fnv.New64a()
	h.Write([]byte(b.Value.String()))
	return HashKey{Type: BIGINT_OBJ, Value: h.Sum64()}
}

// GetBigInt creates a BigInt from an int64.
func GetBigInt(value int64) *BigInt {
	return &BigInt{Value: big.NewInt(value)}
}

// ParseBigInt parses a string to create a BigInt.
// Returns nil and sets error if parsing fails.
func ParseBigInt(s string) (*BigInt, *Error) {
	// Remove optional 'n' suffix
	s = strings.TrimSuffix(s, "n")

	n := new(big.Int)
	_, ok := n.SetString(s, 0) // auto-detect base (0x, 0o, 0b prefixes supported)
	if !ok {
		return nil, NewError("invalid big integer: %s", s)
	}
	return &BigInt{Value: n}, nil
}

// NewBigInt creates a BigInt from a *big.Int.
func NewBigInt(value *big.Int) *BigInt {
	return &BigInt{Value: value}
}