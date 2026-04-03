// Package vm implements the virtual machine for Sflang bytecode execution.
// The VM is a stack-based bytecode interpreter with performance optimizations.
package vm

import (
	"fmt"

	"github.com/topxeq/sflang/compiler"
	"github.com/topxeq/sflang/object"
)

const (
	StackSize      = 65536  // Increased stack size
	GlobalSize     = 65536
	MaxFrames      = 4096   // Increased frame limit
	MaxTryHandlers = 256
)

// currentVM holds a reference to the currently running VM.
// This allows builtin functions to call user-defined functions.
var currentVM *VM

// GetCurrentVM returns the currently running VM.
func GetCurrentVM() *VM {
	return currentVM
}

// VM represents the virtual machine state.
// Fields are ordered for optimal memory access patterns.
type VM struct {
	// Hot data - accessed frequently in main loop
	stack      []object.Object
	sp         int            // Stack pointer
	frames     []Frame        // Inline frame storage (not pointers)
	frameIndex int            // Current frame index

	// Constants and globals
	constants []object.Object
	globals   []object.Object

	// Exception handling
	tryHandlers []TryHandler
	tryIndex    int
}

// TryHandler represents a try-catch handler.
type TryHandler struct {
	catchIP     int    // Instruction pointer for catch block
	catchBase   int    // Stack base pointer to restore
	catchFrame  int    // Frame index where try block was defined
	catchVarIdx int    // Local variable index for catch variable
}

// New creates a new VM with the given bytecode.
func New(bytecode *compiler.Bytecode) *VM {
	mainFn := &object.CompiledFunction{
		Instructions: bytecode.Instructions,
		NumLocals:    bytecode.NumLocals,
	}

	globals := make([]object.Object, GlobalSize)

	vm := &VM{
		constants:   bytecode.Constants,
		stack:       make([]object.Object, StackSize),
		sp:          bytecode.NumLocals, // Reserve stack space for local variables
		globals:     globals,
		frames:      make([]Frame, MaxFrames),
		frameIndex:  1,
		tryHandlers: make([]TryHandler, MaxTryHandlers),
		tryIndex:    0,
	}

	// Initialize main frame directly
	vm.frames[0].fn = mainFn
	vm.frames[0].free = nil
	vm.frames[0].ip = -1
	vm.frames[0].basePointer = 0

	return vm
}

// NewWithGlobals creates a new VM with custom globals.
func NewWithGlobals(bytecode *compiler.Bytecode, globals []object.Object) *VM {
	vm := New(bytecode)
	vm.globals = globals
	return vm
}

// currentFrame returns pointer to current frame (inline for performance).
func (vm *VM) currentFrame() *Frame {
	return &vm.frames[vm.frameIndex-1]
}

// pushFrame adds a new frame (optimized, no allocation).
func (vm *VM) pushFrame(fn *object.CompiledFunction, free []object.Object, basePointer int) {
	f := &vm.frames[vm.frameIndex]
	f.fn = fn
	f.free = free
	f.ip = -1
	f.basePointer = basePointer
	vm.frameIndex++
}

// popFrame removes current frame and returns base pointer.
func (vm *VM) popFrame() int {
	vm.frameIndex--
	return vm.frames[vm.frameIndex].basePointer
}

// push adds an object to the stack.
func (vm *VM) push(obj object.Object) error {
	if vm.sp >= StackSize {
		return fmt.Errorf("stack overflow")
	}
	vm.stack[vm.sp] = obj
	vm.sp++
	return nil
}

// pop removes and returns top object from stack.
func (vm *VM) pop() object.Object {
	vm.sp--
	return vm.stack[vm.sp]
}

