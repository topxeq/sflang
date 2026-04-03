// Package lexer implements lexical analysis for Sflang source code.
// It converts source code text into a stream of tokens for the parser.
package lexer

import "strings"

// Lexer holds the state of the lexical analysis process.
type Lexer struct {
	input        string     // The source code being lexed
	position     int        // Current position in input (points to current char)
	readPosition int        // Current reading position in input (after current char)
	ch           byte       // Current character under examination
	line         int        // Current line number (1-based)
	column       int        // Current column number (1-based)
}

// New creates a new Lexer instance for the given input string.
func New(input string) *Lexer {
	l := &Lexer{
		input:  input,
		line:   1,
		column: 0,
	}
	l.readChar()
	return l
}

// readChar reads the next character from input and advances the position.
func (l *Lexer) readChar() {
	l.column++
	if l.readPosition >= len(l.input) {
		l.ch = 0 // ASCII NUL character represents EOF
	} else {
		l.ch = l.input[l.readPosition]
	}
	l.position = l.readPosition
	l.readPosition++

	// Track line and column numbers
	if l.ch == '\n' {
		l.line++
		l.column = 0
	}
}

// NextToken reads and returns the next token from the input.
// This is the main entry point for lexical analysis.
func (l *Lexer) NextToken() Token {
	var tok Token

	l.skipWhitespace()

	// Record token position before processing
	line := l.line
	column := l.column

	switch l.ch {
	case '=':
		if l.peekChar() == '=' {
			ch := l.ch
			l.readChar()
			tok = Token{Type: EQ, Literal: string(ch) + string(l.ch), Line: line, Column: column}
		} else {
			tok = Token{Type: ASSIGN, Literal: string(l.ch), Line: line, Column: column}
		}
	case '+':
		if l.peekChar() == '=' {
			l.readChar()
			tok = Token{Type: PLUS_ASSIGN, Literal: "+=", Line: line, Column: column}
		} else if l.peekChar() == '+' {
			l.readChar()
			tok = Token{Type: INCREMENT, Literal: "++", Line: line, Column: column}
		} else {
			tok = Token{Type: PLUS, Literal: string(l.ch), Line: line, Column: column}
		}
	case '-':
		if l.peekChar() == '=' {
			l.readChar()
			tok = Token{Type: MINUS_ASSIGN, Literal: "-=", Line: line, Column: column}
		} else if l.peekChar() == '-' {
			l.readChar()
			tok = Token{Type: DECREMENT, Literal: "--", Line: line, Column: column}
		} else {
			tok = Token{Type: MINUS, Literal: string(l.ch), Line: line, Column: column}
		}
	case '*':
		if l.peekChar() == '=' {
			l.readChar()
			tok = Token{Type: MUL_ASSIGN, Literal: "*=", Line: line, Column: column}
		} else {
			tok = Token{Type: ASTERISK, Literal: string(l.ch), Line: line, Column: column}
		}
	case '/':
		if l.peekChar() == '/' {
			// Single-line comment
			l.skipComment()
			return l.NextToken()
		} else if l.peekChar() == '*' {
			// Multi-line comment
			l.skipBlockComment()
			return l.NextToken()
		} else if l.peekChar() == '=' {
			l.readChar()
			tok = Token{Type: DIV_ASSIGN, Literal: "/=", Line: line, Column: column}
		} else {
			tok = Token{Type: SLASH, Literal: string(l.ch), Line: line, Column: column}
		}
	case '%':
		if l.peekChar() == '=' {
			l.readChar()
			tok = Token{Type: MOD_ASSIGN, Literal: "%=", Line: line, Column: column}
		} else {
			tok = Token{Type: PERCENT, Literal: string(l.ch), Line: line, Column: column}
		}
	case '!':
		if l.peekChar() == '=' {
			ch := l.ch
			l.readChar()
			tok = Token{Type: NOT_EQ, Literal: string(ch) + string(l.ch), Line: line, Column: column}
		} else {
			tok = Token{Type: BANG, Literal: string(l.ch), Line: line, Column: column}
		}
	case '<':
		if l.peekChar() == '=' {
			l.readChar()
			tok = Token{Type: LT_EQ, Literal: "<=", Line: line, Column: column}
		} else if l.peekChar() == '<' {
			l.readChar()
			tok = Token{Type: SHL, Literal: "<<", Line: line, Column: column}
		} else {
			tok = Token{Type: LT, Literal: string(l.ch), Line: line, Column: column}
		}
	case '>':
		if l.peekChar() == '=' {
			l.readChar()
			tok = Token{Type: GT_EQ, Literal: ">=", Line: line, Column: column}
		} else if l.peekChar() == '>' {
			l.readChar()
			tok = Token{Type: SHR, Literal: ">>", Line: line, Column: column}
		} else {
			tok = Token{Type: GT, Literal: string(l.ch), Line: line, Column: column}
		}
	case '&':
		if l.peekChar() == '&' {
			l.readChar()
			tok = Token{Type: AND, Literal: "&&", Line: line, Column: column}
		} else {
			tok = Token{Type: BIT_AND, Literal: string(l.ch), Line: line, Column: column}
		}
	case '|':
		if l.peekChar() == '|' {
			l.readChar()
			tok = Token{Type: OR, Literal: "||", Line: line, Column: column}
		} else {
			tok = Token{Type: BIT_OR, Literal: string(l.ch), Line: line, Column: column}
		}
	case '^':
		tok = Token{Type: BIT_XOR, Literal: string(l.ch), Line: line, Column: column}
	case '~':
		tok = Token{Type: BIT_NOT, Literal: string(l.ch), Line: line, Column: column}
	case ',':
		tok = Token{Type: COMMA, Literal: string(l.ch), Line: line, Column: column}
	case ':':
		if l.peekChar() == '=' {
			l.readChar()
			tok = Token{Type: DEFINE, Literal: ":=", Line: line, Column: column}
		} else {
			tok = Token{Type: COLON, Literal: string(l.ch), Line: line, Column: column}
		}
	case ';':
		tok = Token{Type: SEMICOLON, Literal: string(l.ch), Line: line, Column: column}
	case '.':
		if l.peekChar() == '.' {
			l.readChar()
			if l.peekChar() == '.' {
				l.readChar()
				tok = Token{Type: ELLIPSIS, Literal: "...", Line: line, Column: column}
			} else {
				// Two dots - not valid, but treat as two DOT tokens
				tok = Token{Type: DOT, Literal: ".", Line: line, Column: column}
			}
		} else {
			tok = Token{Type: DOT, Literal: string(l.ch), Line: line, Column: column}
		}
	case '(':
		tok = Token{Type: LPAREN, Literal: string(l.ch), Line: line, Column: column}
	case ')':
		tok = Token{Type: RPAREN, Literal: string(l.ch), Line: line, Column: column}
	case '{':
		tok = Token{Type: LBRACE, Literal: string(l.ch), Line: line, Column: column}
	case '}':
		tok = Token{Type: RBRACE, Literal: string(l.ch), Line: line, Column: column}
	case '[':
		tok = Token{Type: LBRACKET, Literal: string(l.ch), Line: line, Column: column}
	case ']':
		tok = Token{Type: RBRACKET, Literal: string(l.ch), Line: line, Column: column}
	case '"':
		tok = Token{Type: STRING, Literal: l.readString('"'), Line: line, Column: column}
	case '\'':
		tok = Token{Type: STRING, Literal: l.readString('\''), Line: line, Column: column}
	case '`':
		tok = Token{Type: STRING, Literal: l.readRawString(), Line: line, Column: column}
	case 0:
		tok = Token{Type: EOF, Literal: "", Line: line, Column: column}
	default:
		if isLetter(l.ch) {
			ident := l.readIdentifier()
			tok = Token{Type: LookupIdent(ident), Literal: ident, Line: line, Column: column}
			return tok
		} else if isDigit(l.ch) {
			num := l.readNumber()
			// Check for BigInt suffix 'n'
			if strings.HasSuffix(num, "n") {
				tok = Token{Type: BIGINT, Literal: num, Line: line, Column: column}
			} else if strings.HasSuffix(num, "m") {
				// BigFloat suffix 'm'
				tok = Token{Type: BIGFLOAT, Literal: num, Line: line, Column: column}
			} else if strings.Contains(num, ".") || strings.Contains(strings.ToLower(num), "e") {
				tok = Token{Type: FLOAT, Literal: num, Line: line, Column: column}
			} else {
				tok = Token{Type: INT, Literal: num, Line: line, Column: column}
			}
			return tok
		} else {
			tok = Token{Type: ILLEGAL, Literal: string(l.ch), Line: line, Column: column}
		}
	}

	l.readChar()
	return tok
}

