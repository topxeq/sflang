// Package main is the entry point for the Sflang interpreter.
// Sflang is a lightweight, fast interpreted programming language.
package main

import (
	"flag"
	"fmt"
	"os"

	"github.com/topxeq/sflang/builtin"
	"github.com/topxeq/sflang/compiler"
	"github.com/topxeq/sflang/lexer"
	"github.com/topxeq/sflang/object"
	"github.com/topxeq/sflang/parser"
	"github.com/topxeq/sflang/repl"
	"github.com/topxeq/sflang/vm"
)

// Version information
const (
	Version   = "0.1.0"
	BuildDate = "2024-03-18"
)

func main() {
	// Command-line flags
	var (
		showVersion  bool
		showHelp     bool
		printAst     bool
		printBytecode bool
	)

	flag.BoolVar(&showVersion, "version", false, "Print version information")
	flag.BoolVar(&showVersion, "v", false, "Print version information (shorthand)")
	flag.BoolVar(&showHelp, "help", false, "Print help information")
	flag.BoolVar(&showHelp, "h", false, "Print help information (shorthand)")
	flag.BoolVar(&printAst, "ast", false, "Print the AST and exit")
	flag.BoolVar(&printBytecode, "bc", false, "Print the bytecode and exit")

	flag.Parse()

	// Handle flags
	if showVersion {
		fmt.Printf("Sflang v%s (built %s)\n", Version, BuildDate)
		fmt.Println("A lightweight, fast interpreted programming language")
		return
	}

	if showHelp {
		printUsage()
		return
	}

	// Initialize built-in functions
	initBuiltins()

	args := flag.Args()

	// If no file is specified, start REPL
	if len(args) == 0 {
		repl.Start(os.Stdin, os.Stdout)
		return
	}

	// Execute the specified file
	filename := args[0]
	if err := runFile(filename, printAst, printBytecode, args[1:]); err != nil {
		fmt.Fprintf(os.Stderr, "Error: %s\n", err)
		os.Exit(1)
	}
}

// printUsage displays the usage information.
func printUsage() {
	fmt.Println("Sflang - A lightweight, fast interpreted programming language")
	fmt.Println()
	fmt.Println("Usage:")
	fmt.Println("  sf [options] [file] [args...]")
	fmt.Println()
	fmt.Println("Options:")
	flag.PrintDefaults()
	fmt.Println()
	fmt.Println("Examples:")
	fmt.Println("  sf              Start the REPL")
	fmt.Println("  sf script.sf    Run a script file")
	fmt.Println("  sf -ast file.sf Print the AST for the file")
	fmt.Println("  sf -bc file.sf  Print the bytecode for the file")
}

// runFile executes a Sflang script file.
func runFile(filename string, printAst, printBytecode bool, args []string) error {
	// Read the file
	source, err := os.ReadFile(filename)
	if err != nil {
		return fmt.Errorf("failed to read file '%s': %w", filename, err)
	}

	// Create lexer
	l := lexer.New(string(source))

	// Parse
	p := parser.New(l)
	program := p.ParseProgram()

	if len(p.Errors()) > 0 {
		return fmt.Errorf("parse errors:\n%s", formatErrors(p.Errors()))
	}

	// Print AST if requested
	if printAst {
		fmt.Println(program.String())
		return nil
	}

	// Create symbol table with built-in functions
	symbolTable := compiler.NewSymbolTable()
	for i, name := range builtin.GetBuiltinNames() {
		symbolTable.DefineBuiltin(i, name)
	}

	// Define argsG as a global variable before compilation
	argsSymbol := symbolTable.Define("argsG")

	// Compile
	c := compiler.NewWithState(symbolTable, nil)
	if err := c.Compile(program); err != nil {
		return fmt.Errorf("compilation error: %w", err)
	}

	// Print bytecode if requested
	if printBytecode {
		fmt.Println("Bytecode:")
		fmt.Println("---")
		printBytecodeInfo(c.Bytecode())
		fmt.Println("---")
		return nil
	}

	// Create globals array and pre-populate argsG
	globals := make([]object.Object, vm.GlobalSize)

	// Convert args to Sflang Array of Strings
	argsElements := make([]object.Object, len(args)+1)
	// First element is the script path
	argsElements[0] = &object.String{Value: filename}
	// Remaining elements are the script arguments
	for i, arg := range args {
		argsElements[i+1] = &object.String{Value: arg}
	}
	globals[argsSymbol.Index] = &object.Array{Elements: argsElements}

	// Run the VM with pre-populated globals
	machine := vm.NewWithGlobals(c.Bytecode(), globals)
	if err := machine.Run(); err != nil {
		return fmt.Errorf("runtime error: %w", err)
	}

	return nil
}

// initBuiltins initializes the built-in functions.
func initBuiltins() {
	builtins := make([]*object.Builtin, len(builtin.Builtins))
	for i, b := range builtin.Builtins {
		builtins[i] = object.NewBuiltin(b.Fn)
	}
	object.RegisterBuiltins(builtins)
}

// formatErrors formats a list of errors into a string.
func formatErrors(errors []string) string {
	result := ""
	for _, err := range errors {
		result += "  " + err + "\n"
	}
	return result
}

// printBytecodeInfo prints information about the compiled bytecode.
func printBytecodeInfo(bc *compiler.Bytecode) {
	fmt.Printf("Constants: %d\n", len(bc.Constants))
	for i, c := range bc.Constants {
		fmt.Printf("  %d: %s (%s)\n", i, c.Inspect(), c.Type())
	}

	fmt.Printf("\nInstructions: %d bytes\n", len(bc.Instructions))
	fmt.Println("  (use a disassembler for detailed output)")
}