// Run executes the bytecode with aggressive optimizations.
func (vm *VM) Run() error {
	// Set current VM for builtin access
	currentVM = vm
	defer func() { currentVM = nil }()

	// Cache current frame pointer for faster access
	frame := vm.currentFrame()

	for {
		// Increment IP
		frame.ip++

		// Get instruction directly
		ip := frame.ip
		ins := frame.fn.Instructions

		if ip >= len(ins) {
			break
		}

		op := compiler.Opcode(ins[ip])

		switch op {
		case compiler.OpConstant:
			constIndex := int(ins[ip+1])<<8 | int(ins[ip+2])
			frame.ip += 2
			vm.stack[vm.sp] = vm.constants[constIndex]
			vm.sp++

		case compiler.OpNull:
			vm.stack[vm.sp] = object.NULL
			vm.sp++

		case compiler.OpTrue:
			vm.stack[vm.sp] = object.TRUE
			vm.sp++

		case compiler.OpFalse:
			vm.stack[vm.sp] = object.FALSE
			vm.sp++

		case compiler.OpPop:
			vm.sp--

		case compiler.OpDup:
			vm.stack[vm.sp] = vm.stack[vm.sp-1]
			vm.sp++

		case compiler.OpSwap:
			// Swap top two values: [a, b] -> [b, a]
			vm.stack[vm.sp-1], vm.stack[vm.sp-2] = vm.stack[vm.sp-2], vm.stack[vm.sp-1]

		case compiler.OpRot3:
			// Rotate top 3 values for index assignment
			// Before: [value, array, index] (positions: sp-3, sp-2, sp-1)
			// After: [array, index, value]
			value := vm.stack[vm.sp-3]
			array := vm.stack[vm.sp-2]
			index := vm.stack[vm.sp-1]
			vm.stack[vm.sp-3] = array
			vm.stack[vm.sp-2] = index
			vm.stack[vm.sp-1] = value

		case compiler.OpAdd:
			vm.sp--
			right := vm.stack[vm.sp]
			vm.sp--
			left := vm.stack[vm.sp]

			// Fast path for integers
			if l, ok := left.(*object.Integer); ok {
				if r, ok := right.(*object.Integer); ok {
					vm.stack[vm.sp] = object.GetInteger(l.Value + r.Value)
					vm.sp++
					continue
				}
			}
			// Slow path
			result, err := vm.addObjects(left, right)
			if err != nil {
				return err
			}
			vm.stack[vm.sp] = result
			vm.sp++

		case compiler.OpSub:
			vm.sp--
			right := vm.stack[vm.sp]
			vm.sp--
			left := vm.stack[vm.sp]

			if l, ok := left.(*object.Integer); ok {
				if r, ok := right.(*object.Integer); ok {
					vm.stack[vm.sp] = object.GetInteger(l.Value - r.Value)
					vm.sp++
					continue
				}
			}
			result, err := vm.subObjects(left, right)
			if err != nil {
				return err
			}
			vm.stack[vm.sp] = result
			vm.sp++

		case compiler.OpMul:
			vm.sp--
			right := vm.stack[vm.sp]
			vm.sp--
			left := vm.stack[vm.sp]

			if l, ok := left.(*object.Integer); ok {
				if r, ok := right.(*object.Integer); ok {
					vm.stack[vm.sp] = object.GetInteger(l.Value * r.Value)
					vm.sp++
					continue
				}
			}
			result, err := vm.mulObjects(left, right)
			if err != nil {
				return err
			}
			vm.stack[vm.sp] = result
			vm.sp++

		case compiler.OpDiv:
			vm.sp--
			right := vm.stack[vm.sp]
			vm.sp--
			left := vm.stack[vm.sp]

			if l, ok := left.(*object.Integer); ok {
				if r, ok := right.(*object.Integer); ok {
					if r.Value == 0 {
						return fmt.Errorf("division by zero")
					}
					vm.stack[vm.sp] = object.GetInteger(l.Value / r.Value)
					vm.sp++
					continue
				}
			}
			result, err := vm.divObjects(left, right)
			if err != nil {
				return err
			}
			vm.stack[vm.sp] = result
			vm.sp++

		case compiler.OpMod:
			vm.sp--
			right := vm.stack[vm.sp]
			vm.sp--
			left := vm.stack[vm.sp]

			if l, ok := left.(*object.Integer); ok {
				if r, ok := right.(*object.Integer); ok {
					vm.stack[vm.sp] = object.GetInteger(l.Value % r.Value)
					vm.sp++
					continue
				}
			}
			result, err := vm.modObjects(left, right)
			if err != nil {
				return err
			}
			vm.stack[vm.sp] = result
			vm.sp++

		case compiler.OpNeg:
			vm.sp--
			val := vm.stack[vm.sp]
			if i, ok := val.(*object.Integer); ok {
				vm.stack[vm.sp] = object.GetInteger(-i.Value)
				vm.sp++
				continue
			}
			result, err := vm.negObject(val)
			if err != nil {
				return err
			}
			vm.stack[vm.sp] = result
			vm.sp++

		case compiler.OpEqual:
			vm.sp--
			right := vm.stack[vm.sp]
			vm.sp--
			left := vm.stack[vm.sp]
			if vm.objectsEqual(left, right) {
				vm.stack[vm.sp] = object.TRUE
			} else {
				vm.stack[vm.sp] = object.FALSE
			}
			vm.sp++

		case compiler.OpNotEqual:
			vm.sp--
			right := vm.stack[vm.sp]
			vm.sp--
			left := vm.stack[vm.sp]
			if vm.objectsEqual(left, right) {
				vm.stack[vm.sp] = object.FALSE
			} else {
				vm.stack[vm.sp] = object.TRUE
			}
			vm.sp++

		case compiler.OpLess:
			vm.sp--
			right := vm.stack[vm.sp]
			vm.sp--
			left := vm.stack[vm.sp]
			cmp, err := vm.compareLess(left, right)
			if err != nil {
				return err
			}
			vm.stack[vm.sp] = cmp
			vm.sp++

		case compiler.OpLessEqual:
			vm.sp--
			right := vm.stack[vm.sp]
			vm.sp--
			left := vm.stack[vm.sp]
			cmp, err := vm.compareLessEqual(left, right)
			if err != nil {
				return err
			}
			vm.stack[vm.sp] = cmp
			vm.sp++

		case compiler.OpGreater:
			vm.sp--
			right := vm.stack[vm.sp]
			vm.sp--
			left := vm.stack[vm.sp]
			cmp, err := vm.compareGreater(left, right)
			if err != nil {
				return err
			}
			vm.stack[vm.sp] = cmp
			vm.sp++

		case compiler.OpGreaterEqual:
			vm.sp--
			right := vm.stack[vm.sp]
			vm.sp--
			left := vm.stack[vm.sp]
			cmp, err := vm.compareGreaterEqual(left, right)
			if err != nil {
				return err
			}
			vm.stack[vm.sp] = cmp
			vm.sp++

		case compiler.OpNot:
			vm.sp--
			val := vm.stack[vm.sp]
			if val == object.TRUE {
				vm.stack[vm.sp] = object.FALSE
			} else if val == object.FALSE {
				vm.stack[vm.sp] = object.TRUE
			} else if val == object.NULL {
				vm.stack[vm.sp] = object.TRUE
			} else {
				vm.stack[vm.sp] = object.FALSE
			}
			vm.sp++

		case compiler.OpAnd:
			vm.sp--
			right := vm.stack[vm.sp]
			vm.sp--
			left := vm.stack[vm.sp]
			if left == object.FALSE || left == object.NULL {
				vm.stack[vm.sp] = object.FALSE
			} else {
				vm.stack[vm.sp] = right
			}
			vm.sp++

		case compiler.OpOr:
			vm.sp--
			right := vm.stack[vm.sp]
			vm.sp--
			left := vm.stack[vm.sp]
			if left == object.TRUE {
				vm.stack[vm.sp] = object.TRUE
			} else {
				vm.stack[vm.sp] = right
			}
			vm.sp++

		case compiler.OpJump:
			pos := int(ins[ip+1])<<8 | int(ins[ip+2])
			frame.ip = pos - 1 // -1 because loop increments

		case compiler.OpJumpNotTrue:
			pos := int(ins[ip+1])<<8 | int(ins[ip+2])
			frame.ip += 2
			vm.sp--
			condition := vm.stack[vm.sp]
			if condition == object.FALSE || condition == object.NULL {
				frame.ip = pos - 1
			}

		case compiler.OpJumpTrue:
			pos := int(ins[ip+1])<<8 | int(ins[ip+2])
			frame.ip += 2
			vm.sp--
			condition := vm.stack[vm.sp]
			if condition != object.FALSE && condition != object.NULL {
				frame.ip = pos - 1
			}

		case compiler.OpSetGlobal:
			globalIndex := int(ins[ip+1])<<8 | int(ins[ip+2])
			frame.ip += 2
			vm.sp--
			vm.globals[globalIndex] = vm.stack[vm.sp]

		case compiler.OpGetGlobal:
			globalIndex := int(ins[ip+1])<<8 | int(ins[ip+2])
			frame.ip += 2
			vm.stack[vm.sp] = vm.globals[globalIndex]
			vm.sp++

		case compiler.OpSetLocal:
			localIndex := int(ins[ip+1])
			frame.ip++
			vm.sp--
			vm.stack[frame.basePointer+localIndex] = vm.stack[vm.sp]

		case compiler.OpGetLocal:
			localIndex := int(ins[ip+1])
			frame.ip++
			vm.stack[vm.sp] = vm.stack[frame.basePointer+localIndex]
			vm.sp++

		case compiler.OpGetBuiltin:
			builtinIndex := int(ins[ip+1])
			frame.ip++
			vm.stack[vm.sp] = object.Builtins[builtinIndex]
			vm.sp++

		case compiler.OpArray:
			numElements := int(ins[ip+1])<<8 | int(ins[ip+2])
			frame.ip += 2
			elements := make([]object.Object, numElements)
			for i := numElements - 1; i >= 0; i-- {
				vm.sp--
				elements[i] = vm.stack[vm.sp]
			}
			vm.stack[vm.sp] = &object.Array{Elements: elements}
			vm.sp++

		case compiler.OpMap:
			numPairs := int(ins[ip+1])<<8 | int(ins[ip+2])
			frame.ip += 2
			pairs := make(map[object.HashKey]object.MapKeyPair)
			for i := 0; i < numPairs; i += 2 {
				vm.sp--
				value := vm.stack[vm.sp]
				vm.sp--
				key := vm.stack[vm.sp]
				hashKey, ok := key.(object.Hashable)
				if !ok {
					return fmt.Errorf("unusable as hash key: %s", key.Type())
				}
				pairs[hashKey.HashKey()] = object.MapKeyPair{Key: key, Value: value}
			}
			vm.stack[vm.sp] = &object.Map{Pairs: pairs}
			vm.sp++

		case compiler.OpIndex:
			vm.sp--
			index := vm.stack[vm.sp]
			vm.sp--
			left := vm.stack[vm.sp]

			var result object.Object
			switch l := left.(type) {
			case *object.Array:
				idx, ok := index.(*object.Integer)
				if !ok {
					return fmt.Errorf("non-integer array index: %s", index.Type())
				}
				idxVal := int(idx.Value)
				// Support negative indices: -1 means last element, -2 means second last, etc.
				if idxVal < 0 {
					idxVal = len(l.Elements) + idxVal
				}
				if idxVal < 0 || idxVal >= len(l.Elements) {
					result = object.NULL
				} else {
					result = l.Elements[idxVal]
				}
			case *object.Map:
				key, ok := index.(object.Hashable)
				if !ok {
					return fmt.Errorf("unusable as map key: %s", index.Type())
				}
				pair, ok := l.Pairs[key.HashKey()]
				if !ok {
					result = object.NULL
				} else {
					result = pair.Value
				}
			case *object.String:
				idx, ok := index.(*object.Integer)
				if !ok {
					return fmt.Errorf("non-integer string index: %s", index.Type())
				}
				idxVal := int(idx.Value)
				// Support negative indices for strings as well
				if idxVal < 0 {
					idxVal = len(l.Value) + idxVal
				}
				if idxVal < 0 || idxVal >= len(l.Value) {
					result = object.NULL
				} else {
					result = &object.String{Value: string(l.Value[idxVal])}
				}
			default:
				return fmt.Errorf("index operator not supported: %s", left.Type())
			}
			vm.stack[vm.sp] = result
			vm.sp++

		case compiler.OpSetIndex:
			vm.sp--
			value := vm.stack[vm.sp]
			vm.sp--
			index := vm.stack[vm.sp]
			vm.sp--
			left := vm.stack[vm.sp]

			switch l := left.(type) {
			case *object.Array:
				idx, ok := index.(*object.Integer)
				if !ok {
					return fmt.Errorf("non-integer array index: %s", index.Type())
				}
				idxVal := int(idx.Value)
				// Support negative indices for array assignment
				if idxVal < 0 {
					idxVal = len(l.Elements) + idxVal
				}
				if idxVal < 0 || idxVal >= len(l.Elements) {
					return fmt.Errorf("array index out of bounds: %d", idx.Value)
				}
				l.Elements[idxVal] = value
			case *object.Map:
				key, ok := index.(object.Hashable)
				if !ok {
					return fmt.Errorf("unusable as map key: %s", index.Type())
				}
				l.Pairs[key.HashKey()] = object.MapKeyPair{Key: index, Value: value}
			default:
				return fmt.Errorf("index assignment not supported: %s", left.Type())
			}

		case compiler.OpSlice:
			// Stack: [array, start, end]
			vm.sp--
			endIdx := vm.stack[vm.sp]
			vm.sp--
			startIdx := vm.stack[vm.sp]
			vm.sp--
			left := vm.stack[vm.sp]

			arr, ok := left.(*object.Array)
			if !ok {
				return fmt.Errorf("slice operator not supported: %s", left.Type())
			}

			start, ok1 := startIdx.(*object.Integer)
			if !ok1 {
				return fmt.Errorf("non-integer slice start: %s", startIdx.Type())
			}

			var end int
			if endI, ok2 := endIdx.(*object.Integer); ok2 {
				if endI.Value == -1 {
					// -1 means "end of array"
					end = len(arr.Elements)
				} else {
					end = int(endI.Value)
				}
			} else {
				return fmt.Errorf("non-integer slice end: %s", endIdx.Type())
			}

			startVal := int(start.Value)
			if startVal < 0 {
				startVal = 0
			}
			if end < startVal {
				end = startVal
			}
			if end > len(arr.Elements) {
				end = len(arr.Elements)
			}

			result := &object.Array{Elements: arr.Elements[startVal:end]}
			vm.stack[vm.sp] = result
			vm.sp++

		case compiler.OpCall:
			numArgs := int(ins[ip+1])
			frame.ip++

			// Get callee from stack
			callee := vm.stack[vm.sp-1-numArgs]

			switch fn := callee.(type) {
			case *object.Closure:
				if fn.Fn.IsVariadic {
					// For variadic functions, allow any number of args >= NumParameters
					minArgs := fn.Fn.NumParameters
					if numArgs < minArgs {
						return fmt.Errorf("wrong number of arguments: want>=%d, got=%d", minArgs, numArgs)
					}

					// Collect variadic args into an array
					// Stack layout: [closure][regular_args...][variadic_args...]
					//             sp-numArgs-1      sp-numArgs    sp-numVarArgs
					numVarArgs := numArgs - minArgs
					varArgs := make([]object.Object, numVarArgs)
					for i := 0; i < numVarArgs; i++ {
						// Variadic args are at positions [sp-numVarArgs, sp-1)
						varArgs[i] = vm.stack[vm.sp-numVarArgs+i]
					}
					argsArray := &object.Array{Elements: varArgs}

					// Reorganize stack: remove variadic args, add argsArray
					// New layout: [closure][regular_args...][argsArray]
					// The argsArray should be at position sp-1 after reorganization

					// Move argsArray to replace variadic args
					// Position of first variadic arg: sp - numVarArgs
					vm.stack[vm.sp-numVarArgs] = argsArray

					// New stack has: closure + regular args + argsArray

					// BasePointer points to first argument (or argsArray if no regular args)
					basePointer := vm.sp - numVarArgs - minArgs
					vm.pushFrame(fn.Fn, fn.Free, basePointer)
					vm.sp = basePointer + fn.Fn.NumLocals

					// Update frame reference
					frame = vm.currentFrame()
				} else {
					if numArgs != fn.Fn.NumParameters {
						return fmt.Errorf("wrong number of arguments: want=%d, got=%d", fn.Fn.NumParameters, numArgs)
					}

					// Push new frame (no allocation)
					basePointer := vm.sp - numArgs
					vm.pushFrame(fn.Fn, fn.Free, basePointer)
					vm.sp = basePointer + fn.Fn.NumLocals

					// Update frame reference
					frame = vm.currentFrame()
				}

			case *object.Builtin:
				// Build args slice
				args := make([]object.Object, numArgs)
				for i := 0; i < numArgs; i++ {
					args[i] = vm.stack[vm.sp-numArgs+i]
				}

				result := fn.Fn(args...)
				vm.sp = vm.sp - numArgs - 1

				if result != nil {
					vm.stack[vm.sp] = result
					vm.sp++
				} else {
					vm.stack[vm.sp] = object.NULL
					vm.sp++
				}

			default:
				return fmt.Errorf("not a function: %T", callee)
			}

		case compiler.OpReturnValue:
			vm.sp--
			returnValue := vm.stack[vm.sp]
			bp := vm.popFrame()
			vm.sp = bp - 1
			vm.stack[vm.sp] = returnValue
			vm.sp++

			// Update frame reference
			frame = vm.currentFrame()

		case compiler.OpReturn:
			bp := vm.popFrame()
			vm.sp = bp - 1
			vm.stack[vm.sp] = object.NULL
			vm.sp++

			// Update frame reference
			frame = vm.currentFrame()

		case compiler.OpClosure:
			constIndex := int(ins[ip+1])<<8 | int(ins[ip+2])
			numFree := int(ins[ip+3])
			frame.ip += 3

			fn, ok := vm.constants[constIndex].(*object.CompiledFunction)
			if !ok {
				return fmt.Errorf("not a function: %T", vm.constants[constIndex])
			}

			free := make([]object.Object, numFree)
			for i := 0; i < numFree; i++ {
				free[i] = vm.stack[vm.sp-numFree+i]
			}
			vm.sp -= numFree

			vm.stack[vm.sp] = &object.Closure{Fn: fn, Free: free}
			vm.sp++

		case compiler.OpGetFree:
			freeIndex := int(ins[ip+1])
			frame.ip++
			vm.stack[vm.sp] = frame.free[freeIndex]
			vm.sp++

		case compiler.OpGetUp:
			freeIndex := int(ins[ip+1])
			frame.ip++
			vm.stack[vm.sp] = frame.free[freeIndex]
			vm.sp++

		case compiler.OpSetUp:
			freeIndex := int(ins[ip+1])
			frame.ip++
			vm.sp--
			frame.free[freeIndex] = vm.stack[vm.sp]

		case compiler.OpBitAnd:
			vm.sp--
			right := vm.stack[vm.sp]
			vm.sp--
			left := vm.stack[vm.sp]
			l, ok1 := left.(*object.Integer)
			r, ok2 := right.(*object.Integer)
			if !ok1 || !ok2 {
				return fmt.Errorf("bitwise operators require integers")
			}
			vm.stack[vm.sp] = object.GetInteger(l.Value & r.Value)
			vm.sp++

		case compiler.OpBitOr:
			vm.sp--
			right := vm.stack[vm.sp]
			vm.sp--
			left := vm.stack[vm.sp]
			l, ok1 := left.(*object.Integer)
			r, ok2 := right.(*object.Integer)
			if !ok1 || !ok2 {
				return fmt.Errorf("bitwise operators require integers")
			}
			vm.stack[vm.sp] = object.GetInteger(l.Value | r.Value)
			vm.sp++

		case compiler.OpBitXor:
			vm.sp--
			right := vm.stack[vm.sp]
			vm.sp--
			left := vm.stack[vm.sp]
			l, ok1 := left.(*object.Integer)
			r, ok2 := right.(*object.Integer)
			if !ok1 || !ok2 {
				return fmt.Errorf("bitwise operators require integers")
			}
			vm.stack[vm.sp] = object.GetInteger(l.Value ^ r.Value)
			vm.sp++

		case compiler.OpShl:
			vm.sp--
			right := vm.stack[vm.sp]
			vm.sp--
			left := vm.stack[vm.sp]
			l, ok1 := left.(*object.Integer)
			r, ok2 := right.(*object.Integer)
			if !ok1 || !ok2 {
				return fmt.Errorf("bitwise operators require integers")
			}
			vm.stack[vm.sp] = object.GetInteger(l.Value << uint(r.Value))
			vm.sp++

		case compiler.OpShr:
			vm.sp--
			right := vm.stack[vm.sp]
			vm.sp--
			left := vm.stack[vm.sp]
			l, ok1 := left.(*object.Integer)
			r, ok2 := right.(*object.Integer)
			if !ok1 || !ok2 {
				return fmt.Errorf("bitwise operators require integers")
			}
			vm.stack[vm.sp] = object.GetInteger(l.Value >> uint(r.Value))
			vm.sp++

		case compiler.OpBitNot:
			vm.sp--
			val := vm.stack[vm.sp]
			i, ok := val.(*object.Integer)
			if !ok {
				return fmt.Errorf("bitwise not requires integer")
			}
			vm.stack[vm.sp] = object.GetInteger(^i.Value)
			vm.sp++

		case compiler.OpPushTry:
			catchPos := int(ins[ip+1])<<8 | int(ins[ip+2])
			frame.ip += 2
			vm.tryHandlers[vm.tryIndex] = TryHandler{
				catchIP:    catchPos,
				catchBase:  frame.basePointer,
				catchFrame: vm.frameIndex,
			}
			vm.tryIndex++

		case compiler.OpPopTry:
			vm.tryIndex--

		case compiler.OpCatchStart:
			// Adjust basePointer for catch block execution
			// The error object is at vm.sp - 1, set basePointer so OpGetLocal(0) reads it
			frame.basePointer = vm.sp - 1

		case compiler.OpThrow:
			vm.sp--
			errObj := vm.stack[vm.sp]

			// Find catch handler - unwind try handlers and frames
			for vm.tryIndex > 0 {
				vm.tryIndex--
				handler := vm.tryHandlers[vm.tryIndex]

				// Unwind frames if needed - restore to the frame where try was defined
				for vm.frameIndex > handler.catchFrame {
					vm.frameIndex--
					frame = &vm.frames[vm.frameIndex-1]
				}

				// Update frame reference after potential unwinding
				frame = vm.currentFrame()

				// Restore state for catch block execution
				// Set basePointer to catchBase so catch variable (index 0) reads from correct position
				frame.basePointer = handler.catchBase
				frame.ip = handler.catchIP - 1
				vm.sp = handler.catchBase

				// Push error object as catch variable at position catchBase
				vm.stack[vm.sp] = errObj
				vm.sp++
				goto continue_execution
			}

			return fmt.Errorf("unhandled error: %s", errObj.Inspect())

		default:
			return fmt.Errorf("unknown opcode: %d", op)
		}

	continue_execution:
	}

	return nil
}