// readIdentifier reads an identifier or keyword from input.
func (l *Lexer) readIdentifier() string {
	position := l.position
	for isLetter(l.ch) || isDigit(l.ch) {
		l.readChar()
	}
	return l.input[position:l.position]
}

// readNumber reads a numeric literal (integer, float, bigint, or bigfloat) from input.
// Supports decimal notation and scientific notation (e.g., 1.5e10).
// BigInt has suffix 'n', BigFloat has suffix 'm'.
func (l *Lexer) readNumber() string {
	position := l.position

	// Read integer part
	for isDigit(l.ch) {
		l.readChar()
	}

	// Check for decimal point
	if l.ch == '.' && isDigit(l.peekChar()) {
		l.readChar() // consume '.'
		for isDigit(l.ch) {
			l.readChar()
		}
	}

	// Check for scientific notation
	if l.ch == 'e' || l.ch == 'E' {
		l.readChar() // consume 'e' or 'E'
		if l.ch == '+' || l.ch == '-' {
			l.readChar() // consume sign
		}
		for isDigit(l.ch) {
			l.readChar()
		}
	}

	// Check for BigInt suffix 'n' or BigFloat suffix 'm'
	if l.ch == 'n' || l.ch == 'm' {
		l.readChar()
	}

	return l.input[position:l.position]
}

