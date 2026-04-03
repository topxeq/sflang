// Package ast defines Abstract Syntax Tree nodes for Sflang.
// The AST is the intermediate representation between parsing and compilation.
package ast

import (
	"bytes"
	"strings"

	"github.com/topxeq/sflang/lexer"
)

// Node is the base interface for all AST nodes.
type Node interface {
	TokenLiteral() string // Returns the literal value of the token
	String() string        // Returns a string representation of the node
}

// Statement represents a statement node in the AST.
type Statement interface {
	Node
	statementNode()
}

// Expression represents an expression node in the AST.
type Expression interface {
	Node
	expressionNode()
}

// Program is the root node of the AST, containing all statements.
type Program struct {
	Statements []Statement
}

// TokenLiteral returns the literal of the first statement's token.
func (p *Program) TokenLiteral() string {
	if len(p.Statements) > 0 {
		return p.Statements[0].TokenLiteral()
	}
	return ""
}

// String returns a string representation of all statements.
func (p *Program) String() string {
	var out bytes.Buffer
	for _, s := range p.Statements {
		out.WriteString(s.String())
	}
	return out.String()
}

// Identifier represents an identifier expression (variable/function name).
type Identifier struct {
	Token lexer.Token // The IDENT token
	Value string
}

func (i *Identifier) expressionNode()      {}
func (i *Identifier) TokenLiteral() string { return i.Token.Literal }
func (i *Identifier) String() string       { return i.Value }

// LetStatement represents a variable declaration statement.
// Example: let x = 10;
type LetStatement struct {
	Token lexer.Token // The LET token
	Name  *Identifier
	Value Expression
}

func (ls *LetStatement) statementNode()       {}
func (ls *LetStatement) TokenLiteral() string { return ls.Token.Literal }
func (ls *LetStatement) String() string {
	var out bytes.Buffer
	out.WriteString(ls.TokenLiteral() + " ")
	out.WriteString(ls.Name.String())
	out.WriteString(" = ")
	if ls.Value != nil {
		out.WriteString(ls.Value.String())
	}
	return out.String()
}

// ReturnStatement represents a return statement.
// Example: return x + 1;
type ReturnStatement struct {
	Token       lexer.Token // The RETURN token
	ReturnValue Expression
}

func (rs *ReturnStatement) statementNode()       {}
func (rs *ReturnStatement) TokenLiteral() string { return rs.Token.Literal }
func (rs *ReturnStatement) String() string {
	var out bytes.Buffer
	out.WriteString(rs.TokenLiteral() + " ")
	if rs.ReturnValue != nil {
		out.WriteString(rs.ReturnValue.String())
	}
	return out.String()
}

// ExpressionStatement wraps an expression as a statement.
type ExpressionStatement struct {
	Token      lexer.Token // The first token of the expression
	Expression Expression
}

func (es *ExpressionStatement) statementNode()       {}
func (es *ExpressionStatement) TokenLiteral() string { return es.Token.Literal }
func (es *ExpressionStatement) String() string {
	if es.Expression != nil {
		return es.Expression.String()
	}
	return ""
}

// BlockStatement represents a block of statements enclosed in braces.
type BlockStatement struct {
	Token      lexer.Token // The { token
	Statements []Statement
}

func (bs *BlockStatement) statementNode()       {}
func (bs *BlockStatement) TokenLiteral() string { return bs.Token.Literal }
func (bs *BlockStatement) String() string {
	var out bytes.Buffer
	for _, s := range bs.Statements {
		out.WriteString(s.String())
	}
	return out.String()
}

// IntegerLiteral represents an integer literal expression.
type IntegerLiteral struct {
	Token lexer.Token
	Value int64
}

func (il *IntegerLiteral) expressionNode()      {}
func (il *IntegerLiteral) TokenLiteral() string { return il.Token.Literal }
func (il *IntegerLiteral) String() string       { return il.Token.Literal }

// FloatLiteral represents a floating-point literal expression.
type FloatLiteral struct {
	Token lexer.Token
	Value float64
}

func (fl *FloatLiteral) expressionNode()      {}
func (fl *FloatLiteral) TokenLiteral() string { return fl.Token.Literal }
func (fl *FloatLiteral) String() string       { return fl.Token.Literal }

// BigIntLiteral represents a big integer literal expression.
// BigInt supports arbitrary-precision integers with suffix 'n'.
type BigIntLiteral struct {
	Token lexer.Token
	Value string // String representation of the big integer
}

func (bil *BigIntLiteral) expressionNode()      {}
func (bil *BigIntLiteral) TokenLiteral() string { return bil.Token.Literal }
func (bil *BigIntLiteral) String() string       { return bil.Token.Literal }

