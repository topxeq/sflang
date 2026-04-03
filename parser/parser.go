// Package parser implements syntax analysis for Sflang source code.
// It converts a stream of tokens into an Abstract Syntax Tree (AST).
package parser

import (
	"fmt"
	"strings"

	"github.com/topxeq/sflang/ast"
	"github.com/topxeq/sflang/lexer"
)

// Parser holds the state of the parsing process.
type Parser struct {
	lexer        *lexer.Lexer
	currentToken lexer.Token
	peekToken    lexer.Token
	errors       []string

	// Pratt parser maps
	prefixParseFns map[lexer.TokenType]prefixParseFn
	infixParseFns  map[lexer.TokenType]infixParseFn
}

// Operator precedence levels (higher number = higher precedence).
const (
	_ int = iota
	LOWEST
	OR          // ||
	AND         // &&
	EQUALS      // ==, !=
	LESSGREATER // <, >, <=, >=
	SHIFT       // <<, >>
	BIT_OR      // |
	BIT_XOR     // ^
	BIT_AND     // &
	SUM         // +, -
	PRODUCT     // *, /, %
	PREFIX      // -X, !X, ~X
	CALL        // myFunction(X)
	INDEX       // array[index]
)

// Precedence map for token types.
var precedences = map[lexer.TokenType]int{
	lexer.OR:          OR,
	lexer.AND:         AND,
	lexer.EQ:          EQUALS,
	lexer.NOT_EQ:      EQUALS,
	lexer.LT:          LESSGREATER,
	lexer.GT:          LESSGREATER,
	lexer.LT_EQ:       LESSGREATER,
	lexer.GT_EQ:       LESSGREATER,
	lexer.SHL:         SHIFT,
	lexer.SHR:         SHIFT,
	lexer.BIT_OR:      BIT_OR,
	lexer.BIT_XOR:     BIT_XOR,
	lexer.BIT_AND:     BIT_AND,
	lexer.PLUS:        SUM,
	lexer.MINUS:       SUM,
	lexer.ASTERISK:    PRODUCT,
	lexer.SLASH:       PRODUCT,
	lexer.PERCENT:     PRODUCT,
	lexer.LPAREN:      CALL,
	lexer.LBRACKET:    INDEX,
		lexer.DOT:         CALL, // member access
	lexer.INCREMENT:   CALL, // postfix ++
	lexer.DECREMENT:   CALL, // postfix --
}

// Parsing function types for Pratt parsing.
type (
	prefixParseFn func() ast.Expression
	infixParseFn  func(ast.Expression) ast.Expression
)

// New creates a new Parser instance for the given lexer.
func New(l *lexer.Lexer) *Parser {
	p := &Parser{
		lexer:        l,
		errors:       []string{},
		prefixParseFns: make(map[lexer.TokenType]prefixParseFn),
		infixParseFns:  make(map[lexer.TokenType]infixParseFn),
	}

	// Register prefix parse functions
	p.registerPrefix(lexer.IDENT, p.parseIdentifier)
	p.registerPrefix(lexer.INT, p.parseIntegerLiteral)
	p.registerPrefix(lexer.FLOAT, p.parseFloatLiteral)
	p.registerPrefix(lexer.BIGINT, p.parseBigIntLiteral)
	p.registerPrefix(lexer.BIGFLOAT, p.parseBigFloatLiteral)
	p.registerPrefix(lexer.STRING, p.parseStringLiteral)
	p.registerPrefix(lexer.TRUE, p.parseBooleanLiteral)
	p.registerPrefix(lexer.FALSE, p.parseBooleanLiteral)
	p.registerPrefix(lexer.NULL, p.parseNullLiteral)
	p.registerPrefix(lexer.BANG, p.parsePrefixExpression)
	p.registerPrefix(lexer.MINUS, p.parsePrefixExpression)
	p.registerPrefix(lexer.BIT_NOT, p.parsePrefixExpression)
	p.registerPrefix(lexer.LPAREN, p.parseGroupedExpression)
	p.registerPrefix(lexer.LBRACKET, p.parseArrayLiteral)
	p.registerPrefix(lexer.LBRACE, p.parseMapLiteral)
	p.registerPrefix(lexer.IF, p.parseIfExpression)
	p.registerPrefix(lexer.FN, p.parseFunctionLiteral)

	// Register infix parse functions
	p.registerInfix(lexer.PLUS, p.parseInfixExpression)
	p.registerInfix(lexer.MINUS, p.parseInfixExpression)
	p.registerInfix(lexer.ASTERISK, p.parseInfixExpression)
	p.registerInfix(lexer.SLASH, p.parseInfixExpression)
	p.registerInfix(lexer.PERCENT, p.parseInfixExpression)
	p.registerInfix(lexer.EQ, p.parseInfixExpression)
	p.registerInfix(lexer.NOT_EQ, p.parseInfixExpression)
	p.registerInfix(lexer.LT, p.parseInfixExpression)
	p.registerInfix(lexer.GT, p.parseInfixExpression)
	p.registerInfix(lexer.LT_EQ, p.parseInfixExpression)
	p.registerInfix(lexer.GT_EQ, p.parseInfixExpression)
	p.registerInfix(lexer.AND, p.parseInfixExpression)
	p.registerInfix(lexer.OR, p.parseInfixExpression)
	p.registerInfix(lexer.BIT_AND, p.parseInfixExpression)
	p.registerInfix(lexer.BIT_OR, p.parseInfixExpression)
	p.registerInfix(lexer.BIT_XOR, p.parseInfixExpression)
	p.registerInfix(lexer.SHL, p.parseInfixExpression)
	p.registerInfix(lexer.SHR, p.parseInfixExpression)
	p.registerInfix(lexer.LPAREN, p.parseCallExpression)
	p.registerInfix(lexer.LBRACKET, p.parseIndexExpression)
	p.registerInfix(lexer.DOT, p.parseDotExpression)
	p.registerInfix(lexer.INCREMENT, p.parsePostfixExpression)
	p.registerInfix(lexer.DECREMENT, p.parsePostfixExpression)

	// Read two tokens to initialize current and peek
	p.nextToken()
	p.nextToken()

	return p
}

