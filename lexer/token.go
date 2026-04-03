// Package lexer defines Token types and constants for lexical analysis.
package lexer

// TokenType represents the type of a token as a string identifier.
type TokenType string

// Token represents a lexical token with its type, literal value, and position.
type Token struct {
	Type    TokenType // The type of the token
	Literal string    // The literal value of the token
	Line    int       // Line number (1-based)
	Column  int       // Column number (1-based)
}

// Token type constants define all possible token types in Sflang.
const (
	// Special tokens
	ILLEGAL TokenType = "ILLEGAL" // Unknown/illegal character
	EOF     TokenType = "EOF"     // End of file

	// Identifiers and literals
	IDENT    TokenType = "IDENT"    // Identifier (variable/function name)
	INT      TokenType = "INT"      // Integer literal
	FLOAT    TokenType = "FLOAT"    // Floating-point literal
	BIGINT   TokenType = "BIGINT"   // Big integer literal (suffix 'n')
	BIGFLOAT TokenType = "BIGFLOAT" // Big float literal (suffix 'm')
	STRING   TokenType = "STRING"   // String literal

	// Operators
	ASSIGN   TokenType = "="   // Assignment
	DEFINE   TokenType = ":="  // Short variable declaration
	PLUS     TokenType = "+"   // Addition
	MINUS    TokenType = "-"   // Subtraction/Negation
	BANG     TokenType = "!"   // Logical NOT
	ASTERISK TokenType = "*"   // Multiplication
	SLASH    TokenType = "/"   // Division
	PERCENT  TokenType = "%"   // Modulo

	// Comparison operators
	EQ       TokenType = "==" // Equal
	NOT_EQ   TokenType = "!=" // Not equal
	LT       TokenType = "<"  // Less than
	GT       TokenType = ">"  // Greater than
	LT_EQ    TokenType = "<=" // Less than or equal
	GT_EQ    TokenType = ">=" // Greater than or equal

	// Logical operators
	AND TokenType = "&&" // Logical AND
	OR  TokenType = "||" // Logical OR

	// Bitwise operators
	BIT_AND TokenType = "&"  // Bitwise AND
	BIT_OR  TokenType = "|"  // Bitwise OR
	BIT_XOR TokenType = "^"  // Bitwise XOR
	BIT_NOT TokenType = "~"  // Bitwise NOT
	SHL     TokenType = "<<" // Left shift
	SHR     TokenType = ">>" // Right shift

	// Compound assignment operators
	PLUS_ASSIGN  TokenType = "+=" // Addition assignment
	MINUS_ASSIGN TokenType = "-=" // Subtraction assignment
	MUL_ASSIGN   TokenType = "*=" // Multiplication assignment
	DIV_ASSIGN   TokenType = "/=" // Division assignment
	MOD_ASSIGN   TokenType = "%=" // Modulo assignment

	// Increment/Decrement
	INCREMENT TokenType = "++" // Increment
	DECREMENT TokenType = "--" // Decrement

	// Delimiters
	COMMA     TokenType = "," // Comma
	COLON     TokenType = ":" // Colon
	SEMICOLON TokenType = ";" // Semicolon (optional)
	DOT       TokenType = "." // Dot (for member access)

	// Brackets
	LPAREN   TokenType = "(" // Left parenthesis
	RPAREN   TokenType = ")" // Right parenthesis
	LBRACE   TokenType = "{" // Left brace
	RBRACE   TokenType = "}" // Right brace
	LBRACKET TokenType = "[" // Left bracket
	RBRACKET TokenType = "]" // Right bracket

	// Variadic
	ELLIPSIS TokenType = "..." // Ellipsis for variadic parameters

	// Keywords
	FN     TokenType = "func"   // Function declaration
	LET    TokenType = "let"    // Variable declaration
	VAR    TokenType = "var"    // Variable declaration without initialization
	IF     TokenType = "if"     // If statement
	ELSE   TokenType = "else"   // Else clause
	FOR    TokenType = "for"    // For loop
	IN     TokenType = "in"     // For-in loop
	RETURN TokenType = "return" // Return statement
	BREAK  TokenType = "break"  // Break statement
	CONTINUE TokenType = "continue" // Continue statement

	// Boolean and null literals
	TRUE  TokenType = "true"  // Boolean true
	FALSE TokenType = "false" // Boolean false
	NULL  TokenType = "null"  // Null value

	// Exception handling
	TRY     TokenType = "try"     // Try block
	CATCH   TokenType = "catch"   // Catch block
	FINALLY TokenType = "finally" // Finally block
	THROW   TokenType = "throw"   // Throw statement
)

// Keywords maps keyword strings to their token types.
var Keywords = map[string]TokenType{
	"func":     FN,
	"let":      LET,
	"var":      VAR,
	"if":       IF,
	"else":     ELSE,
	"for":      FOR,
	"in":       IN,
	"return":   RETURN,
	"break":    BREAK,
	"continue": CONTINUE,
	"true":     TRUE,
	"false":    FALSE,
	"null":     NULL,
	"try":      TRY,
	"catch":    CATCH,
	"finally":  FINALLY,
	"throw":    THROW,
}

// LookupIdent checks if an identifier is a keyword.
// Returns the keyword's token type if it is, otherwise returns IDENT.
func LookupIdent(ident string) TokenType {
	if tok, ok := Keywords[ident]; ok {
		return tok
	}
	return IDENT
}
