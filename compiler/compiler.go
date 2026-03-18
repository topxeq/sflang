// Package compiler implements the bytecode compiler for Sflang.
// It transforms an AST into bytecode instructions for the VM.
package compiler

import (
	"fmt"

	"github.com/topxeq/sflang/ast"
	"github.com/topxeq/sflang/lexer"
	"github.com/topxeq/sflang/object"
	"github.com/topxeq/sflang/parser"
)

// Bytecode holds the compiled output of the compiler.
type Bytecode struct {
	Instructions []byte
	Constants    []object.Object
}

// Compiler holds the state of the compilation process.
type Compiler struct {
	instructions []byte
	constants    []object.Object
	symbolTable  *SymbolTable

	scopes        []CompilationScope
	scopeIndex    int
	loopContexts  []LoopContext
	tryContexts   []TryContext
}

// CompilationScope holds the bytecode for a single compilation scope.
type CompilationScope struct {
	instructions []byte
}

// LoopContext holds information about a loop for break/continue.
type LoopContext struct {
	breakPositions    []int
	continuePositions []int
	loopStart         int
}

// TryContext holds information about a try-catch block.
type TryContext struct {
	catchStart int
	catchEnd   int
	tryStart   int
}

// New creates a new Compiler instance.
func New() *Compiler {
	return NewWithState(NewSymbolTable(), nil)
}

// NewWithState creates a new Compiler with the given symbol table and constants.
func NewWithState(symbolTable *SymbolTable, constants []object.Object) *Compiler {
	mainScope := CompilationScope{instructions: []byte{}}
	return &Compiler{
		instructions:   []byte{},
		constants:      constants,
		symbolTable:    symbolTable,
		scopes:         []CompilationScope{mainScope},
		scopeIndex:     0,
		loopContexts:   []LoopContext{},
		tryContexts:    []TryContext{},
	}
}