// registerPrefix registers a prefix parse function for a token type.
func (p *Parser) registerPrefix(tokenType lexer.TokenType, fn prefixParseFn) {
	p.prefixParseFns[tokenType] = fn
}

// registerInfix registers an infix parse function for a token type.
func (p *Parser) registerInfix(tokenType lexer.TokenType, fn infixParseFn) {
	p.infixParseFns[tokenType] = fn
}

// nextToken advances to the next token.
func (p *Parser) nextToken() {
	p.currentToken = p.peekToken
	p.peekToken = p.lexer.NextToken()
}

// ParseProgram parses the entire program and returns the AST.
func (p *Parser) ParseProgram() *ast.Program {
	program := &ast.Program{Statements: []ast.Statement{}}

	for p.currentToken.Type != lexer.EOF {
		stmt := p.parseStatement()
		if stmt != nil {
			program.Statements = append(program.Statements, stmt)
		}
		p.nextToken()
	}

	return program
}

// parseStatement parses a statement based on the current token.
func (p *Parser) parseStatement() ast.Statement {
	switch p.currentToken.Type {
	case lexer.LET:
		return p.parseLetStatement()
	case lexer.VAR:
		return p.parseVarStatement()
	case lexer.RETURN:
		return p.parseReturnStatement()
	case lexer.FOR:
		return p.parseForStatement()
	case lexer.BREAK:
		return p.parseBreakStatement()
	case lexer.CONTINUE:
		return p.parseContinueStatement()
	case lexer.TRY:
		return p.parseTryStatement()
	case lexer.THROW:
		return p.parseThrowStatement()
	default:
		return p.parseExpressionStatement()
	}
}

// parseLetStatement parses a let statement.
// Syntax: let <identifier> = <expression>;
func (p *Parser) parseLetStatement() *ast.LetStatement {
	stmt := &ast.LetStatement{Token: p.currentToken}

	if !p.expectPeek(lexer.IDENT) {
		return nil
	}

	stmt.Name = &ast.Identifier{Token: p.currentToken, Value: p.currentToken.Literal}

	if !p.expectPeek(lexer.ASSIGN) {
		return nil
	}

	p.nextToken()
	stmt.Value = p.parseExpression(LOWEST)

	if p.peekTokenIs(lexer.SEMICOLON) {
		p.nextToken()
	}

	return stmt
}

// parseVarStatement parses a var statement.
// Syntax: var <identifier>; or var <identifier> = <expression>;
func (p *Parser) parseVarStatement() *ast.LetStatement {
	stmt := &ast.LetStatement{Token: p.currentToken}

	if !p.expectPeek(lexer.IDENT) {
		return nil
	}

	stmt.Name = &ast.Identifier{Token: p.currentToken, Value: p.currentToken.Literal}

	// Check if there's an assignment
	if p.peekTokenIs(lexer.ASSIGN) {
		p.nextToken() // move to =
		p.nextToken() // move past =
		stmt.Value = p.parseExpression(LOWEST)
	} else {
		// No initialization, set to null
		stmt.Value = &ast.NullLiteral{Token: lexer.Token{Type: lexer.NULL, Literal: "null"}}
	}

	if p.peekTokenIs(lexer.SEMICOLON) {
		p.nextToken()
	}

	return stmt
}