// BigFloatLiteral represents a big float literal expression.
// BigFloat supports arbitrary-precision floating-point numbers with suffix 'm'.
type BigFloatLiteral struct {
	Token lexer.Token
	Value string // String representation of the big float
}

func (bfl *BigFloatLiteral) expressionNode()      {}
func (bfl *BigFloatLiteral) TokenLiteral() string { return bfl.Token.Literal }
func (bfl *BigFloatLiteral) String() string       { return bfl.Token.Literal }

// StringLiteral represents a string literal expression.
type StringLiteral struct {
	Token lexer.Token
	Value string
}

func (sl *StringLiteral) expressionNode()      {}
func (sl *StringLiteral) TokenLiteral() string { return sl.Token.Literal }
func (sl *StringLiteral) String() string       { return sl.Token.Literal }

// BooleanLiteral represents a boolean literal expression (true/false).
type BooleanLiteral struct {
	Token lexer.Token
	Value bool
}

func (bl *BooleanLiteral) expressionNode()      {}
func (bl *BooleanLiteral) TokenLiteral() string { return bl.Token.Literal }
func (bl *BooleanLiteral) String() string       { return bl.Token.Literal }

// NullLiteral represents the null literal.
type NullLiteral struct {
	Token lexer.Token
}

func (nl *NullLiteral) expressionNode()      {}
func (nl *NullLiteral) TokenLiteral() string { return nl.Token.Literal }
func (nl *NullLiteral) String() string       { return "null" }

// PrefixExpression represents a prefix operator expression.
// Example: -x, !flag
type PrefixExpression struct {
	Token    lexer.Token // The prefix operator token
	Operator string
	Right    Expression
}

func (pe *PrefixExpression) expressionNode()      {}
func (pe *PrefixExpression) TokenLiteral() string { return pe.Token.Literal }
func (pe *PrefixExpression) String() string {
	var out bytes.Buffer
	out.WriteString("(")
	out.WriteString(pe.Operator)
	out.WriteString(pe.Right.String())
	out.WriteString(")")
	return out.String()
}

// InfixExpression represents an infix operator expression.
// Example: x + y, a == b
type InfixExpression struct {
	Token    lexer.Token // The operator token
	Left     Expression
	Operator string
	Right    Expression
}

func (ie *InfixExpression) expressionNode()      {}
func (ie *InfixExpression) TokenLiteral() string { return ie.Token.Literal }
func (ie *InfixExpression) String() string {
	var out bytes.Buffer
	out.WriteString("(")
	out.WriteString(ie.Left.String())
	out.WriteString(" " + ie.Operator + " ")
	out.WriteString(ie.Right.String())
	out.WriteString(")")
	return out.String()
}

// PostfixExpression represents a postfix operator expression.
// Example: i++, j--
type PostfixExpression struct {
	Token    lexer.Token
	Left     Expression
	Operator string
}

func (pe *PostfixExpression) expressionNode()      {}
func (pe *PostfixExpression) TokenLiteral() string { return pe.Token.Literal }
func (pe *PostfixExpression) String() string {
	var out bytes.Buffer
	out.WriteString("(")
	out.WriteString(pe.Left.String())
	out.WriteString(pe.Operator)
	out.WriteString(")")
	return out.String()
}

// CallExpression represents a function call expression.
// Example: add(1, 2)
type CallExpression struct {
	Token     lexer.Token // The ( token
	Function  Expression  // The function being called
	Arguments []Expression
}

func (ce *CallExpression) expressionNode()      {}
func (ce *CallExpression) TokenLiteral() string { return ce.Token.Literal }
func (ce *CallExpression) String() string {
	var out bytes.Buffer
	args := make([]string, 0, len(ce.Arguments))
	for _, a := range ce.Arguments {
		args = append(args, a.String())
	}
	out.WriteString(ce.Function.String())
	out.WriteString("(")
	out.WriteString(strings.Join(args, ", "))
	out.WriteString(")")
	return out.String()
}

// ArrayLiteral represents an array literal expression.
// Example: [1, 2, 3]
type ArrayLiteral struct {
	Token    lexer.Token // The [ token
	Elements []Expression
}

func (al *ArrayLiteral) expressionNode()      {}
func (al *ArrayLiteral) TokenLiteral() string { return al.Token.Literal }
func (al *ArrayLiteral) String() string {
	var out bytes.Buffer
	elements := make([]string, 0, len(al.Elements))
	for _, e := range al.Elements {
		elements = append(elements, e.String())
	}
	out.WriteString("[")
	out.WriteString(strings.Join(elements, ", "))
	out.WriteString("]")
	return out.String()
}

// IndexExpression represents an index access expression.
// Example: arr[0], obj["key"]
type IndexExpression struct {
	Token lexer.Token // The [ token
	Left  Expression
	Index Expression
}

