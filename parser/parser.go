// Package parser implements syntax analysis for Sflang source code.
// It converts a stream of tokens into an Abstract Syntax Tree (AST).
package parser

import (
	"fmt"

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
	// Check for assignment
	if p.peekTokenIs(lexer.ASSIGN) {
		return p.parseAssignmentStatement()
	}

	// Check for compound assignment
	if p.isCompoundAssignToken(p.peekToken.Type) {
		return p.parseCompoundAssignStatement()
	}

	stmt := &ast.ExpressionStatement{Token: p.currentToken}
	stmt.Expression = p.parseExpression(LOWEST)

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
func (p *Parser) parseForStatement() ast.Statement {
	stmt := &ast.ForStatement{Token: p.currentToken}

	if !p.expectPeek(lexer.LPAREN) {
		return nil
	}

	// Check for for-in syntax
	if p.peekTokenIs(lexer.IDENT) {
		// Could be for-in: for (k, v in obj) or for (k in obj)
		p.nextToken()
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
		// Not for-in, rewind
		p.nextToken()
		p.nextToken()
	}

	// Regular for loop: for (init; condition; update) { body }
	// Move past the opening parenthesis
	p.nextToken()

	if !p.currentTokenIs(lexer.SEMICOLON) {
		stmt.Init = p.parseStatement()
	}

	// After parseStatement, currentToken might be at semicolon (if statement consumed it)
	// or before semicolon. Check and handle both cases.
	if p.currentTokenIs(lexer.SEMICOLON) {
		// Statement already consumed the semicolon, move to condition
		p.nextToken()
	} else if p.peekTokenIs(lexer.SEMICOLON) {
		// Statement didn't consume semicolon, consume it now
		p.nextToken()
		p.nextToken()
	} else {
		// Missing semicolon
		p.peekError(lexer.SEMICOLON)
		return nil
	}

	if !p.currentTokenIs(lexer.SEMICOLON) {
		stmt.Condition = p.parseExpression(LOWEST)
	}

	if !p.expectPeek(lexer.SEMICOLON) {
		return nil
	}

	p.nextToken()
	if !p.currentTokenIs(lexer.RPAREN) {
		stmt.Update = p.parseStatement()
	}

	if !p.expectPeek(lexer.RPAREN) {
		return nil
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

// parseTryStatement parses a try-catch statement.
// Syntax: try { <body> } catch (<var>) { <catchBody> }
func (p *Parser) parseTryStatement() *ast.TryStatement {
	stmt := &ast.TryStatement{Token: p.currentToken}

	if !p.expectPeek(lexer.LBRACE) {
		return nil
	}

	stmt.Body = p.parseBlockStatement()

	if !p.expectPeek(lexer.CATCH) {
		return nil
	}

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

// parseIndexExpression parses an index expression.
func (p *Parser) parseIndexExpression(left ast.Expression) ast.Expression {
	exp := &ast.IndexExpression{Token: p.currentToken, Left: left}

	p.nextToken()
	exp.Index = p.parseExpression(LOWEST)

	if !p.expectPeek(lexer.RBRACKET) {
		return nil
	}

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

	// Check for named function: fn name(params) { ... }
	if p.peekTokenIs(lexer.IDENT) {
		p.nextToken()
		lit.Name = p.currentToken.Literal
	}

	if !p.expectPeek(lexer.LPAREN) {
		return nil
	}

	lit.Parameters = p.parseFunctionParameters()

	if !p.expectPeek(lexer.LBRACE) {
		return nil
	}

	lit.Body = p.parseBlockStatement()

	return lit
}

// parseFunctionParameters parses function parameters.
func (p *Parser) parseFunctionParameters() []*ast.Identifier {
	var identifiers []*ast.Identifier

	if p.peekTokenIs(lexer.RPAREN) {
		p.nextToken()
		return identifiers
	}

	p.nextToken()
	ident := &ast.Identifier{Token: p.currentToken, Value: p.currentToken.Literal}
	identifiers = append(identifiers, ident)

	for p.peekTokenIs(lexer.COMMA) {
		p.nextToken()
		p.nextToken()
		ident := &ast.Identifier{Token: p.currentToken, Value: p.currentToken.Literal}
		identifiers = append(identifiers, ident)
	}

	if !p.expectPeek(lexer.RPAREN) {
		return nil
	}

	return identifiers
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