// parseDefineStatement parses a short variable declaration statement.
// Syntax: <identifier> := <expression>;
func (p *Parser) parseDefineStatement() *ast.LetStatement {
	stmt := &ast.LetStatement{Token: p.currentToken}

	stmt.Name = &ast.Identifier{Token: p.currentToken, Value: p.currentToken.Literal}

	p.nextToken() // move to :=
	p.nextToken() // move past :=

	stmt.Value = p.parseExpression(LOWEST)

	if p.peekTokenIs(lexer.SEMICOLON) {
		p.nextToken()
	}

	return stmt
}

// parseReturnStatement parses a return statement.
// Syntax: return <expression>;
func (p *Parser) parseReturnStatement() *ast.ReturnStatement {
	stmt := &ast.ReturnStatement{Token: p.currentToken}

	p.nextToken()
	stmt.ReturnValue = p.parseExpression(LOWEST)

	if p.peekTokenIs(lexer.SEMICOLON) {
		p.nextToken()
	}

	return stmt
}

// parseExpressionStatement parses an expression statement.
func (p *Parser) parseExpressionStatement() ast.Statement {
	// Check for short variable declaration (identifier := value)
	if p.currentTokenIs(lexer.IDENT) && p.peekTokenIs(lexer.DEFINE) {
		return p.parseDefineStatement()
	}

	// Check for simple assignment (identifier = value)
	if p.peekTokenIs(lexer.ASSIGN) {
		return p.parseAssignmentStatement()
	}

	// Check for compound assignment (identifier += value, etc.)
	if p.isCompoundAssignToken(p.peekToken.Type) {
		return p.parseCompoundAssignStatement()
	}

	stmt := &ast.ExpressionStatement{Token: p.currentToken}
	stmt.Expression = p.parseExpression(LOWEST)

	// Check for index assignment (a[index] = value) after parsing expression
	if p.peekTokenIs(lexer.ASSIGN) {
		// Check if the expression is an index expression or identifier
		if _, ok := stmt.Expression.(*ast.IndexExpression); ok {
			return p.parseIndexAssignmentStatement(stmt.Expression)
		}
	}

	// Check for compound index assignment (a[index] += value)
	if p.isCompoundAssignToken(p.peekToken.Type) {
		if _, ok := stmt.Expression.(*ast.IndexExpression); ok {
			return p.parseCompoundIndexAssignStatement(stmt.Expression)
		}
	}

	if p.peekTokenIs(lexer.SEMICOLON) {
		p.nextToken()
	}

	return stmt
}

// isCompoundAssignToken returns true if the token is a compound assignment operator.
func (p *Parser) isCompoundAssignToken(t lexer.TokenType) bool {
	return t == lexer.PLUS_ASSIGN || t == lexer.MINUS_ASSIGN ||
		t == lexer.MUL_ASSIGN || t == lexer.DIV_ASSIGN || t == lexer.MOD_ASSIGN
}

// parseAssignmentStatement parses an assignment statement.
func (p *Parser) parseAssignmentStatement() *ast.AssignStatement {
	stmt := &ast.AssignStatement{Token: p.peekToken}

	// The left side is the current token (should be an identifier or index expression)
	stmt.Left = p.parseExpression(LOWEST)

	if !p.expectPeek(lexer.ASSIGN) {
		return nil
	}

	p.nextToken()
	stmt.Right = p.parseExpression(LOWEST)

	if p.peekTokenIs(lexer.SEMICOLON) {
		p.nextToken()
	}

	return stmt
}

// parseCompoundAssignStatement parses a compound assignment statement.
func (p *Parser) parseCompoundAssignStatement() *ast.CompoundAssignStatement {
	stmt := &ast.CompoundAssignStatement{Token: p.peekToken}

	stmt.Left = p.parseExpression(LOWEST)

	stmt.Operator = p.peekToken.Literal
	p.nextToken() // move to the compound operator

	p.nextToken() // move past the operator
	stmt.Right = p.parseExpression(LOWEST)

	if p.peekTokenIs(lexer.SEMICOLON) {
		p.nextToken()
	}

	return stmt
}

// parseIndexAssignmentStatement parses an index assignment statement.
// Example: a[0] = 10, m["key"] = "value"
func (p *Parser) parseIndexAssignmentStatement(left ast.Expression) *ast.AssignStatement {
	stmt := &ast.AssignStatement{Token: p.peekToken}

	stmt.Left = left

	p.nextToken() // move to =
	p.nextToken() // move past =

	stmt.Right = p.parseExpression(LOWEST)

	if p.peekTokenIs(lexer.SEMICOLON) {
		p.nextToken()
	}

	return stmt
}