// readString reads a string literal enclosed by the given quote character.
// Supports escape sequences: \n, \t, \r, \\, \", \', \xHH, \uHHHH
func (l *Lexer) readString(quote byte) string {
	var sb strings.Builder

	l.readChar() // skip opening quote

	for l.ch != quote {
		if l.ch == 0 {
			break // EOF reached, unterminated string
		}

		if l.ch == '\\' {
			l.readChar()
			switch l.ch {
			case 'n':
				sb.WriteByte('\n')
			case 't':
				sb.WriteByte('\t')
			case 'r':
				sb.WriteByte('\r')
			case '\\':
				sb.WriteByte('\\')
			case '"':
				sb.WriteByte('"')
			case '\'':
				sb.WriteByte('\'')
			case 'x':
				// \xHH hex escape
				l.readChar()
				hex := l.readHexDigits(2)
				if len(hex) == 2 {
					sb.WriteByte(parseHexByte(hex))
				}
				continue
			case 'u':
				// \uHHHH unicode escape
				l.readChar()
				hex := l.readHexDigits(4)
				if len(hex) == 4 {
					sb.WriteRune(parseHexRune(hex))
				}
				continue
			case 0:
				break
			default:
				sb.WriteByte(l.ch)
			}
		} else {
			sb.WriteByte(l.ch)
		}
		l.readChar()
	}

	return sb.String()
}

// readRawString reads a raw string literal enclosed by backticks.
// Raw strings do not interpret escape sequences and can span multiple lines.
func (l *Lexer) readRawString() string {
	position := l.position + 1 // skip opening backtick

	for {
		l.readChar()
		if l.ch == '`' || l.ch == 0 {
			break
		}
	}

	str := l.input[position:l.position]
	l.readChar() // skip closing backtick
	return str
}

// readHexDigits reads exactly n hexadecimal digits from input.
func (l *Lexer) readHexDigits(n int) string {
	position := l.position
	count := 0
	for count < n && isHexDigit(l.ch) {
		l.readChar()
		count++
	}
	return l.input[position:l.position]
}

// skipWhitespace skips over whitespace characters.
func (l *Lexer) skipWhitespace() {
	for l.ch == ' ' || l.ch == '\t' || l.ch == '\n' || l.ch == '\r' {
		l.readChar()
	}
}

// skipComment skips a single-line comment (// to end of line).
func (l *Lexer) skipComment() {
	for l.ch != '\n' && l.ch != 0 {
		l.readChar()
	}
	l.readChar() // skip the newline
}

// skipBlockComment skips a multi-line comment (/* to */).
func (l *Lexer) skipBlockComment() {
	l.readChar() // skip /
	l.readChar() // skip *

	for {
		if l.ch == '*' && l.peekChar() == '/' {
			l.readChar() // skip *
			l.readChar() // skip /
			break
		}
		if l.ch == 0 {
			break // EOF, unterminated comment
		}
		l.readChar()
	}
}

// peekChar returns the next character without advancing the position.
func (l *Lexer) peekChar() byte {
	if l.readPosition >= len(l.input) {
		return 0
	}
	return l.input[l.readPosition]
}