// Compile compiles an AST node into bytecode.
func (c *Compiler) Compile(node ast.Node) error {
	switch node := node.(type) {
	case *ast.Program:
		for _, s := range node.Statements {
			if err := c.Compile(s); err != nil {
				return err
			}
		}

	case *ast.ExpressionStatement:
		// Check if this is a named function expression
		if fn, ok := node.Expression.(*ast.FunctionLiteral); ok && fn.Name != "" {
			// For recursive functions, define the symbol first
			symbol := c.symbolTable.Define(fn.Name)

			// Compile the function
			if err := c.Compile(node.Expression); err != nil {
				return err
			}

			// The closure is on the stack, store it
			if symbol.Scope == GlobalScope {
				c.emit(OpSetGlobal, symbol.Index)
			} else {
				c.emit(OpSetLocal, symbol.Index)
			}
		} else if _, ok := node.Expression.(*ast.PostfixExpression); ok {
			// Postfix expressions (i++, i--) don't leave a value on the stack
			// They are statement-like, so no need to pop
			if err := c.Compile(node.Expression); err != nil {
				return err
			}
		} else {
			if err := c.Compile(node.Expression); err != nil {
				return err
			}
			c.emit(OpPop)
		}

	case *ast.LetStatement:
		if err := c.Compile(node.Value); err != nil {
			return err
		}
		symbol := c.symbolTable.Define(node.Name.Value)
		if symbol.Scope == GlobalScope {
			c.emit(OpSetGlobal, symbol.Index)
		} else {
			c.emit(OpSetLocal, symbol.Index)
		}

	case *ast.AssignStatement:
		if err := c.Compile(node.Right); err != nil {
			return err
		}
		if err := c.compileAssignmentLeft(node.Left); err != nil {
			return err
		}

	case *ast.CompoundAssignStatement:
		// Evaluate left side for compound operations
		if ident, ok := node.Left.(*ast.Identifier); ok {
			symbol, ok := c.symbolTable.Resolve(ident.Value)
			if !ok {
				return fmt.Errorf("undefined variable: %s", ident.Value)
			}
			// Load current value
			if symbol.Scope == GlobalScope {
				c.emit(OpGetGlobal, symbol.Index)
			} else if symbol.Scope == LocalScope {
				c.emit(OpGetLocal, symbol.Index)
			} else if symbol.Scope == FreeScope {
				c.emit(OpGetFree, symbol.Index)
			}
			// Compile right side
			if err := c.Compile(node.Right); err != nil {
				return err
			}
			// Apply operator
			c.compileCompoundOperator(node.Operator)
			// Store result
			if symbol.Scope == GlobalScope {
				c.emit(OpSetGlobal, symbol.Index)
			} else if symbol.Scope == LocalScope {
				c.emit(OpSetLocal, symbol.Index)
			}
		} else if index, ok := node.Left.(*ast.IndexExpression); ok {
			// Load array/map
			if err := c.Compile(index.Left); err != nil {
				return err
			}
			// Load index
			if err := c.Compile(index.Index); err != nil {
				return err
			}
			// Duplicate for get and set
			c.emit(OpDup)
			c.emit(OpDup)
			c.emit(OpIndex)
			// Compile right side
			if err := c.Compile(node.Right); err != nil {
				return err
			}
			// Apply operator
			c.compileCompoundOperator(node.Operator)
			// Store result
			c.emit(OpSetIndex)
		}

	case *ast.ReturnStatement:
		if node.ReturnValue == nil {
			c.emit(OpReturn)
		} else {
			if err := c.Compile(node.ReturnValue); err != nil {
				return err
			}
			c.emit(OpReturnValue)
		}

	case *ast.BlockStatement:
		for _, s := range node.Statements {
			if err := c.Compile(s); err != nil {
				return err
			}
		}

	case *ast.IfExpression:
		if err := c.Compile(node.Condition); err != nil {
			return err
		}

		// Jump to else or end if not true
		jumpNotTruePos := c.emit(OpJumpNotTrue, 9999)

		if err := c.Compile(node.Consequence); err != nil {
			return err
		}

		// Jump over alternative
		jumpPos := c.emit(OpJump, 9999)

		afterConsequence := len(c.currentInstructions())
		c.changeOperand(jumpNotTruePos, afterConsequence)

		if node.Alternative != nil {
			if err := c.Compile(node.Alternative); err != nil {
				return err
			}
		} else {
			c.emit(OpNull)
		}

		afterAlternative := len(c.currentInstructions())
		c.changeOperand(jumpPos, afterAlternative)

	case *ast.ForStatement:
		c.enterLoop()

		// Compile init
		if node.Init != nil {
			if err := c.Compile(node.Init); err != nil {
				return err
			}
		}

		// Mark loop start
		loopStart := len(c.currentInstructions())
		c.currentLoopContext().loopStart = loopStart

		// Compile condition
		if node.Condition != nil {
			if err := c.Compile(node.Condition); err != nil {
				return err
			}
		} else {
			c.emit(OpTrue) // Infinite loop
		}

		// Jump to end if condition is false
		jumpNotTruePos := c.emit(OpJumpNotTrue, 9999)

		// Compile body
		if err := c.Compile(node.Body); err != nil {
			return err
		}

		// Continue target (for update)
		continuePos := len(c.currentInstructions())

		// Compile update
		if node.Update != nil {
			if err := c.Compile(node.Update); err != nil {
				return err
			}
		}

		// Jump back to condition check
		c.emit(OpJump, loopStart)

		// End of loop
		endPos := len(c.currentInstructions())
		c.changeOperand(jumpNotTruePos, endPos)

		// Fix break and continue jumps
		c.leaveLoop(continuePos, endPos)

	case *ast.ForInStatement:
		c.enterLoop()

		// Compile source expression
		if err := c.Compile(node.Source); err != nil {
			return err
		}

		// Call iterator builtin (will be implemented in VM)
		c.emit(OpGetBuiltin, BuiltinIterator)
		c.emit(OpCall, 1)

		// Define loop variables
		if node.Key != nil {
			c.symbolTable.Define(node.Key.Value)
		}
		if node.Value != nil {
			c.symbolTable.Define(node.Value.Value)
		}

		// Loop start
		loopStart := len(c.currentInstructions())
		c.currentLoopContext().loopStart = loopStart

		// Call iterator next
		c.emit(OpDup) // Duplicate iterator
		c.emit(OpGetBuiltin, BuiltinNext)
		c.emit(OpCall, 1)

		// Check if done (returns null when done)
		c.emit(OpNull)
		c.emit(OpEqual)
		jumpNotTruePos := c.emit(OpJumpNotTrue, 9999)

		// Unpack key, value (implementation depends on iterator protocol)
		// For now, this is a simplified version

		// Compile body
		if err := c.Compile(node.Body); err != nil {
			return err
		}

		// Jump back to loop start
		continuePos := len(c.currentInstructions())
		c.emit(OpJump, loopStart)

		// End of loop
		endPos := len(c.currentInstructions())
		c.changeOperand(jumpNotTruePos, endPos)
		c.emit(OpPop) // Pop iterator

		c.leaveLoop(continuePos, endPos)

	case *ast.BreakStatement:
		if len(c.loopContexts) == 0 {
			return fmt.Errorf("'break' outside of loop")
		}
		pos := c.emit(OpJump, 9999)
		c.currentLoopContext().breakPositions = append(c.currentLoopContext().breakPositions, pos)

	case *ast.ContinueStatement:
		if len(c.loopContexts) == 0 {
			return fmt.Errorf("'continue' outside of loop")
		}
		pos := c.emit(OpJump, 9999)
		c.currentLoopContext().continuePositions = append(c.currentLoopContext().continuePositions, pos)

	case *ast.TryStatement:
		// Push try handler
		catchJumpPos := c.emit(OpPushTry, 9999)

		// Compile try body
		if err := c.Compile(node.Body); err != nil {
			return err
		}

		// Pop try handler and jump over catch
		c.emit(OpPopTry)
		jumpOverCatchPos := c.emit(OpJump, 9999)

		// Catch block position
		catchStart := len(c.currentInstructions())
		c.changeOperand(catchJumpPos, catchStart)

		// Define catch variable
		if node.CatchVar != nil {
			c.symbolTable.Define(node.CatchVar.Value)
		}

		// Compile catch body
		if err := c.Compile(node.CatchBody); err != nil {
			return err
		}

		// After catch
		afterCatch := len(c.currentInstructions())
		c.changeOperand(jumpOverCatchPos, afterCatch)

	case *ast.ThrowStatement:
		if err := c.Compile(node.ErrExpr); err != nil {
			return err
		}
		c.emit(OpThrow)

	case *ast.Identifier:
		symbol, ok := c.symbolTable.Resolve(node.Value)
		if !ok {
			return fmt.Errorf("undefined variable: %s", node.Value)
		}
		c.loadSymbol(symbol)

	case *ast.IntegerLiteral:
		c.emit(OpConstant, c.addConstant(object.GetInteger(node.Value)))

	case *ast.FloatLiteral:
		fl := &object.Float{Value: node.Value}
		c.emit(OpConstant, c.addConstant(fl))

	case *ast.StringLiteral:
		str := &object.String{Value: node.Value}
		c.emit(OpConstant, c.addConstant(str))

	case *ast.BooleanLiteral:
		if node.Value {
			c.emit(OpTrue)
		} else {
			c.emit(OpFalse)
		}

	case *ast.NullLiteral:
		c.emit(OpNull)

	case *ast.PrefixExpression:
		if err := c.Compile(node.Right); err != nil {
			return err
		}
		switch node.Operator {
		case "-":
			c.emit(OpNeg)
		case "!":
			c.emit(OpNot)
		case "~":
			c.emit(OpBitNot)
		default:
			return fmt.Errorf("unknown operator: %s", node.Operator)
		}

	case *ast.InfixExpression:
		if node.Operator == "&&" {
			return c.compileLogicalAnd(node)
		} else if node.Operator == "||" {
			return c.compileLogicalOr(node)
		}

		if err := c.Compile(node.Left); err != nil {
			return err
		}
		if err := c.Compile(node.Right); err != nil {
			return err
		}

		switch node.Operator {
		case "+":
			c.emit(OpAdd)
		case "-":
			c.emit(OpSub)
		case "*":
			c.emit(OpMul)
		case "/":
			c.emit(OpDiv)
		case "%":
			c.emit(OpMod)
		case "==":
			c.emit(OpEqual)
		case "!=":
			c.emit(OpNotEqual)
		case "<":
			c.emit(OpLess)
		case "<=":
			c.emit(OpLessEqual)
		case ">":
			c.emit(OpGreater)
		case ">=":
			c.emit(OpGreaterEqual)
		case "&":
			c.emit(OpBitAnd)
		case "|":
			c.emit(OpBitOr)
		case "^":
			c.emit(OpBitXor)
		case "<<":
			c.emit(OpShl)
		case ">>":
			c.emit(OpShr)
		default:
			return fmt.Errorf("unknown operator: %s", node.Operator)
		}

	case *ast.PostfixExpression:
		// i++ or i--
		if ident, ok := node.Left.(*ast.Identifier); ok {
			symbol, ok := c.symbolTable.Resolve(ident.Value)
			if !ok {
				return fmt.Errorf("undefined variable: %s", ident.Value)
			}
			// Load current value
			c.loadSymbol(symbol)
			// Apply increment/decrement
			c.emit(OpConstant, c.addConstant(object.GetInteger(1)))
			if node.Operator == "++" {
				c.emit(OpAdd)
			} else {
				c.emit(OpSub)
			}
			// Store result (OpSetGlobal/OpSetLocal pops the value)
			if symbol.Scope == GlobalScope {
				c.emit(OpSetGlobal, symbol.Index)
			} else if symbol.Scope == LocalScope {
				c.emit(OpSetLocal, symbol.Index)
			}
			// Don't leave a value on the stack - this is a statement form
		}

	case *ast.CallExpression:
		if err := c.Compile(node.Function); err != nil {
			return err
		}
		for _, arg := range node.Arguments {
			if err := c.Compile(arg); err != nil {
				return err
			}
		}
		c.emit(OpCall, len(node.Arguments))

	case *ast.ArrayLiteral:
		for _, el := range node.Elements {
			if err := c.Compile(el); err != nil {
				return err
			}
		}
		c.emit(OpArray, len(node.Elements))

	case *ast.MapLiteral:
		size := len(node.Pairs)
		for key, value := range node.Pairs {
			if err := c.Compile(key); err != nil {
				return err
			}
			if err := c.Compile(value); err != nil {
				return err
			}
		}
		c.emit(OpMap, size*2)

	case *ast.IndexExpression:
		if err := c.Compile(node.Left); err != nil {
			return err
		}
		if err := c.Compile(node.Index); err != nil {
			return err
		}
		c.emit(OpIndex)

	case *ast.FunctionLiteral:
		c.enterScope()

		for _, p := range node.Parameters {
			c.symbolTable.Define(p.Value)
		}

		if err := c.Compile(node.Body); err != nil {
			return err
		}

		if len(c.currentInstructions()) == 0 || c.currentInstructions()[len(c.currentInstructions())-1] != byte(OpReturnValue) {
			c.emit(OpReturn)
		}

		freeSymbols := c.symbolTable.FreeSymbols
		numLocals := c.symbolTable.NumDefinitions()
		instructions := c.leaveScope()

		compiledFn := &object.CompiledFunction{
			Instructions:  instructions,
			NumLocals:     numLocals,
			NumParameters: len(node.Parameters),
		}

		fnIndex := c.addConstant(compiledFn)
		c.emit(OpClosure, fnIndex, len(freeSymbols))

	default:
		return fmt.Errorf("unknown node type: %T", node)
	}

	return nil
}

