// Package vm implements the virtual machine for Sflang bytecode execution.
package vm

import "github.com/topxeq/sflang/object"

// Frame represents a function call frame on the call stack.
// Optimized for performance with direct field access (no methods).
type Frame struct {
	fn          *object.CompiledFunction // Direct function reference
	free        []object.Object          // Free variables (nil for non-closures)
	ip          int                      // Instruction pointer
	basePointer int                      // Stack base pointer for this frame
}

// FramePool holds pre-allocated Frame objects to avoid allocations.
type FramePool struct {
	frames []Frame
	index  int
}

// NewFramePool creates a new frame pool with pre-allocated frames.
func NewFramePool(size int) *FramePool {
	return &FramePool{
		frames: make([]Frame, size),
		index:  0,
	}
}

// Acquire gets a frame from the pool.
func (p *FramePool) Acquire(fn *object.CompiledFunction, free []object.Object, basePointer int) *Frame {
	if p.index >= len(p.frames) {
		// Pool exhausted, create new frame
		return &Frame{
			fn:          fn,
			free:        free,
			ip:          -1,
			basePointer: basePointer,
		}
	}
	f := &p.frames[p.index]
	p.index++
	f.fn = fn
	f.free = free
	f.ip = -1
	f.basePointer = basePointer
	return f
}

// Release returns frames to the pool.
func (p *FramePool) Release(count int) {
	p.index -= count
	if p.index < 0 {
		p.index = 0
	}
}