// PeekToken returns the next token without advancing the position.
// This allows looking ahead one token without consuming it.
func (l *Lexer) PeekToken() Token {
	// Save current state
	pos := l.position
	readPos := l.readPosition
	ch := l.ch
	line := l.line
	col := l.column

	// Read next token
	l.skipWhitespace()
	startLine := l.line
	startCol := l.column

	var tok Token
	switch l.ch {
	case '=':
		if l.peekChar() == '=' {
			ch := l.ch
			l.readChar()
			tok = Token{Type: EQ, Literal: string(ch) + string(l.ch), Line: startLine, Column: startCol}
		} else {
			tok = Token{Type: ASSIGN, Literal: string(l.ch), Line: startLine, Column: startCol}
		}
	case '+':
		if l.peekChar() == '=' {
			l.readChar()
			tok = Token{Type: PLUS_ASSIGN, Literal: "+=", Line: startLine, Column: startCol}
		} else if l.peekChar() == '+' {
			l.readChar()
			tok = Token{Type: INCREMENT, Literal: "++", Line: startLine, Column: startCol}
		} else {
			tok = Token{Type: PLUS, Literal: string(l.ch), Line: startLine, Column: startCol}
		}
	case '-':
		if l.peekChar() == '=' {
			l.readChar()
			tok = Token{Type: MINUS_ASSIGN, Literal: "-=", Line: startLine, Column: startCol}
		} else if l.peekChar() == '-' {
			l.readChar()
			tok = Token{Type: DECREMENT, Literal: "--", Line: startLine, Column: startCol}
		} else {
			tok = Token{Type: MINUS, Literal: string(l.ch), Line: startLine, Column: startCol}
		}
	case '*':
		if l.peekChar() == '=' {
			l.readChar()
			tok = Token{Type: MUL_ASSIGN, Literal: "*=", Line: startLine, Column: startCol}
		} else {
			tok = Token{Type: ASTERISK, Literal: string(l.ch), Line: startLine, Column: startCol}
		}
	case '/':
		if l.peekChar() == '/' {
			tok = Token{Type: SEMICOLON, Literal: "", Line: startLine, Column: startCol} // Skip comment
		} else if l.peekChar() == '*' {
			tok = Token{Type: SEMICOLON, Literal: "", Line: startLine, Column: startCol} // Skip block comment
		} else if l.peekChar() == '=' {
			l.readChar()
			tok = Token{Type: DIV_ASSIGN, Literal: "/=", Line: startLine, Column: startCol}
		} else {
			tok = Token{Type: SLASH, Literal: string(l.ch), Line: startLine, Column: startCol}
		}
	case '%':
		if l.peekChar() == '=' {
			l.readChar()
			tok = Token{Type: MOD_ASSIGN, Literal: "%=", Line: startLine, Column: startCol}
		} else {
			tok = Token{Type: PERCENT, Literal: string(l.ch), Line: startLine, Column: startCol}
		}
	case '!':
		if l.peekChar() == '=' {
			ch := l.ch
			l.readChar()
			tok = Token{Type: NOT_EQ, Literal: string(ch) + string(l.ch), Line: startLine, Column: startCol}
		} else {
			tok = Token{Type: BANG, Literal: string(l.ch), Line: startLine, Column: startCol}
		}
	case '<':
		if l.peekChar() == '=' {
			l.readChar()
			tok = Token{Type: LT_EQ, Literal: "<=", Line: startLine, Column: startCol}
		} else if l.peekChar() == '<' {
			l.readChar()
			tok = Token{Type: SHL, Literal: "<<", Line: startLine, Column: startCol}
		} else {
			tok = Token{Type: LT, Literal: string(l.ch), Line: startLine, Column: startCol}
		}
	case '>':
		if l.peekChar() == '=' {
			l.readChar()
			tok = Token{Type: GT_EQ, Literal: ">=", Line: startLine, Column: startCol}
		} else if l.peekChar() == '>' {
			l.readChar()
			tok = Token{Type: SHR, Literal: ">>", Line: startLine, Column: startCol}
		} else {
			tok = Token{Type: GT, Literal: string(l.ch), Line: startLine, Column: startCol}
		}
	case '&':
		if l.peekChar() == '&' {
			l.readChar()
			tok = Token{Type: AND, Literal: "&&", Line: startLine, Column: startCol}
		} else {
			tok = Token{Type: BIT_AND, Literal: string(l.ch), Line: startLine, Column: startCol}
		}
	case '|':
		if l.peekChar() == '|' {
			l.readChar()
			tok = Token{Type: OR, Literal: "||", Line: startLine, Column: startCol}
		} else {
			tok = Token{Type: BIT_OR, Literal: string(l.ch), Line: startLine, Column: startCol}
		}
	case '^':
		tok = Token{Type: BIT_XOR, Literal: string(l.ch), Line: startLine, Column: startCol}
	case '~':
		tok = Token{Type: BIT_NOT, Literal: string(l.ch), Line: startLine, Column: startCol}
	case ',':
		tok = Token{Type: COMMA, Literal: string(l.ch), Line: startLine, Column: startCol}
	case ':':
		if l.peekChar() == '=' {
			l.readChar()
			tok = Token{Type: DEFINE, Literal: ":=", Line: startLine, Column: startCol}
		} else {
			tok = Token{Type: COLON, Literal: string(l.ch), Line: startLine, Column: startCol}
		}
	case ';':
		tok = Token{Type: SEMICOLON, Literal: string(l.ch), Line: startLine, Column: startCol}
	case '.':
		tok = Token{Type: DOT, Literal: string(l.ch), Line: startLine, Column: startCol}
	case '(':
		tok = Token{Type: LPAREN, Literal: string(l.ch), Line: startLine, Column: startCol}
	case ')':
		tok = Token{Type: RPAREN, Literal: string(l.ch), Line: startLine, Column: startCol}
	case '{':
		tok = Token{Type: LBRACE, Literal: string(l.ch), Line: startLine, Column: startCol}
	case '}':
		tok = Token{Type: RBRACE, Literal: string(l.ch), Line: startLine, Column: startCol}
	case '[':
		tok = Token{Type: LBRACKET, Literal: string(l.ch), Line: startLine, Column: startCol}
	case ']':
		tok = Token{Type: RBRACKET, Literal: string(l.ch), Line: startLine, Column: startCol}
	case '"':
		tok = Token{Type: STRING, Literal: l.readString('"'), Line: startLine, Column: startCol}
	case '\'':
		tok = Token{Type: STRING, Literal: l.readString('\''), Line: startLine, Column: startCol}
	case '`':
		tok = Token{Type: STRING, Literal: l.readRawString(), Line: startLine, Column: startCol}
	case 0:
		tok = Token{Type: EOF, Literal: "", Line: startLine, Column: startCol}
	default:
		if isLetter(l.ch) {
			ident := l.readIdentifier()
			tok = Token{Type: LookupIdent(ident), Literal: ident, Line: startLine, Column: startCol}
		} else if isDigit(l.ch) {
			num := l.readNumber()
			// Check for BigInt suffix 'n'
			if strings.HasSuffix(num, "n") {
				tok = Token{Type: BIGINT, Literal: num, Line: startLine, Column: startCol}
			} else if strings.HasSuffix(num, "m") {
				// BigFloat suffix 'm'
				tok = Token{Type: BIGFLOAT, Literal: num, Line: startLine, Column: startCol}
			} else if strings.Contains(num, ".") || strings.Contains(strings.ToLower(num), "e") {
				tok = Token{Type: FLOAT, Literal: num, Line: startLine, Column: startCol}
			} else {
				tok = Token{Type: INT, Literal: num, Line: startLine, Column: startCol}
			}
		} else {
			tok = Token{Type: ILLEGAL, Literal: string(l.ch), Line: startLine, Column: startCol}
		}
	}

	// Restore state
	l.position = pos
	l.readPosition = readPos
	l.ch = ch
	l.line = line
	l.column = col

	return tok
}