func (ie *IndexExpression) expressionNode()      {}
func (ie *IndexExpression) TokenLiteral() string { return ie.Token.Literal }
func (ie *IndexExpression) String() string {
	var out bytes.Buffer
	out.WriteString("(")
	out.WriteString(ie.Left.String())
	out.WriteString("[")
	out.WriteString(ie.Index.String())
	out.WriteString("])")
	return out.String()
}

// SliceExpression represents an array slice expression.
// Example: a[1:3] returns elements from index 1 to 2 (exclusive of 3)
// Start and End can be nil for open-ended slices: a[:3], a[1:], a[:]
type SliceExpression struct {
	Token lexer.Token // The [ token
	Left  Expression
	Start Expression // Can be nil for [:end]
	End   Expression // Can be nil for [start:]
}

func (se *SliceExpression) expressionNode()      {}
func (se *SliceExpression) TokenLiteral() string { return se.Token.Literal }
func (se *SliceExpression) String() string {
	var out bytes.Buffer
	out.WriteString("(")
	out.WriteString(se.Left.String())
	out.WriteString("[")
	if se.Start != nil {
		out.WriteString(se.Start.String())
	}
	out.WriteString(":")
	if se.End != nil {
		out.WriteString(se.End.String())
	}
	out.WriteString("])")
	return out.String()
}

// MapLiteral represents a map literal expression.
// Example: {"key": "value", "num": 42}
type MapLiteral struct {
	Token lexer.Token // The { token
	Pairs map[Expression]Expression
}

func (ml *MapLiteral) expressionNode()      {}
func (ml *MapLiteral) TokenLiteral() string { return ml.Token.Literal }
func (ml *MapLiteral) String() string {
	var out bytes.Buffer
	pairs := make([]string, 0, len(ml.Pairs))
	for key, value := range ml.Pairs {
		pairs = append(pairs, key.String()+":"+value.String())
	}
	out.WriteString("{")
	out.WriteString(strings.Join(pairs, ", "))
	out.WriteString("}")
	return out.String()
}

// FunctionLiteral represents a function definition expression.
// Example: func(x, y) { return x + y; }
// Variadic: func(x, ...args) { return args; }
type FunctionLiteral struct {
	Token          lexer.Token // The func token
	Name           string      // Optional function name
	Parameters     []*Identifier
	VariadicParam  *Identifier // Optional variadic parameter (...args)
	Body           *BlockStatement
}

func (fl *FunctionLiteral) expressionNode()      {}
func (fl *FunctionLiteral) TokenLiteral() string { return fl.Token.Literal }
func (fl *FunctionLiteral) String() string {
	var out bytes.Buffer
	params := make([]string, 0, len(fl.Parameters))
	for _, p := range fl.Parameters {
		params = append(params, p.String())
	}
	out.WriteString(fl.TokenLiteral())
	if fl.Name != "" {
		out.WriteString(" ")
		out.WriteString(fl.Name)
	}
	out.WriteString("(")
	out.WriteString(strings.Join(params, ", "))
	if fl.VariadicParam != nil {
		if len(params) > 0 {
			out.WriteString(", ")
		}
		out.WriteString("...")
		out.WriteString(fl.VariadicParam.String())
	}
	out.WriteString(") ")
	out.WriteString(fl.Body.String())
	return out.String()
}

// IfExpression represents an if-else expression.
// Example: if (x > 0) { x } else { -x }
type IfExpression struct {
	Token       lexer.Token // The if token
	Condition   Expression
	Consequence *BlockStatement
	Alternative *BlockStatement
}

func (ie *IfExpression) expressionNode()      {}
func (ie *IfExpression) TokenLiteral() string { return ie.Token.Literal }
func (ie *IfExpression) String() string {
	var out bytes.Buffer
	out.WriteString("if ")
	out.WriteString(ie.Condition.String())
	out.WriteString(" ")
	out.WriteString(ie.Consequence.String())
	if ie.Alternative != nil {
		out.WriteString(" else ")
		out.WriteString(ie.Alternative.String())
	}
	return out.String()
}

// ForStatement represents a for loop statement.
// Example: for (let i = 0; i < 10; i++) { ... }
type ForStatement struct {
	Token       lexer.Token // The for token
	Init        Statement   // Initialization statement
	Condition   Expression  // Loop condition
	Update      Statement   // Update statement
	Body        *BlockStatement
}