// parseCompoundIndexAssignStatement parses a compound index assignment statement.
// Example: a[0] += 10, m["key"] *= 2
func (p *Parser) parseCompoundIndexAssignStatement(left ast.Expression) *ast.CompoundAssignStatement {
	stmt := &ast.CompoundAssignStatement{Token: p.peekToken}

	stmt.Left = left

	stmt.Operator = p.peekToken.Literal
	p.nextToken() // move to the compound operator

	p.nextToken() // move past the operator
	stmt.Right = p.parseExpression(LOWEST)

	if p.peekTokenIs(lexer.SEMICOLON) {
		p.nextToken()
	}

	return stmt
}

// parseBlockStatement parses a block statement.
func (p *Parser) parseBlockStatement() *ast.BlockStatement {
	block := &ast.BlockStatement{Token: p.currentToken}
	block.Statements = []ast.Statement{}

	p.nextToken()

	for !p.currentTokenIs(lexer.RBRACE) && !p.currentTokenIs(lexer.EOF) {
		stmt := p.parseStatement()
		if stmt != nil {
			block.Statements = append(block.Statements, stmt)
		}
		p.nextToken()
	}

	return block
}

// parseForStatement parses a for loop statement.
// Syntax: for (<init>; <condition>; <update>) { <body> }
// or: for (<key>, <value> in <source>) { <body> }
// Supports empty sections: for (;;) is an infinite loop
func (p *Parser) parseForStatement() ast.Statement {
	stmt := &ast.ForStatement{Token: p.currentToken}

	if !p.expectPeek(lexer.LPAREN) {
		return nil
	}

	// Peek ahead to check for for-in syntax
	// We look at the token after the next identifier
	nextTok := p.lexer.PeekToken()
	if nextTok.Type == lexer.COMMA || nextTok.Type == lexer.IN {
		// This is for-in syntax
		p.nextToken() // move to first identifier
		firstIdent := &ast.Identifier{Token: p.currentToken, Value: p.currentToken.Literal}

		if p.peekTokenIs(lexer.COMMA) {
			// for (k, v in obj)
			p.nextToken()
			if !p.expectPeek(lexer.IDENT) {
				return nil
			}
			secondIdent := &ast.Identifier{Token: p.currentToken, Value: p.currentToken.Literal}

			if !p.expectPeek(lexer.IN) {
				return nil
			}

			p.nextToken()
			source := p.parseExpression(LOWEST)

			if !p.expectPeek(lexer.RPAREN) {
				return nil
			}

			if !p.expectPeek(lexer.LBRACE) {
				return nil
			}

			return &ast.ForInStatement{
				Token:  stmt.Token,
				Key:    firstIdent,
				Value:  secondIdent,
				Source: source,
				Body:   p.parseBlockStatement(),
			}
		} else if p.peekTokenIs(lexer.IN) {
			// for (k in obj)
			p.nextToken()
			p.nextToken()
			source := p.parseExpression(LOWEST)

			if !p.expectPeek(lexer.RPAREN) {
				return nil
			}

			if !p.expectPeek(lexer.LBRACE) {
				return nil
			}

			return &ast.ForInStatement{
				Token:  stmt.Token,
				Key:    firstIdent,
				Source: source,
				Body:   p.parseBlockStatement(),
			}
		}
	}

	// Regular for loop: for (init; condition; update) { body }
	// currentToken is LPAREN, peekToken is the first token of init
	p.nextToken()

	// Parse init (empty if current token is semicolon)
	if !p.currentTokenIs(lexer.SEMICOLON) {
		stmt.Init = p.parseStatement()
	}

	// Move past the first semicolon (init/condition separator)
	if p.currentTokenIs(lexer.SEMICOLON) {
		p.nextToken()
	} else if p.peekTokenIs(lexer.SEMICOLON) {
		p.nextToken()
		p.nextToken()
	} else {
		p.peekError(lexer.SEMICOLON)
		return nil
	}

	// Parse condition (empty if current token is semicolon - infinite loop)
	if !p.currentTokenIs(lexer.SEMICOLON) {
		stmt.Condition = p.parseExpression(LOWEST)
	}

	// Move past the second semicolon (condition/update separator)
	if p.currentTokenIs(lexer.SEMICOLON) {
		// Condition was empty, already at semicolon, move to next
		p.nextToken()
	} else if p.peekTokenIs(lexer.SEMICOLON) {
		// Condition was present, expect semicolon and move past it
		p.nextToken()
		p.nextToken()
	} else {
		p.peekError(lexer.SEMICOLON)
		return nil
	}

	// Parse update (empty if current token is right paren)
	if !p.currentTokenIs(lexer.RPAREN) {
		stmt.Update = p.parseStatement()
	}

	// After update, handle the closing parenthesis
	// currentToken could be at various positions depending on what was parsed
	if p.currentTokenIs(lexer.RPAREN) {
		// Update was empty or ended at RPAREN
		p.nextToken()
	} else if p.peekTokenIs(lexer.RPAREN) {
		// Update ended with peekToken at RPAREN
		p.nextToken()
		p.nextToken()
	} else if p.currentTokenIs(lexer.SEMICOLON) {
		// Update ended with semicolon (shouldn't happen for empty update, but handle it)
		p.nextToken()
		if p.currentTokenIs(lexer.RPAREN) {
			p.nextToken()
		} else if p.peekTokenIs(lexer.RPAREN) {
			p.nextToken()
			p.nextToken()
		} else {
			p.peekError(lexer.RPAREN)
			return nil
		}
	} else {
		// Neither currentToken nor peekToken is RPAREN - this is an error
		p.peekError(lexer.RPAREN)
		return nil
	}

	if p.currentTokenIs(lexer.LBRACE) {
		stmt.Body = p.parseBlockStatement()
		return stmt
	}

	if !p.expectPeek(lexer.LBRACE) {
		return nil
	}

	stmt.Body = p.parseBlockStatement()

	return stmt
}