// Helper methods for non-inline operations

func (vm *VM) addObjects(left, right object.Object) (object.Object, error) {
	switch l := left.(type) {
	case *object.Integer:
		if r, ok := right.(*object.Integer); ok {
			return object.GetInteger(l.Value + r.Value), nil
		}
		if r, ok := right.(*object.Float); ok {
			return &object.Float{Value: float64(l.Value) + r.Value}, nil
		}
	case *object.Float:
		if r, ok := right.(*object.Integer); ok {
			return &object.Float{Value: l.Value + float64(r.Value)}, nil
		}
		if r, ok := right.(*object.Float); ok {
			return &object.Float{Value: l.Value + r.Value}, nil
		}
	case *object.String:
		if r, ok := right.(*object.String); ok {
			return &object.String{Value: l.Value + r.Value}, nil
		}
	}
	return nil, fmt.Errorf("type mismatch: %s + %s", left.Type(), right.Type())
}

func (vm *VM) subObjects(left, right object.Object) (object.Object, error) {
	switch l := left.(type) {
	case *object.Integer:
		if r, ok := right.(*object.Integer); ok {
			return object.GetInteger(l.Value - r.Value), nil
		}
		if r, ok := right.(*object.Float); ok {
			return &object.Float{Value: float64(l.Value) - r.Value}, nil
		}
	case *object.Float:
		if r, ok := right.(*object.Integer); ok {
			return &object.Float{Value: l.Value - float64(r.Value)}, nil
		}
		if r, ok := right.(*object.Float); ok {
			return &object.Float{Value: l.Value - r.Value}, nil
		}
	}
	return nil, fmt.Errorf("type mismatch: %s - %s", left.Type(), right.Type())
}

