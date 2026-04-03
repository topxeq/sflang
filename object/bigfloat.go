package object

import (
	"hash/fnv"
	"math/big"
	"strings"
)

// BigFloat represents an arbitrary-precision floating-point number in Sflang.
// It wraps a Go *big.Float value and implements the Object interface.
// BigFloat supports floating-point numbers of unlimited precision.
type BigFloat struct {
	Value *big.Float
}

// Type returns the ObjectType for BigFloat.
func (b *BigFloat) Type() ObjectType { return BIGFLOAT_OBJ }

// Inspect returns a string representation of the big float value.
func (b *BigFloat) Inspect() string {
	if b.Value == nil {
		return "0.0m"
	}
	return b.Value.Text('g', -1) + "m"
}

// TypeCode returns the fixed numeric type code for BigFloat.
func (b *BigFloat) TypeCode() int { return TypeCodeBigFloat }

// TypeName returns the human-readable type name.
func (b *BigFloat) TypeName() string { return "bigFloat" }

// HashKey returns a HashKey for using big floats as map keys.
// Uses FNV-1a hash algorithm on the string representation.
func (b *BigFloat) HashKey() HashKey {
	h := fnv.New64a()
	h.Write([]byte(b.Value.Text('g', -1)))
	return HashKey{Type: BIGFLOAT_OBJ, Value: h.Sum64()}
}

// GetBigFloat creates a BigFloat from a float64.
func GetBigFloat(value float64) *BigFloat {
	return &BigFloat{Value: big.NewFloat(value)}
}

// ParseBigFloat parses a string to create a BigFloat.
// Returns nil and sets error if parsing fails.
func ParseBigFloat(s string) (*BigFloat, *Error) {
	// Remove optional 'm' suffix
	s = strings.TrimSuffix(s, "m")

	f, _, err := big.ParseFloat(s, 10, 0, big.ToNearestEven)
	if err != nil {
		return nil, NewError("invalid big float: %s", s)
	}
	return &BigFloat{Value: f}, nil
}

// NewBigFloat creates a BigFloat from a *big.Float.
func NewBigFloat(value *big.Float) *BigFloat {
	return &BigFloat{Value: value}
}