// parseBreakStatement parses a break statement.
func (p *Parser) parseBreakStatement() *ast.BreakStatement {
	stmt := &ast.BreakStatement{Token: p.currentToken}

	if p.peekTokenIs(lexer.SEMICOLON) {
		p.nextToken()
	}

	return stmt
}

// parseContinueStatement parses a continue statement.
func (p *Parser) parseContinueStatement() *ast.ContinueStatement {
	stmt := &ast.ContinueStatement{Token: p.currentToken}

	if p.peekTokenIs(lexer.SEMICOLON) {
		p.nextToken()
	}

	return stmt
}

// parseTryStatement parses a try-catch-finally statement.
// Syntax: try { <body> } catch (<var>) { <catchBody> } [finally { <finallyBody> }]
func (p *Parser) parseTryStatement() *ast.TryStatement {
	stmt := &ast.TryStatement{Token: p.currentToken}

	if !p.expectPeek(lexer.LBRACE) {
		return nil
	}

	stmt.Body = p.parseBlockStatement()

	// Catch is optional if finally is present
	if p.peekTokenIs(lexer.CATCH) {
		p.nextToken() // move to catch

		if !p.expectPeek(lexer.LPAREN) {
			return nil
		}

		if !p.expectPeek(lexer.IDENT) {
			return nil
		}

		stmt.CatchVar = &ast.Identifier{Token: p.currentToken, Value: p.currentToken.Literal}

		if !p.expectPeek(lexer.RPAREN) {
			return nil
		}

		if !p.expectPeek(lexer.LBRACE) {
			return nil
		}

		stmt.CatchBody = p.parseBlockStatement()
	}

	// Check for finally block
	if p.peekTokenIs(lexer.FINALLY) {
		p.nextToken() // move to finally

		if !p.expectPeek(lexer.LBRACE) {
			return nil
		}

		stmt.FinallyBody = p.parseBlockStatement()
	}

	// Must have at least catch or finally
	if stmt.CatchBody == nil && stmt.FinallyBody == nil {
		p.errors = append(p.errors, "try statement must have catch or finally block")
		return nil
	}

	return stmt
}

// parseThrowStatement parses a throw statement.
// Syntax: throw <expression>;
func (p *Parser) parseThrowStatement() *ast.ThrowStatement {
	stmt := &ast.ThrowStatement{Token: p.currentToken}

	p.nextToken()
	stmt.ErrExpr = p.parseExpression(LOWEST)

	if p.peekTokenIs(lexer.SEMICOLON) {
		p.nextToken()
	}

	return stmt
}

// parseExpression parses an expression using Pratt parsing.
func (p *Parser) parseExpression(precedence int) ast.Expression {
	prefix := p.prefixParseFns[p.currentToken.Type]
	if prefix == nil {
		p.noPrefixParseFnError(p.currentToken.Type)
		return nil
	}
	leftExp := prefix()

	for !p.peekTokenIs(lexer.SEMICOLON) && precedence < p.peekPrecedence() {
		infix := p.infixParseFns[p.peekToken.Type]
		if infix == nil {
			return leftExp
		}

		p.nextToken()

		leftExp = infix(leftExp)
	}

	return leftExp
}