func (vm *VM) mulObjects(left, right object.Object) (object.Object, error) {
	switch l := left.(type) {
	case *object.Integer:
		if r, ok := right.(*object.Integer); ok {
			return object.GetInteger(l.Value * r.Value), nil
		}
		if r, ok := right.(*object.Float); ok {
			return &object.Float{Value: float64(l.Value) * r.Value}, nil
		}
	case *object.Float:
		if r, ok := right.(*object.Integer); ok {
			return &object.Float{Value: l.Value * float64(r.Value)}, nil
		}
		if r, ok := right.(*object.Float); ok {
			return &object.Float{Value: l.Value * r.Value}, nil
		}
	case *object.String:
		if r, ok := right.(*object.Integer); ok {
			n := int(r.Value)
			if n <= 0 {
				return &object.String{Value: ""}, nil
			}
			result := make([]byte, 0, len(l.Value)*n)
			for i := 0; i < n; i++ {
				result = append(result, l.Value...)
			}
			return &object.String{Value: string(result)}, nil
		}
	}
	return nil, fmt.Errorf("type mismatch: %s * %s", left.Type(), right.Type())
}

func (vm *VM) divObjects(left, right object.Object) (object.Object, error) {
	switch l := left.(type) {
	case *object.Integer:
		if r, ok := right.(*object.Integer); ok {
			if r.Value == 0 {
				return nil, fmt.Errorf("division by zero")
			}
			return object.GetInteger(l.Value / r.Value), nil
		}
		if r, ok := right.(*object.Float); ok {
			if r.Value == 0 {
				return nil, fmt.Errorf("division by zero")
			}
			return &object.Float{Value: float64(l.Value) / r.Value}, nil
		}
	case *object.Float:
		if r, ok := right.(*object.Integer); ok {
			if r.Value == 0 {
				return nil, fmt.Errorf("division by zero")
			}
			return &object.Float{Value: l.Value / float64(r.Value)}, nil
		}
		if r, ok := right.(*object.Float); ok {
			if r.Value == 0 {
				return nil, fmt.Errorf("division by zero")
			}
			return &object.Float{Value: l.Value / r.Value}, nil
		}
	}
	return nil, fmt.Errorf("type mismatch: %s / %s", left.Type(), right.Type())
}