// compileAssignmentLeft handles the left side of an assignment.
func (c *Compiler) compileAssignmentLeft(left ast.Expression) error {
	switch left := left.(type) {
	case *ast.Identifier:
		symbol, ok := c.symbolTable.Resolve(left.Value)
		if !ok {
			return fmt.Errorf("undefined variable: %s", left.Value)
		}
		if symbol.Scope == GlobalScope {
			c.emit(OpSetGlobal, symbol.Index)
		} else if symbol.Scope == LocalScope {
			c.emit(OpSetLocal, symbol.Index)
		} else if symbol.Scope == FreeScope {
			// Free variables need special handling
			c.emit(OpGetUp, symbol.Index)
		}
	case *ast.IndexExpression:
		// Stack: array, index, value -> set array[index] = value
		if err := c.Compile(left.Left); err != nil {
			return err
		}
		if err := c.Compile(left.Index); err != nil {
			return err
		}
		// Reorder stack: currently have [value, array, index], need [array, index, value]
		// This requires swapping - for simplicity, we use a different approach
		c.emit(OpSetIndex)
	default:
		return fmt.Errorf("cannot assign to %T", left)
	}
	return nil
}

// compileCompoundOperator compiles a compound assignment operator.
func (c *Compiler) compileCompoundOperator(op string) {
	switch op {
	case "+=":
		c.emit(OpAdd)
	case "-=":
		c.emit(OpSub)
	case "*=":
		c.emit(OpMul)
	case "/=":
		c.emit(OpDiv)
	case "%=":
		c.emit(OpMod)
	}
}