// parseIdentifier parses an identifier expression.
func (p *Parser) parseIdentifier() ast.Expression {
	return &ast.Identifier{Token: p.currentToken, Value: p.currentToken.Literal}
}

// parseIntegerLiteral parses an integer literal expression.
func (p *Parser) parseIntegerLiteral() ast.Expression {
	lit := &ast.IntegerLiteral{Token: p.currentToken}

	var value int64
	for i, ch := range p.currentToken.Literal {
		if ch >= '0' && ch <= '9' {
			value = value*10 + int64(ch-'0')
		} else {
			p.errors = append(p.errors, fmt.Sprintf("could not parse %q as integer", p.currentToken.Literal))
			return nil
		}
		if i > 18 { // Prevent overflow
			break
		}
	}

	lit.Value = value
	return lit
}

// parseFloatLiteral parses a float literal expression.
func (p *Parser) parseFloatLiteral() ast.Expression {
	lit := &ast.FloatLiteral{Token: p.currentToken}

	var value float64
	var divisor float64 = 1.0
	var afterDecimal bool
	var expSign float64 = 1.0
	var expValue float64
	var inExponent bool

	for _, ch := range p.currentToken.Literal {
		if ch >= '0' && ch <= '9' {
			if inExponent {
				expValue = expValue*10 + float64(ch-'0')
			} else if afterDecimal {
				value = value*10 + float64(ch-'0')
				divisor *= 10
			} else {
				value = value*10 + float64(ch-'0')
			}
		} else if ch == '.' {
			afterDecimal = true
		} else if ch == 'e' || ch == 'E' {
			inExponent = true
		} else if ch == '-' && inExponent {
			expSign = -1
		} else if ch == '+' && inExponent {
			// Skip
		}
	}

	lit.Value = value / divisor
	if inExponent {
		expValue *= expSign
		for i := 0; i < int(expValue); i++ {
			lit.Value *= 10
		}
		for i := 0; i > int(expValue); i-- {
			lit.Value /= 10
		}
	}

	return lit
}

// parseBigIntLiteral parses a big integer literal expression.
// BigInt literals have the suffix 'n' (e.g., 123456789012345678901234567890n).
func (p *Parser) parseBigIntLiteral() ast.Expression {
	lit := &ast.BigIntLiteral{Token: p.currentToken}

	// Remove the 'n' suffix and store the value as string
	value := strings.TrimSuffix(p.currentToken.Literal, "n")
	lit.Value = value

	return lit
}

// parseBigFloatLiteral parses a big float literal expression.
// BigFloat literals have the suffix 'm' (e.g., 3.141592653589793238462643383279m).
func (p *Parser) parseBigFloatLiteral() ast.Expression {
	lit := &ast.BigFloatLiteral{Token: p.currentToken}

	// Remove the 'm' suffix and store the value as string
	value := strings.TrimSuffix(p.currentToken.Literal, "m")
	lit.Value = value

	return lit
}

// parseStringLiteral parses a string literal expression.
func (p *Parser) parseStringLiteral() ast.Expression {
	return &ast.StringLiteral{Token: p.currentToken, Value: p.currentToken.Literal}
}

// parseBooleanLiteral parses a boolean literal expression.
func (p *Parser) parseBooleanLiteral() ast.Expression {
	return &ast.BooleanLiteral{Token: p.currentToken, Value: p.currentTokenIs(lexer.TRUE)}
}

// parseNullLiteral parses a null literal expression.
func (p *Parser) parseNullLiteral() ast.Expression {
	return &ast.NullLiteral{Token: p.currentToken}
}

// parsePrefixExpression parses a prefix expression.
func (p *Parser) parsePrefixExpression() ast.Expression {
	expression := &ast.PrefixExpression{
		Token:    p.currentToken,
		Operator: p.currentToken.Literal,
	}

	p.nextToken()
	expression.Right = p.parseExpression(PREFIX)

	return expression
}

// parseInfixExpression parses an infix expression.
func (p *Parser) parseInfixExpression(left ast.Expression) ast.Expression {
	expression := &ast.InfixExpression{
		Token:    p.currentToken,
		Left:     left,
		Operator: p.currentToken.Literal,
	}

	precedence := p.currentPrecedence()
	p.nextToken()
	expression.Right = p.parseExpression(precedence)

	return expression
}

// parsePostfixExpression parses a postfix expression (++, --).
func (p *Parser) parsePostfixExpression(left ast.Expression) ast.Expression {
	return &ast.PostfixExpression{
		Token:    p.currentToken,
		Left:     left,
		Operator: p.currentToken.Literal,
	}
}