func (vm *VM) modObjects(left, right object.Object) (object.Object, error) {
	switch l := left.(type) {
	case *object.Integer:
		if r, ok := right.(*object.Integer); ok {
			return object.GetInteger(l.Value % r.Value), nil
		}
		if r, ok := right.(*object.Float); ok {
			return &object.Float{Value: float64(int64(l.Value) % int64(r.Value))}, nil
		}
	case *object.Float:
		if r, ok := right.(*object.Integer); ok {
			return &object.Float{Value: float64(int64(l.Value) % r.Value)}, nil
		}
		if r, ok := right.(*object.Float); ok {
			return &object.Float{Value: float64(int64(l.Value) % int64(r.Value))}, nil
		}
	}
	return nil, fmt.Errorf("type mismatch: %s %% %s", left.Type(), right.Type())
}

func (vm *VM) negObject(val object.Object) (object.Object, error) {
	switch v := val.(type) {
	case *object.Integer:
		return object.GetInteger(-v.Value), nil
	case *object.Float:
		return &object.Float{Value: -v.Value}, nil
	}
	return nil, fmt.Errorf("negation not supported: %s", val.Type())
}

func (vm *VM) objectsEqual(left, right object.Object) bool {
	if left == right {
		return true
	}

	switch l := left.(type) {
	case *object.Integer:
		if r, ok := right.(*object.Integer); ok {
			return l.Value == r.Value
		}
	case *object.Float:
		if r, ok := right.(*object.Float); ok {
			return l.Value == r.Value
		}
	case *object.String:
		if r, ok := right.(*object.String); ok {
			return l.Value == r.Value
		}
	case *object.Boolean:
		if r, ok := right.(*object.Boolean); ok {
			return l.Value == r.Value
		}
	}
	return false
}

