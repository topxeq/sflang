package object

import (
	"fmt"
	"time"
)

// Time represents a timestamp in Sflang.
// It wraps a Go time.Time value and implements the Object interface.
type Time struct {
	Value time.Time
}

// Type returns the ObjectType for Time.
func (t *Time) Type() ObjectType { return TIME_OBJ }

// Inspect returns a string representation of the time value.
// Uses RFC3339 format for standard representation.
func (t *Time) Inspect() string {
	if t.Value.IsZero() {
		return "time(null)"
	}
	return fmt.Sprintf("time(%s)", t.Value.Format(time.RFC3339))
}

// TypeCode returns the fixed numeric type code for Time.
func (t *Time) TypeCode() int { return TypeCodeTime }

// TypeName returns the human-readable type name.
func (t *Time) TypeName() string { return "time" }

// Now returns the current time as a Time object.
func Now() *Time {
	return &Time{Value: time.Now()}
}

// Unix returns a Time object from Unix timestamp (seconds).
func Unix(sec int64) *Time {
	return &Time{Value: time.Unix(sec, 0)}
}

// UnixMilli returns a Time object from Unix timestamp (milliseconds).
func UnixMilli(msec int64) *Time {
	return &Time{Value: time.Unix(msec/1000, (msec%1000)*1000000)}
}

// UnixNano returns a Time object from Unix timestamp (nanoseconds).
func UnixNano(nsec int64) *Time {
	return &Time{Value: time.Unix(0, nsec)}
}

// Unix returns the Unix timestamp in seconds.
func (t *Time) Unix() int64 {
	return t.Value.Unix()
}

// UnixMilli returns the Unix timestamp in milliseconds.
func (t *Time) UnixMilli() int64 {
	return t.Value.UnixMilli()
}

// UnixNano returns the Unix timestamp in nanoseconds.
func (t *Time) UnixNano() int64 {
	return t.Value.UnixNano()
}

// Format returns the time formatted according to the layout.
func (t *Time) Format(layout string) string {
	return t.Value.Format(layout)
}