// Package repl provides an interactive Read-Eval-Print Loop for Sflang.
package repl

import (
	"bufio"
	"fmt"
	"io"

	"github.com/topxeq/sflang/builtin"
	"github.com/topxeq/sflang/compiler"
	"github.com/topxeq/sflang/lexer"
	"github.com/topxeq/sflang/object"
	"github.com/topxeq/sflang/parser"
	"github.com/topxeq/sflang/vm"
)

const PROMPT = "sf> "

// Start begins the REPL session.
func Start(in io.Reader, out io.Writer) {
	// Initialize built-in functions
	initBuiltins()

	scanner := bufio.NewScanner(in)

	// Create a symbol table with built-in functions
	symbolTable := compiler.NewSymbolTable()
	for i, name := range builtin.GetBuiltinNames() {
		symbolTable.DefineBuiltin(i, name)
	}

	// Global scope for the REPL session
	globals := make([]object.Object, vm.GlobalSize)

	var constants []object.Object
	var lastResult object.Object

	for {
		fmt.Fprint(out, PROMPT)

		scanned := scanner.Scan()
		if !scanned {
			return
		}

		line := scanner.Text()
		if line == "" {
			continue
		}

		// Handle special commands
		switch line {
		case ".exit", ".quit":
			fmt.Fprintln(out, "Bye!")
			return
		case ".help":
			printHelp(out)
			continue
		case ".clear":
			// Reset the REPL state
			constants = nil
			globals = make([]object.Object, vm.GlobalSize)
			continue
		}

		// Parse, compile and execute
		l := lexer.New(line)
		p := parser.New(l)
		program := p.ParseProgram()

		if len(p.Errors()) != 0 {
			printParserErrors(out, p.Errors())
			continue
		}

		c := compiler.NewWithState(symbolTable, constants)
		if err := c.Compile(program); err != nil {
			fmt.Fprintf(out, "Compilation error:\n  %s\n", err)
			continue
		}

		constants = c.Constants()

		machine := vm.NewWithGlobals(c.Bytecode(), globals)
		if err := machine.Run(); err != nil {
			fmt.Fprintf(out, "Runtime error:\n  %s\n", err)
			continue
		}

		lastResult = machine.StackTop()
		if lastResult != nil && lastResult != object.NULL {
			fmt.Fprintln(out, lastResult.Inspect())
		}
	}
}

// printHelp displays the help message.
func printHelp(out io.Writer) {
	fmt.Fprintln(out, "Sflang REPL Commands:")
	fmt.Fprintln(out, "  .exit, .quit  - Exit the REPL")
	fmt.Fprintln(out, "  .clear        - Clear the REPL state")
	fmt.Fprintln(out, "  .help         - Show this help message")
	fmt.Fprintln(out, "")
	fmt.Fprintln(out, "Available built-in functions:")
	for _, name := range builtin.GetBuiltinNames() {
		fmt.Fprintf(out, "  %s\n", name)
	}
}

// printParserErrors displays parser errors.
func printParserErrors(out io.Writer, errors []string) {
	fmt.Fprintln(out, "Parser errors:")
	for _, msg := range errors {
		fmt.Fprintf(out, "  %s\n", msg)
	}
}

// initBuiltins initializes the built-in functions.
func initBuiltins() {
	builtins := make([]*object.Builtin, len(builtin.Builtins))
	for i, b := range builtin.Builtins {
		builtins[i] = object.NewBuiltin(b.Fn)
	}
	object.RegisterBuiltins(builtins)
}