func (vm *VM) compareLess(left, right object.Object) (*object.Boolean, error) {
	switch l := left.(type) {
	case *object.Integer:
		switch r := right.(type) {
		case *object.Integer:
			return nativeBoolToBooleanObject(l.Value < r.Value), nil
		case *object.Float:
			return nativeBoolToBooleanObject(float64(l.Value) < r.Value), nil
		}
	case *object.Float:
		switch r := right.(type) {
		case *object.Integer:
			return nativeBoolToBooleanObject(l.Value < float64(r.Value)), nil
		case *object.Float:
			return nativeBoolToBooleanObject(l.Value < r.Value), nil
		}
	case *object.String:
		if r, ok := right.(*object.String); ok {
			return nativeBoolToBooleanObject(l.Value < r.Value), nil
		}
	}
	return nil, fmt.Errorf("type mismatch: %s < %s", left.Type(), right.Type())
}

func (vm *VM) compareLessEqual(left, right object.Object) (*object.Boolean, error) {
	switch l := left.(type) {
	case *object.Integer:
		switch r := right.(type) {
		case *object.Integer:
			return nativeBoolToBooleanObject(l.Value <= r.Value), nil
		case *object.Float:
			return nativeBoolToBooleanObject(float64(l.Value) <= r.Value), nil
		}
	case *object.Float:
		switch r := right.(type) {
		case *object.Integer:
			return nativeBoolToBooleanObject(l.Value <= float64(r.Value)), nil
		case *object.Float:
			return nativeBoolToBooleanObject(l.Value <= r.Value), nil
		}
	case *object.String:
		if r, ok := right.(*object.String); ok {
			return nativeBoolToBooleanObject(l.Value <= r.Value), nil
		}
	}
	return nil, fmt.Errorf("type mismatch: %s <= %s", left.Type(), right.Type())
}

func (vm *VM) compareGreater(left, right object.Object) (*object.Boolean, error) {
	switch l := left.(type) {
	case *object.Integer:
		switch r := right.(type) {
		case *object.Integer:
			return nativeBoolToBooleanObject(l.Value > r.Value), nil
		case *object.Float:
			return nativeBoolToBooleanObject(float64(l.Value) > r.Value), nil
		}
	case *object.Float:
		switch r := right.(type) {
		case *object.Integer:
			return nativeBoolToBooleanObject(l.Value > float64(r.Value)), nil
		case *object.Float:
			return nativeBoolToBooleanObject(l.Value > r.Value), nil
		}
	case *object.String:
		if r, ok := right.(*object.String); ok {
			return nativeBoolToBooleanObject(l.Value > r.Value), nil
		}
	}
	return nil, fmt.Errorf("type mismatch: %s > %s", left.Type(), right.Type())
}

func (vm *VM) compareGreaterEqual(left, right object.Object) (*object.Boolean, error) {
	switch l := left.(type) {
	case *object.Integer:
		switch r := right.(type) {
		case *object.Integer:
			return nativeBoolToBooleanObject(l.Value >= r.Value), nil
		case *object.Float:
			return nativeBoolToBooleanObject(float64(l.Value) >= r.Value), nil
		}
	case *object.Float:
		switch r := right.(type) {
		case *object.Integer:
			return nativeBoolToBooleanObject(l.Value >= float64(r.Value)), nil
		case *object.Float:
			return nativeBoolToBooleanObject(l.Value >= r.Value), nil
		}
	case *object.String:
		if r, ok := right.(*object.String); ok {
			return nativeBoolToBooleanObject(l.Value >= r.Value), nil
		}
	}
	return nil, fmt.Errorf("type mismatch: %s >= %s", left.Type(), right.Type())
}