// parseGroupedExpression parses a grouped expression (inside parentheses).
func (p *Parser) parseGroupedExpression() ast.Expression {
	p.nextToken()

	exp := p.parseExpression(LOWEST)

	if !p.expectPeek(lexer.RPAREN) {
		return nil
	}

	return exp
}

// parseArrayLiteral parses an array literal expression.
func (p *Parser) parseArrayLiteral() ast.Expression {
	array := &ast.ArrayLiteral{Token: p.currentToken}
	array.Elements = p.parseExpressionList(lexer.RBRACKET)
	return array
}

// parseMapLiteral parses a map literal expression.
func (p *Parser) parseMapLiteral() ast.Expression {
	mapLit := &ast.MapLiteral{Token: p.currentToken}
	mapLit.Pairs = make(map[ast.Expression]ast.Expression)

	for !p.peekTokenIs(lexer.RBRACE) {
		p.nextToken()
		key := p.parseExpression(LOWEST)

		if !p.expectPeek(lexer.COLON) {
			return nil
		}

		p.nextToken()
		value := p.parseExpression(LOWEST)

		mapLit.Pairs[key] = value

		if !p.peekTokenIs(lexer.RBRACE) && !p.expectPeek(lexer.COMMA) {
			return nil
		}
	}

	if !p.expectPeek(lexer.RBRACE) {
		return nil
	}

	return mapLit
}

// parseExpressionList parses a comma-separated list of expressions.
func (p *Parser) parseExpressionList(end lexer.TokenType) []ast.Expression {
	var list []ast.Expression

	if p.peekTokenIs(end) {
		p.nextToken()
		return list
	}

	p.nextToken()
	list = append(list, p.parseExpression(LOWEST))

	for p.peekTokenIs(lexer.COMMA) {
		p.nextToken()
		p.nextToken()
		list = append(list, p.parseExpression(LOWEST))
	}

	if !p.expectPeek(end) {
		return nil
	}

	return list
}

// parseCallExpression parses a function call expression.
func (p *Parser) parseCallExpression(function ast.Expression) ast.Expression {
	exp := &ast.CallExpression{Token: p.currentToken, Function: function}
	exp.Arguments = p.parseExpressionList(lexer.RPAREN)
	return exp
}

// parseIndexExpression parses an index or slice expression.
// Index: a[0], a["key"]
// Slice: a[1:3], a[:3], a[1:], a[:]
func (p *Parser) parseIndexExpression(left ast.Expression) ast.Expression {
	token := p.currentToken

	p.nextToken()

	// Check for slice expression starting with :
	if p.currentTokenIs(lexer.COLON) {
		// Slice with no start: [:end]
		p.nextToken() // move past :
		slice := &ast.SliceExpression{Token: token, Left: left}

		if !p.currentTokenIs(lexer.RBRACKET) {
			slice.End = p.parseExpression(LOWEST)
		}

		// Consume the closing bracket
		if p.currentTokenIs(lexer.RBRACKET) {
			return slice
		}
		if !p.expectPeek(lexer.RBRACKET) {
			return nil
		}

		return slice
	}

	// Parse the index/start
	index := p.parseExpression(LOWEST)

	// Check for slice expression with start
	if p.peekTokenIs(lexer.COLON) {
		p.nextToken() // move to :
		p.nextToken() // move past :

		slice := &ast.SliceExpression{Token: token, Left: left, Start: index}

		if !p.currentTokenIs(lexer.RBRACKET) {
			slice.End = p.parseExpression(LOWEST)
		}

		// Consume the closing bracket
		if p.currentTokenIs(lexer.RBRACKET) {
			return slice
		}
		if !p.expectPeek(lexer.RBRACKET) {
			return nil
		}

		return slice
	}

	// Regular index expression
	exp := &ast.IndexExpression{Token: token, Left: left, Index: index}

	if !p.expectPeek(lexer.RBRACKET) {
		return nil
	}

	return exp
}

// parseDotExpression parses a dot expression for map member access.
// Example: a.key is transformed to a["key"]
func (p *Parser) parseDotExpression(left ast.Expression) ast.Expression {
	// Create an IndexExpression with the key as a string literal
	exp := &ast.IndexExpression{Token: p.currentToken, Left: left}

	p.nextToken()

	// The identifier after the dot becomes the string key
	if !p.currentTokenIs(lexer.IDENT) {
		p.errors = append(p.errors, fmt.Sprintf("expected identifier after '.', got %s", p.currentToken.Type))
		return nil
	}

	exp.Index = &ast.StringLiteral{Token: p.currentToken, Value: p.currentToken.Literal}

	return exp
}

