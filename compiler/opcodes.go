// Package compiler defines bytecode opcodes for the Sflang VM.
package compiler

// Opcode represents a single bytecode operation.
type Opcode byte

// Opcode definitions with fixed numeric values for version compatibility.
// Categories are organized by the high nibble for efficient dispatch.
const (
	// 0x0X - Special and stack operations
	OpConstant Opcode = 0x01 // Load constant from constant pool
	OpNull     Opcode = 0x02 // Push null onto stack
	OpTrue     Opcode = 0x03 // Push true onto stack
	OpFalse    Opcode = 0x04 // Push false onto stack
	OpPop      Opcode = 0x05 // Pop value from stack
	OpDup      Opcode = 0x06 // Duplicate top of stack
	OpSwap     Opcode = 0x07 // Swap top two values on stack
	OpRot3     Opcode = 0x08 // Rotate top 3 values: [a, b, c] -> [c, a, b]

	// 0x1X - Arithmetic operations
	OpAdd  Opcode = 0x10 // Addition
	OpSub  Opcode = 0x11 // Subtraction
	OpMul  Opcode = 0x12 // Multiplication
	OpDiv  Opcode = 0x13 // Division
	OpMod  Opcode = 0x14 // Modulo
	OpNeg  Opcode = 0x15 // Negation

	// 0x2X - Comparison operations
	OpEqual        Opcode = 0x20 // Equality comparison
	OpNotEqual     Opcode = 0x21 // Inequality comparison
	OpLess         Opcode = 0x22 // Less than
	OpLessEqual    Opcode = 0x23 // Less than or equal
	OpGreater      Opcode = 0x24 // Greater than
	OpGreaterEqual Opcode = 0x25 // Greater than or equal

	// 0x3X - Logical operations
	OpAnd Opcode = 0x30 // Logical AND (short-circuit)
	OpOr  Opcode = 0x31 // Logical OR (short-circuit)
	OpNot Opcode = 0x32 // Logical NOT

	// 0x4X - Bitwise operations
	OpBitAnd Opcode = 0x40 // Bitwise AND
	OpBitOr  Opcode = 0x41 // Bitwise OR
	OpBitXor Opcode = 0x42 // Bitwise XOR
	OpBitNot Opcode = 0x43 // Bitwise NOT
	OpShl    Opcode = 0x44 // Left shift
	OpShr    Opcode = 0x45 // Right shift

	// 0x5X - Variable operations
	OpGetLocal   Opcode = 0x50 // Get local variable
	OpSetLocal   Opcode = 0x51 // Set local variable
	OpGetGlobal  Opcode = 0x52 // Get global variable
	OpSetGlobal  Opcode = 0x53 // Set global variable
	OpGetFree    Opcode = 0x54 // Get free variable (closure)
	OpGetBuiltin Opcode = 0x55 // Get built-in function

	// 0x6X - Control flow operations
	OpJump         Opcode = 0x60 // Unconditional jump
	OpJumpNotTrue  Opcode = 0x61 // Jump if top of stack is not true (always pops)
	OpJumpTrue     Opcode = 0x67 // Jump if top of stack is true (always pops)
	OpCall         Opcode = 0x62 // Call function
	OpReturn       Opcode = 0x63 // Return from function (no value)
	OpReturnValue  Opcode = 0x64 // Return from function with value
	OpBreak        Opcode = 0x65 // Break from loop
	OpContinue     Opcode = 0x66 // Continue loop

	// 0x7X - Array and Map operations
	OpArray    Opcode = 0x70 // Create array
	OpMap      Opcode = 0x71 // Create map
	OpIndex    Opcode = 0x72 // Index access
	OpSetIndex Opcode = 0x73 // Index assignment
	OpSlice    Opcode = 0x74 // Array slice

	// 0x8X - Function and closure operations
	OpClosure Opcode = 0x80 // Create closure
	OpGetUp   Opcode = 0x81 // Get upvalue (free variable)
	OpSetUp   Opcode = 0x82 // Set upvalue (free variable)

	// 0x9X - Exception operations
	OpThrow      Opcode = 0x90 // Throw exception
	OpTry        Opcode = 0x91 // Start try block
	OpCatch      Opcode = 0x92 // Start catch block
	OpEndTry     Opcode = 0x93 // End try-catch block
	OpPushTry    Opcode = 0x94 // Push try handler
	OpPopTry     Opcode = 0x95 // Pop try handler
	OpCatchStart Opcode = 0x96 // Start catch block execution (adjust basePointer)
)