// isLetter returns true if the character is a letter or underscore.
func isLetter(ch byte) bool {
	return 'a' <= ch && ch <= 'z' || 'A' <= ch && ch <= 'Z' || ch == '_'
}

// isDigit returns true if the character is a decimal digit.
func isDigit(ch byte) bool {
	return '0' <= ch && ch <= '9'
}

// isHexDigit returns true if the character is a hexadecimal digit.
func isHexDigit(ch byte) bool {
	return '0' <= ch && ch <= '9' || 'a' <= ch && ch <= 'f' || 'A' <= ch && ch <= 'F'
}

// parseHexByte parses a 2-digit hex string to a byte.
func parseHexByte(hex string) byte {
	var result byte
	for _, ch := range hex {
		result <<= 4
		switch {
		case '0' <= ch && ch <= '9':
			result |= byte(ch - '0')
		case 'a' <= ch && ch <= 'f':
			result |= byte(ch-'a') + 10
		case 'A' <= ch && ch <= 'F':
			result |= byte(ch-'A') + 10
		}
	}
	return result
}

// parseHexRune parses a 4-digit hex string to a rune.
func parseHexRune(hex string) rune {
	var result rune
	for _, ch := range hex {
		result <<= 4
		switch {
		case '0' <= ch && ch <= '9':
			result |= rune(ch - '0')
		case 'a' <= ch && ch <= 'f':
			result |= rune(ch-'a') + 10
		case 'A' <= ch && ch <= 'F':
			result |= rune(ch-'A') + 10
		}
	}
	return result
}