// parseIfExpression parses an if expression.
func (p *Parser) parseIfExpression() ast.Expression {
	expression := &ast.IfExpression{Token: p.currentToken}

	if !p.expectPeek(lexer.LPAREN) {
		return nil
	}

	p.nextToken()
	expression.Condition = p.parseExpression(LOWEST)

	if !p.expectPeek(lexer.RPAREN) {
		return nil
	}

	if !p.expectPeek(lexer.LBRACE) {
		return nil
	}

	expression.Consequence = p.parseBlockStatement()

	if p.peekTokenIs(lexer.ELSE) {
		p.nextToken()

		if p.peekTokenIs(lexer.IF) {
			// else if
			p.nextToken()
			expression.Alternative = &ast.BlockStatement{
				Statements: []ast.Statement{&ast.ExpressionStatement{
					Expression: p.parseIfExpression(),
				}},
			}
		} else {
			if !p.expectPeek(lexer.LBRACE) {
				return nil
			}
			expression.Alternative = p.parseBlockStatement()
		}
	}

	return expression
}

// parseFunctionLiteral parses a function literal expression.
func (p *Parser) parseFunctionLiteral() ast.Expression {
	lit := &ast.FunctionLiteral{Token: p.currentToken}

	// Check for named function: func name(params) { ... }
	if p.peekTokenIs(lexer.IDENT) {
		p.nextToken()
		lit.Name = p.currentToken.Literal
	}

	if !p.expectPeek(lexer.LPAREN) {
		return nil
	}

	lit.Parameters, lit.VariadicParam = p.parseFunctionParameters()

	if !p.expectPeek(lexer.LBRACE) {
		return nil
	}

	lit.Body = p.parseBlockStatement()

	return lit
}

// parseFunctionParameters parses function parameters.
// Supports regular parameters and variadic parameter (...args).
// Variadic parameter must be the last parameter.
// Returns regular parameters, and sets variadic param if present.
func (p *Parser) parseFunctionParameters() ([]*ast.Identifier, *ast.Identifier) {
	var identifiers []*ast.Identifier
	var variadicParam *ast.Identifier

	if p.peekTokenIs(lexer.RPAREN) {
		p.nextToken()
		return identifiers, nil
	}

	p.nextToken()

	// Check for variadic parameter as first parameter
	if p.currentTokenIs(lexer.ELLIPSIS) {
		// ...args as first/only parameter
		p.nextToken()
		variadicParam = &ast.Identifier{Token: p.currentToken, Value: p.currentToken.Literal}
		if !p.expectPeek(lexer.RPAREN) {
			return nil, nil
		}
		return identifiers, variadicParam
	}

	ident := &ast.Identifier{Token: p.currentToken, Value: p.currentToken.Literal}
	identifiers = append(identifiers, ident)

	for p.peekTokenIs(lexer.COMMA) {
		p.nextToken()
		p.nextToken()

		// Check for variadic parameter
		if p.currentTokenIs(lexer.ELLIPSIS) {
			p.nextToken()
			variadicParam = &ast.Identifier{Token: p.currentToken, Value: p.currentToken.Literal}
			if !p.expectPeek(lexer.RPAREN) {
				return nil, nil
			}
			return identifiers, variadicParam
		}

		ident := &ast.Identifier{Token: p.currentToken, Value: p.currentToken.Literal}
		identifiers = append(identifiers, ident)
	}

	if !p.expectPeek(lexer.RPAREN) {
		return nil, nil
	}

	return identifiers, variadicParam
}

// Helper methods

func (p *Parser) currentTokenIs(t lexer.TokenType) bool {
	return p.currentToken.Type == t
}

func (p *Parser) peekTokenIs(t lexer.TokenType) bool {
	return p.peekToken.Type == t
}

func (p *Parser) expectPeek(t lexer.TokenType) bool {
	if p.peekTokenIs(t) {
		p.nextToken()
		return true
	}
	p.peekError(t)
	return false
}

func (p *Parser) currentPrecedence() int {
	if p, ok := precedences[p.currentToken.Type]; ok {
		return p
	}
	return LOWEST
}

func (p *Parser) peekPrecedence() int {
	if p, ok := precedences[p.peekToken.Type]; ok {
		return p
	}
	return LOWEST
}

func (p *Parser) peekError(t lexer.TokenType) {
	msg := fmt.Sprintf("expected next token to be %s, got %s instead (line %d)",
		t, p.peekToken.Type, p.peekToken.Line)
	p.errors = append(p.errors, msg)
}

func (p *Parser) noPrefixParseFnError(t lexer.TokenType) {
	msg := fmt.Sprintf("no prefix parse function for %s found (line %d)", t, p.currentToken.Line)
	p.errors = append(p.errors, msg)
}

// Errors returns all parsing errors.
func (p *Parser) Errors() []string {
	return p.errors
}