// OpcodeWidths defines the width (in bytes) of each opcode's operands.
// 0 means the opcode has no operands.
// 2 means the opcode has a 2-byte operand.
var OpcodeWidths = map[Opcode]int{
	OpConstant:     2,
	OpNull:         0,
	OpTrue:         0,
	OpFalse:        0,
	OpPop:          0,
	OpDup:          0,
	OpSwap:         0,
	OpRot3:         0,
	OpAdd:          0,
	OpSub:          0,
	OpMul:          0,
	OpDiv:          0,
	OpMod:          0,
	OpNeg:          0,
	OpEqual:        0,
	OpNotEqual:     0,
	OpLess:         0,
	OpLessEqual:    0,
	OpGreater:      0,
	OpGreaterEqual: 0,
	OpAnd:          0,
	OpOr:           0,
	OpNot:          0,
	OpBitAnd:       0,
	OpBitOr:        0,
	OpBitXor:       0,
	OpBitNot:       0,
	OpShl:          0,
	OpShr:          0,
	OpGetLocal:     1,
	OpSetLocal:     1,
	OpGetGlobal:    2,
	OpSetGlobal:    2,
	OpGetFree:      1,
	OpGetBuiltin:   1,
	OpJump:         2,
	OpJumpNotTrue:  2,
	OpJumpTrue:     2,
	OpCall:         1,
	OpReturn:       0,
	OpReturnValue:  0,
	OpBreak:        0,
	OpContinue:     0,
	OpArray:        2,
	OpMap:          2,
	OpIndex:        0,
	OpSetIndex:     0,
	OpSlice:        0,
	OpClosure:      3, // 2 bytes for function index + 1 byte for free variables count
	OpGetUp:        1,
	OpSetUp:        1,
	OpThrow:        0,
	OpTry:          2,
	OpCatch:        2,
	OpEndTry:       0,
	OpPushTry:      2,
	OpPopTry:       0,
	OpCatchStart:   0,
}

// Make creates a bytecode instruction from an opcode and operands.
func Make(op Opcode, operands ...int) []byte {
	width, ok := OpcodeWidths[op]
	if !ok {
		return []byte{byte(op)}
	}

	// Special handling for OpClosure which has two different-width operands
	if op == OpClosure && len(operands) == 2 {
		instruction := make([]byte, 4) // 1 byte opcode + 2 bytes fn index + 1 byte free count
		instruction[0] = byte(op)
		instruction[1] = byte(operands[0] >> 8)
		instruction[2] = byte(operands[0])
		instruction[3] = byte(operands[1])
		return instruction
	}

	totalLen := 1 + len(operands)*width

	instruction := make([]byte, totalLen)
	instruction[0] = byte(op)

	offset := 1
	for _, o := range operands {
		switch width {
		case 1:
			instruction[offset] = byte(o)
		case 2:
			instruction[offset] = byte(o >> 8)
			instruction[offset+1] = byte(o)
		}
		offset += width
	}

	return instruction
}

// ReadUint16 reads a 2-byte unsigned integer from the instruction at the given offset.
func ReadUint16(ins []byte, offset int) uint16 {
	return uint16(ins[offset])<<8 | uint16(ins[offset+1])
}

// ReadUint8 reads a 1-byte unsigned integer from the instruction at the given offset.
func ReadUint8(ins []byte, offset int) uint8 {
	return ins[offset]
}