func nativeBoolToBooleanObject(b bool) *object.Boolean {
	if b {
		return object.TRUE
	}
	return object.FALSE
}

// LastPopped returns the last popped value from the stack.
func (vm *VM) LastPopped() object.Object {
	return vm.stack[vm.sp]
}

// StackTop returns the top of the stack.
func (vm *VM) StackTop() object.Object {
	if vm.sp == 0 {
		return nil
	}
	return vm.stack[vm.sp-1]
}

// CallClosure calls a closure with the given arguments and returns the result.
// This is used by builtins that need to call user-defined functions.
func (vm *VM) CallClosure(closure *object.Closure, args ...object.Object) (object.Object, error) {
	fn := closure.Fn

	// Check argument count
	if !fn.IsVariadic && len(args) != fn.NumParameters {
		return nil, fmt.Errorf("wrong number of arguments: want=%d, got=%d", fn.NumParameters, len(args))
	}
	if fn.IsVariadic && len(args) < fn.NumParameters {
		return nil, fmt.Errorf("wrong number of arguments: want>=%d, got=%d", fn.NumParameters, len(args))
	}

	// Save current state
	savedFrameIndex := vm.frameIndex

	// Push arguments onto stack
	for _, arg := range args {
		vm.stack[vm.sp] = arg
		vm.sp++
	}

	// Push new frame
	basePointer := vm.sp - len(args)
	vm.pushFrame(fn, closure.Free, basePointer)
	vm.sp = basePointer + fn.NumLocals

	// Run only this closure until it returns
	for vm.frameIndex > savedFrameIndex {
		frame := vm.currentFrame()
		frame.ip++

		ip := frame.ip
		ins := frame.fn.Instructions

		if ip >= len(ins) {
			break
		}

		op := compiler.Opcode(ins[ip])

		switch op {
		case compiler.OpReturnValue:
			vm.sp--
			result := vm.stack[vm.sp]
			vm.popFrame()
			return result, nil

		case compiler.OpReturn:
			vm.popFrame()
			return object.NULL, nil

		default:
			// Handle other opcodes inline
			if err := vm.executeInstruction(op, ins, ip); err != nil {
				vm.frameIndex = savedFrameIndex
				return nil, err
			}
		}
	}

	return object.NULL, nil
}

