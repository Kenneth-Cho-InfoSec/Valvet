"""Differential reference for papers/parsec_arith.valvet.

Uses Python `ast` to compute the expected value and expected pass/fail for
each input. Only + - * / over ints/floats are accepted (anything else, syntax
errors, and division by zero count as failures, mirroring the Valvet parser).
Floats are rendered with format(v, '.17g'), matching C printf %.17g used by
Valvet for Float64: with identical IEEE-754 doubles from + - * / only, the
strings agree bit-for-bit.
"""
import ast

CASES = [
    "3 + 4 * 2",
    "( 1 + 2 ) * 3",
    "10 - 4 / 2",
    "- 3 + 4",
    "2 * ( 3 + 4 ) - 5",
    "7 / 2",
    "3.5 * 2",
    "( ( 1 + 1 ) * ( 2 + 2 ) ) / 2",
    "0 - 5",
    "10 / 3",
    "1 +",
    "( 1 + 2",
    "3 * * 4",
    ") 3 + 4",
    "4 / ( 2 - 2 )",
    "",
    "3 4",
    "* 3 + 4",
]


def ev(node):
    if isinstance(node, ast.Expression):
        return ev(node.body)
    if isinstance(node, ast.Constant):
        if isinstance(node.value, bool) or not isinstance(node.value, (int, float)):
            raise ValueError("non-numeric constant")
        return float(node.value)
    if isinstance(node, ast.BinOp):
        l, r = ev(node.left), ev(node.right)
        if isinstance(node.op, ast.Add):
            return l + r
        if isinstance(node.op, ast.Sub):
            return l - r
        if isinstance(node.op, ast.Mult):
            return l * r
        if isinstance(node.op, ast.Div):
            return l / r  # ZeroDivisionError -> failure, like Valvet
        raise ValueError("bad operator")
    if isinstance(node, ast.UnaryOp) and isinstance(node.op, ast.USub):
        return -ev(node.operand)
    raise ValueError("bad node")


for src in CASES:
    print(f"in: {src}")
    try:
        val = ev(ast.parse(src, mode="eval"))
        print(f"out: {format(val, '.17g')}")
    except (SyntaxError, ValueError, ZeroDivisionError):
        print("out: ERROR")