// compileLogicalAnd compiles a short-circuit && operator.
func (c *Compiler) compileLogicalAnd(node *ast.InfixExpression) error {
	if err := c.Compile(node.Left); err != nil {
		return err
	}

	jumpPos := c.emit(OpJumpNotTrue, 9999)
	c.emit(OpPop)

	if err := c.Compile(node.Right); err != nil {
		return err
	}

	endPos := len(c.currentInstructions())
	c.changeOperand(jumpPos, endPos)

	return nil
}

// compileLogicalOr compiles a short-circuit || operator.
func (c *Compiler) compileLogicalOr(node *ast.InfixExpression) error {
	if err := c.Compile(node.Left); err != nil {
		return err
	}

	// Jump if true (skip right side)
	jumpPos := c.emit(OpJump, 9999)

	// If false, evaluate right side
	c.emit(OpPop)
	if err := c.Compile(node.Right); err != nil {
		return err
	}

	endPos := len(c.currentInstructions())
	c.changeOperand(jumpPos, endPos)

	return nil
}

// loadSymbol emits bytecode to load a symbol onto the stack.
func (c *Compiler) loadSymbol(s Symbol) {
	switch s.Scope {
	case GlobalScope:
		c.emit(OpGetGlobal, s.Index)
	case LocalScope:
		c.emit(OpGetLocal, s.Index)
	case BuiltinScope:
		c.emit(OpGetBuiltin, s.Index)
	case FreeScope:
		c.emit(OpGetFree, s.Index)
	}
}