// executeInstruction executes a single instruction for CallClosure.
func (vm *VM) executeInstruction(op compiler.Opcode, ins []byte, ip int) error {
	frame := vm.currentFrame()

	switch op {
	case compiler.OpConstant:
		constIndex := int(ins[ip+1])<<8 | int(ins[ip+2])
		frame.ip += 2
		vm.stack[vm.sp] = vm.constants[constIndex]
		vm.sp++

	case compiler.OpNull:
		vm.stack[vm.sp] = object.NULL
		vm.sp++

	case compiler.OpTrue:
		vm.stack[vm.sp] = object.TRUE
		vm.sp++

	case compiler.OpFalse:
		vm.stack[vm.sp] = object.FALSE
		vm.sp++

	case compiler.OpPop:
		vm.sp--

	case compiler.OpDup:
		vm.stack[vm.sp] = vm.stack[vm.sp-1]
		vm.sp++

	case compiler.OpAdd:
		vm.sp--
		right := vm.stack[vm.sp]
		vm.sp--
		left := vm.stack[vm.sp]
		result, err := vm.addObjects(left, right)
		if err != nil {
			return err
		}
		vm.stack[vm.sp] = result
		vm.sp++

	case compiler.OpSub:
		vm.sp--
		right := vm.stack[vm.sp]
		vm.sp--
		left := vm.stack[vm.sp]
		result, err := vm.subObjects(left, right)
		if err != nil {
			return err
		}
		vm.stack[vm.sp] = result
		vm.sp++

	case compiler.OpMul:
		vm.sp--
		right := vm.stack[vm.sp]
		vm.sp--
		left := vm.stack[vm.sp]
		result, err := vm.mulObjects(left, right)
		if err != nil {
			return err
		}
		vm.stack[vm.sp] = result
		vm.sp++

	case compiler.OpDiv:
		vm.sp--
		right := vm.stack[vm.sp]
		vm.sp--
		left := vm.stack[vm.sp]
		result, err := vm.divObjects(left, right)
		if err != nil {
			return err
		}
		vm.stack[vm.sp] = result
		vm.sp++

	case compiler.OpMod:
		vm.sp--
		right := vm.stack[vm.sp]
		vm.sp--
		left := vm.stack[vm.sp]
		result, err := vm.modObjects(left, right)
		if err != nil {
			return err
		}
		vm.stack[vm.sp] = result
		vm.sp++

	case compiler.OpNeg:
		vm.sp--
		val := vm.stack[vm.sp]
		result, err := vm.negObject(val)
		if err != nil {
			return err
		}
		vm.stack[vm.sp] = result
		vm.sp++

	case compiler.OpEqual:
		vm.sp--
		right := vm.stack[vm.sp]
		vm.sp--
		left := vm.stack[vm.sp]
		if vm.objectsEqual(left, right) {
			vm.stack[vm.sp] = object.TRUE
		} else {
			vm.stack[vm.sp] = object.FALSE
		}
		vm.sp++

	case compiler.OpNotEqual:
		vm.sp--
		right := vm.stack[vm.sp]
		vm.sp--
		left := vm.stack[vm.sp]
		if vm.objectsEqual(left, right) {
			vm.stack[vm.sp] = object.FALSE
		} else {
			vm.stack[vm.sp] = object.TRUE
		}
		vm.sp++

	case compiler.OpLess:
		vm.sp--
		right := vm.stack[vm.sp]
		vm.sp--
		left := vm.stack[vm.sp]
		cmp, err := vm.compareLess(left, right)
		if err != nil {
			return err
		}
		vm.stack[vm.sp] = cmp
		vm.sp++

	case compiler.OpLessEqual:
		vm.sp--
		right := vm.stack[vm.sp]
		vm.sp--
		left := vm.stack[vm.sp]
		cmp, err := vm.compareLessEqual(left, right)
		if err != nil {
			return err
		}
		vm.stack[vm.sp] = cmp
		vm.sp++

	case compiler.OpGreater:
		vm.sp--
		right := vm.stack[vm.sp]
		vm.sp--
		left := vm.stack[vm.sp]
		cmp, err := vm.compareGreater(left, right)
		if err != nil {
			return err
		}
		vm.stack[vm.sp] = cmp
		vm.sp++

	case compiler.OpGreaterEqual:
		vm.sp--
		right := vm.stack[vm.sp]
		vm.sp--
		left := vm.stack[vm.sp]
		cmp, err := vm.compareGreaterEqual(left, right)
		if err != nil {
			return err
		}
		vm.stack[vm.sp] = cmp
		vm.sp++

	case compiler.OpNot:
		vm.sp--
		val := vm.stack[vm.sp]
		if val == object.TRUE {
			vm.stack[vm.sp] = object.FALSE
		} else if val == object.FALSE {
			vm.stack[vm.sp] = object.TRUE
		} else if val == object.NULL {
			vm.stack[vm.sp] = object.TRUE
		} else {
			vm.stack[vm.sp] = object.FALSE
		}
		vm.sp++

	case compiler.OpAnd:
		vm.sp--
		right := vm.stack[vm.sp]
		vm.sp--
		left := vm.stack[vm.sp]
		if left == object.FALSE || left == object.NULL {
			vm.stack[vm.sp] = object.FALSE
		} else {
			vm.stack[vm.sp] = right
		}
		vm.sp++

	case compiler.OpOr:
		vm.sp--
		right := vm.stack[vm.sp]
		vm.sp--
		left := vm.stack[vm.sp]
		if left == object.TRUE {
			vm.stack[vm.sp] = object.TRUE
		} else {
			vm.stack[vm.sp] = right
		}
		vm.sp++

	case compiler.OpJump:
		pos := int(ins[ip+1])<<8 | int(ins[ip+2])
		frame.ip = pos - 1

	case compiler.OpJumpNotTrue:
		pos := int(ins[ip+1])<<8 | int(ins[ip+2])
		frame.ip += 2
		vm.sp--
		condition := vm.stack[vm.sp]
		if condition == object.FALSE || condition == object.NULL {
			frame.ip = pos - 1
		}

	case compiler.OpJumpTrue:
		pos := int(ins[ip+1])<<8 | int(ins[ip+2])
		frame.ip += 2
		vm.sp--
		condition := vm.stack[vm.sp]
		if condition != object.FALSE && condition != object.NULL {
			frame.ip = pos - 1
		}

	case compiler.OpGetLocal:
		localIndex := int(ins[ip+1])
		frame.ip++
		vm.stack[vm.sp] = vm.stack[frame.basePointer+localIndex]
		vm.sp++

	case compiler.OpSetLocal:
		localIndex := int(ins[ip+1])
		frame.ip++
		vm.sp--
		vm.stack[frame.basePointer+localIndex] = vm.stack[vm.sp]

	case compiler.OpGetFree:
		freeIndex := int(ins[ip+1])
		frame.ip++
		vm.stack[vm.sp] = frame.free[freeIndex]
		vm.sp++

	case compiler.OpGetBuiltin:
		builtinIndex := int(ins[ip+1])
		frame.ip++
		vm.stack[vm.sp] = object.Builtins[builtinIndex]
		vm.sp++

	case compiler.OpIndex:
		vm.sp--
		index := vm.stack[vm.sp]
		vm.sp--
		left := vm.stack[vm.sp]
		var result object.Object
		switch l := left.(type) {
		case *object.Array:
			idx, ok := index.(*object.Integer)
			if !ok {
				return fmt.Errorf("non-integer array index: %s", index.Type())
			}
			idxVal := int(idx.Value)
			if idxVal < 0 {
				idxVal = len(l.Elements) + idxVal
			}
			if idxVal < 0 || idxVal >= len(l.Elements) {
				result = object.NULL
			} else {
				result = l.Elements[idxVal]
			}
		case *object.Map:
			key, ok := index.(object.Hashable)
			if !ok {
				return fmt.Errorf("unusable as map key: %s", index.Type())
			}
			pair, ok := l.Pairs[key.HashKey()]
			if !ok {
				result = object.NULL
			} else {
				result = pair.Value
			}
		case *object.String:
			idx, ok := index.(*object.Integer)
			if !ok {
				return fmt.Errorf("non-integer string index: %s", index.Type())
			}
			idxVal := int(idx.Value)
			if idxVal < 0 {
				idxVal = len(l.Value) + idxVal
			}
			if idxVal < 0 || idxVal >= len(l.Value) {
				result = object.NULL
			} else {
				result = &object.String{Value: string(l.Value[idxVal])}
			}
		default:
			return fmt.Errorf("index operator not supported: %s", left.Type())
		}
		vm.stack[vm.sp] = result
		vm.sp++

	case compiler.OpCall:
		numArgs := int(ins[ip+1])
		frame.ip++
		callee := vm.stack[vm.sp-1-numArgs]

		switch fn := callee.(type) {
		case *object.Closure:
			if !fn.Fn.IsVariadic && numArgs != fn.Fn.NumParameters {
				return fmt.Errorf("wrong number of arguments: want=%d, got=%d", fn.Fn.NumParameters, numArgs)
			}
			newBasePointer := vm.sp - numArgs
			vm.pushFrame(fn.Fn, fn.Free, newBasePointer)
			vm.sp = newBasePointer + fn.Fn.NumLocals

		case *object.Builtin:
			args := make([]object.Object, numArgs)
			for i := 0; i < numArgs; i++ {
				args[i] = vm.stack[vm.sp-numArgs+i]
			}
			result := fn.Fn(args...)
			vm.sp = vm.sp - numArgs - 1
			if result != nil {
				vm.stack[vm.sp] = result
				vm.sp++
			} else {
				vm.stack[vm.sp] = object.NULL
				vm.sp++
			}

		default:
			return fmt.Errorf("not a function: %T", callee)
		}

	default:
		return fmt.Errorf("unsupported opcode in executeInstruction: %d", op)
	}

	return nil
}