func (fs *ForStatement) statementNode()       {}
func (fs *ForStatement) TokenLiteral() string { return fs.Token.Literal }
func (fs *ForStatement) String() string {
	var out bytes.Buffer
	out.WriteString("for (")
	if fs.Init != nil {
		out.WriteString(fs.Init.String())
	}
	out.WriteString("; ")
	if fs.Condition != nil {
		out.WriteString(fs.Condition.String())
	}
	out.WriteString("; ")
	if fs.Update != nil {
		out.WriteString(fs.Update.String())
	}
	out.WriteString(") ")
	out.WriteString(fs.Body.String())
	return out.String()
}

// ForInStatement represents a for-in loop statement.
// Example: for (let k, v in map) { ... }
type ForInStatement struct {
	Token  lexer.Token // The for token
	Key    *Identifier
	Value  *Identifier
	Source Expression
	Body   *BlockStatement
}

func (fs *ForInStatement) statementNode()       {}
func (fs *ForInStatement) TokenLiteral() string { return fs.Token.Literal }
func (fs *ForInStatement) String() string {
	var out bytes.Buffer
	out.WriteString("for (")
	if fs.Key != nil {
		out.WriteString(fs.Key.String())
		if fs.Value != nil {
			out.WriteString(", ")
		}
	}
	if fs.Value != nil {
		out.WriteString(fs.Value.String())
	}
	out.WriteString(" in ")
	out.WriteString(fs.Source.String())
	out.WriteString(") ")
	out.WriteString(fs.Body.String())
	return out.String()
}

// BreakStatement represents a break statement.
type BreakStatement struct {
	Token lexer.Token
}

func (bs *BreakStatement) statementNode()       {}
func (bs *BreakStatement) TokenLiteral() string { return bs.Token.Literal }
func (bs *BreakStatement) String() string       { return "break" }

// ContinueStatement represents a continue statement.
type ContinueStatement struct {
	Token lexer.Token
}

func (cs *ContinueStatement) statementNode()       {}
func (cs *ContinueStatement) TokenLiteral() string { return cs.Token.Literal }
func (cs *ContinueStatement) String() string       { return "continue" }

// TryStatement represents a try-catch-finally statement.
// Example: try { ... } catch (e) { ... } finally { ... }
type TryStatement struct {
	Token        lexer.Token // The try token
	Body         *BlockStatement
	CatchVar     *Identifier
	CatchBody    *BlockStatement
	FinallyBody  *BlockStatement
}

func (ts *TryStatement) statementNode()       {}
func (ts *TryStatement) TokenLiteral() string { return ts.Token.Literal }
func (ts *TryStatement) String() string {
	var out bytes.Buffer
	out.WriteString("try ")
	out.WriteString(ts.Body.String())
	if ts.CatchBody != nil {
		out.WriteString(" catch (")
		if ts.CatchVar != nil {
			out.WriteString(ts.CatchVar.String())
		}
		out.WriteString(") ")
		out.WriteString(ts.CatchBody.String())
	}
	if ts.FinallyBody != nil {
		out.WriteString(" finally ")
		out.WriteString(ts.FinallyBody.String())
	}
	return out.String()
}

// ThrowStatement represents a throw statement.
// Example: throw "error message"
type ThrowStatement struct {
	Token   lexer.Token // The throw token
	ErrExpr Expression
}

func (ts *ThrowStatement) statementNode()       {}
func (ts *ThrowStatement) TokenLiteral() string { return ts.Token.Literal }
func (ts *ThrowStatement) String() string {
	var out bytes.Buffer
	out.WriteString("throw ")
	if ts.ErrExpr != nil {
		out.WriteString(ts.ErrExpr.String())
	}
	return out.String()
}

// AssignStatement represents an assignment statement.
// Example: x = 10, arr[0] = 1
type AssignStatement struct {
	Token lexer.Token // The = token
	Left  Expression
	Right Expression
}

func (as *AssignStatement) statementNode()       {}
func (as *AssignStatement) TokenLiteral() string { return as.Token.Literal }
func (as *AssignStatement) String() string {
	var out bytes.Buffer
	out.WriteString(as.Left.String())
	out.WriteString(" = ")
	out.WriteString(as.Right.String())
	return out.String()
}

// CompoundAssignStatement represents a compound assignment statement.
// Example: x += 1, arr[i] *= 2
type CompoundAssignStatement struct {
	Token    lexer.Token // The operator token (+=, -=, etc.)
	Left     Expression
	Operator string
	Right    Expression
}

func (cas *CompoundAssignStatement) statementNode()       {}
func (cas *CompoundAssignStatement) TokenLiteral() string { return cas.Token.Literal }
func (cas *CompoundAssignStatement) String() string {
	var out bytes.Buffer
	out.WriteString(cas.Left.String())
	out.WriteString(" ")
	out.WriteString(cas.Operator)
	out.WriteString(" ")
	out.WriteString(cas.Right.String())
	return out.String()
}