// emit adds an instruction to the current scope's bytecode.
func (c *Compiler) emit(op Opcode, operands ...int) int {
	ins := Make(op, operands...)
	pos := len(c.currentInstructions())
	c.scopes[c.scopeIndex].instructions = append(c.currentInstructions(), ins...)
	return pos
}

// addConstant adds a constant to the constant pool and returns its index.
func (c *Compiler) addConstant(obj object.Object) int {
	c.constants = append(c.constants, obj)
	return len(c.constants) - 1
}

// currentInstructions returns the instructions of the current scope.
func (c *Compiler) currentInstructions() []byte {
	return c.scopes[c.scopeIndex].instructions
}

// changeOperand changes the operand of an instruction at the given position.
func (c *Compiler) changeOperand(pos int, operand int) {
	op := Opcode(c.currentInstructions()[pos])
	newIns := Make(op, operand)
	ins := c.currentInstructions()
	for i := 0; i < len(newIns); i++ {
		ins[pos+i] = newIns[i]
	}
}

// enterScope creates a new compilation scope.
func (c *Compiler) enterScope() {
	scope := CompilationScope{instructions: []byte{}}
	c.scopes = append(c.scopes, scope)
	c.scopeIndex++
	c.symbolTable = NewEnclosedSymbolTable(c.symbolTable)
}

// leaveScope exits the current scope and returns its instructions.
func (c *Compiler) leaveScope() []byte {
	instructions := c.currentInstructions()
	c.scopes = c.scopes[:len(c.scopes)-1]
	c.scopeIndex--
	c.symbolTable = c.symbolTable.Outer
	return instructions
}

// enterLoop enters a new loop context.
func (c *Compiler) enterLoop() {
	c.loopContexts = append(c.loopContexts, LoopContext{})
}

// leaveLoop exits the current loop context and fixes break/continue jumps.
func (c *Compiler) leaveLoop(continuePos, endPos int) {
	ctx := c.currentLoopContext()
	for _, pos := range ctx.breakPositions {
		c.changeOperand(pos, endPos)
	}
	for _, pos := range ctx.continuePositions {
		c.changeOperand(pos, continuePos)
	}
	c.loopContexts = c.loopContexts[:len(c.loopContexts)-1]
}

// currentLoopContext returns the current loop context.
func (c *Compiler) currentLoopContext() *LoopContext {
	return &c.loopContexts[len(c.loopContexts)-1]
}

// Bytecode returns the compiled bytecode.
func (c *Compiler) Bytecode() *Bytecode {
	return &Bytecode{
		Instructions: c.currentInstructions(),
		Constants:    c.constants,
	}
}

// Constants returns the constant pool.
func (c *Compiler) Constants() []object.Object {
	return c.constants
}

// SetConstants sets the constant pool.
func (c *Compiler) SetConstants(constants []object.Object) {
	c.constants = constants
}

// Built-in function indices
const (
	BuiltinIterator = 0
	BuiltinNext     = 1
)

// DefineBuiltins defines built-in functions in the symbol table.
func (c *Compiler) DefineBuiltins(builtins []string) {
	for i, name := range builtins {
		c.symbolTable.DefineBuiltin(i, name)
	}
}

// CompileString compiles a string of Sflang source code.
func CompileString(source string) (*Bytecode, error) {
	l := lexer.New(source)
	p := parser.New(l)
	program := p.ParseProgram()

	if len(p.Errors()) > 0 {
		return nil, fmt.Errorf("parse errors: %v", p.Errors())
	}

	c := New()
	if err := c.Compile(program); err != nil {
		return nil, err
	}

	return c.Bytecode(), nil
}
