use valvet_core::{lexer, pipeline, source::Source};

fn src(text: &str) -> Source {
    Source::new("test.valvet".to_string(), text.to_string())
}

fn check_ok(text: &str) {
    let s = src(text);
    if let Err(ds) = pipeline::front_end(&s) {
        panic!(
            "expected well-formed, got:\n{}",
            pipeline::render_diags(&s, &ds)
        );
    }
}

fn check_codes(text: &str) -> Vec<&'static str> {
    let s = src(text);
    match pipeline::front_end(&s) {
        Ok(_) => panic!("expected failure, compiled ok"),
        Err(ds) => ds.iter().map(|d| d.code).collect(),
    }
}

#[test]
fn lexer_tokens() {
    let s = src("val x = 40 + 2\n");
    let (toks, errs) = lexer::lex(&s);
    assert!(errs.is_empty());
    let kinds: Vec<String> = toks.iter().map(|t| format!("{:?}", t.kind)).collect();
    assert!(kinds[0].starts_with("Val"));
    assert!(matches!(toks[3].kind, valvet_core::token::Tok::IntLit(40)));
}

#[test]
fn lexer_line_comment_and_nesting() {
    let s = src("// hi\nval x = 1 /* outer /* inner */ still */\n");
    let (toks, errs) = lexer::lex(&s);
    assert!(errs.is_empty());
    assert!(toks
        .iter()
        .any(|t| matches!(t.kind, valvet_core::token::Tok::Val)));
}

#[test]
fn lexer_unterminated_block_comment() {
    let s = src("/* never ends");
    let (_, errs) = lexer::lex(&s);
    assert!(errs.iter().any(|e| e.code == "E0003"));
}

#[test]
fn lexer_bad_char() {
    let s = src("val x = $");
    let (_, errs) = lexer::lex(&s);
    assert!(errs.iter().any(|e| e.code == "E0001"));
}

#[test]
fn parser_precedence() {
    // 1 + 2 * 3 == 7 must typecheck as Int32 arithmetic; encode via print of comparison.
    check_ok("fun main() { print(1 + 2 * 3 == 7) }\n");
}

#[test]
fn type_mismatch_rejected() {
    let codes = check_codes("fun main() { val x: Int32 = true }\n");
    assert!(codes.contains(&"E0201"), "{:?}", codes);
}

#[test]
fn val_reassignment_rejected() {
    let codes = check_codes("fun main() { val x = 1\n x = 2 }\n");
    assert!(codes.contains(&"E0205"), "{:?}", codes);
}

#[test]
fn move_then_use_rejected() {
    let codes = check_codes(
        "fun c(s: String) { print(s) }\nfun main() { val a = \"x\"\n c(a)\n print(a) }\n",
    );
    assert!(codes.contains(&"E0301"), "{:?}", codes);
}

#[test]
fn move_ok_when_unused() {
    check_ok("fun c(s: String) { print(s) }\nfun main() { val a = \"x\"\n c(a) }\n");
}

#[test]
fn print_borrows_string() {
    check_ok("fun main() { val a = \"x\"\n print(a)\n print(a) }\n");
}

#[test]
fn missing_main_rejected() {
    let codes = check_codes("fun helper() { }\n");
    assert!(codes.contains(&"E0603"), "{:?}", codes);
}

#[test]
fn codegen_frees_strings() {
    let s = src("fun main() { val a = \"x\"\n print(a) }\n");
    let checked = pipeline::front_end(&s).expect("front end");
    let c = valvet_core::codegen::lower(&checked).expect("lower");
    assert!(
        c.contains("vstr_free"),
        "generated C must deterministically free Strings"
    );
    assert!(c.contains("int main(int argc, char **argv)"));
}

#[test]
fn codegen_int_main_returns() {
    let s = src("fun add(a: Int32, b: Int32): Int32 { a + b }\nfun main(): Int32 { add(1, 2) }\n");
    let checked = pipeline::front_end(&s).expect("front end");
    let c = valvet_core::codegen::lower(&checked).expect("lower");
    assert!(c.contains("v_add_a2_fn("));
    // Tail values materialize before scope frees (owned locals stay live).
    assert!(c.contains("return _vt"));
}

// ---------- regression tests (bugs found during MVP implementation) ----------

#[test]
fn regression_newline_not_eaten_by_operators() {
    // Binary-op parsing must not consume the statement-terminating newline;
    // otherwise `expect_separator` fails and recovery can stall (hang).
    check_ok("fun main() {\n    val a = 40 + 2\n    print(a)\n}\n");
}

#[test]
fn regression_trailing_if_else_is_tail() {
    check_ok("fun sign(x: Int32): Int32 {\n    if (x > 0) { 1 } else { 0 - 1 }\n}\nfun main() { print(sign(1)) }\n");
}

#[test]
fn regression_diverging_block_coerces() {
    check_ok(
        "fun f(n: Int32): Int32 {\n    if (n > 0) {\n        return 1\n    }\n    return 0 - 1\n}\nfun main() { print(f(1)) }\n",
    );
}

#[test]
fn regression_string_temps_freed_once() {
    // Call results and literals in borrow positions must be freed exactly once:
    // every frame temp is either freed at statement end or NULLed on transfer.
    let s = src(
        "fun id(s: String): String { s }\nfun main() {\n    val a = \"x\"\n    print(id(a) + \"!\")\n}\n",
    );
    let checked = pipeline::front_end(&s).expect("front end");
    let c = valvet_core::codegen::lower(&checked).expect("lower");
    assert!(c.contains("vstr_free"));
    // The moved variable must be NULLed exactly where ownership transfers.
    assert!(c.contains("v_a.s = NULL"));
}

// ---------- Phase A: loops and casts ----------

#[test]
fn for_range_ok() {
    check_ok("fun main() {\n var t = 0\n for (i in 0..5) { t += i }\n print(t)\n}\n");
}

#[test]
fn for_inclusive_ok() {
    check_ok("fun main() {\n for (i in 1..=3) { print(i) }\n}\n");
}

#[test]
fn for_float_bounds_rejected() {
    let codes = check_codes("fun main() {\n for (i in 0.0..5.0) { print(i) }\n}\n");
    assert!(codes.contains(&"E0201"), "{:?}", codes);
}

#[test]
fn break_outside_loop_rejected() {
    let codes = check_codes("fun main() {\n break\n}\n");
    assert!(codes.contains(&"E0005"), "{:?}", codes);
}

#[test]
fn loop_break_value_ok() {
    check_ok(
        "fun f(n: Int32): Int32 {\n var i = 0\n loop {\n if (i >= n) { break i }\n i += 1\n }\n}\nfun main() { print(f(3)) }\n",
    );
}

#[test]
fn loop_break_value_mismatch_rejected() {
    let codes = check_codes(
        "fun main() {\n val x: Int32 = loop {\n if (true) { break 1 }\n break\n }\n print(x)\n}\n",
    );
    // plain break mixed with break-value, or Unit-vs-Int32: either E0201 surfaces
    assert!(codes.contains(&"E0201"), "{:?}", codes);
}

#[test]
fn cast_int_widen_ok() {
    check_ok("fun main() {\n val a = 5\n print(a as Int64)\n}\n");
}

#[test]
fn cast_float_saturating_ok() {
    check_ok("fun main() {\n print((1e300) as Int32)\n print((0.0 / 0.0) as Int32)\n}\n");
}

#[test]
fn cast_bool_rejected() {
    let codes = check_codes("fun main() {\n print(true as Int32)\n}\n");
    assert!(codes.contains(&"E0201"), "{:?}", codes);
}

#[test]
fn loop_value_codegen_uses_label() {
    let s = src(
        "fun f(n: Int32): Int32 {\n var i = 0\n loop {\n if (i >= n) { break i }\n i += 1\n }\n}\nfun main() { print(f(1)) }\n",
    );
    let checked = pipeline::front_end(&s).expect("front end");
    let c = valvet_core::codegen::lower(&checked).expect("lower");
    assert!(
        c.contains("goto "),
        "loop-value lowering needs an end label"
    );
    assert!(c.contains("v_f2i32") || c.contains("while (1)"));
}

// ---------- Phase B: structs and enums ----------

#[test]
fn struct_basic_ok() {
    check_ok("struct Point { x: Float64, y: Float64 }\nfun main() {\n val p = Point { x: 1.0, y: 2.0 }\n print(p.x + p.y)\n}\n");
}

#[test]
fn struct_missing_field_rejected() {
    let codes = check_codes(
        "struct Point { x: Float64, y: Float64 }\nfun main() {\n val p = Point { x: 1.0 }\n print(p.x)\n}\n",
    );
    assert!(codes.contains(&"E0201"), "{:?}", codes);
}

#[test]
fn struct_unknown_field_rejected() {
    let codes = check_codes(
        "struct Point { x: Float64 }\nfun main() {\n val p = Point { x: 1.0 }\n print(p.yer)\n}\n",
    );
    assert!(codes.contains(&"E0401"), "{:?}", codes);
}

#[test]
fn struct_use_after_move_rejected() {
    let codes = check_codes(
        "struct U { name: String }\nfun c(u: U) { print(u.name) }\nfun main() {\n val u = U { name: \"a\" }\n c(u)\n print(u.name)\n}\n",
    );
    assert!(codes.contains(&"E0301"), "{:?}", codes);
}

#[test]
fn copy_struct_copies_freely() {
    // All-Copy structs are Copy: reuse after "move" position is fine.
    check_ok("struct P { x: Int32 }\nfun c(p: P) { print(p.x) }\nfun main() {\n val p = P { x: 1 }\n c(p)\n print(p.x)\n}\n");
}

#[test]
fn recursive_struct_rejected() {
    let codes = check_codes("struct Node { next: Node }\nfun main() { print(1) }\n");
    assert!(codes.contains(&"E0201"), "{:?}", codes);
}

#[test]
fn tuple_struct_ok() {
    check_ok("struct Meters(Float64);\nfun main() {\n val m = Meters(1.5)\n print(m.0)\n}\n");
}

#[test]
fn enum_unknown_variant_rejected() {
    let codes = check_codes(
        "enum S { Circle(Float64) }\nfun main() {\n val s = S::Square(1.0)\n print(1)\n}\n",
    );
    assert!(codes.contains(&"E0401"), "{:?}", codes);
}

#[test]
fn struct_drop_fn_generated() {
    let s = src(
        "struct U { name: String }\nfun main() {\n val u = U { name: \"a\" }\n print(u.name)\n}\n",
    );
    let checked = pipeline::front_end(&s).expect("front end");
    let c = valvet_core::codegen::lower(&checked).expect("lower");
    assert!(c.contains("v_drop_U"), "owned structs need drop helpers");
    assert!(
        c.contains("typedef struct"),
        "struct layout must be emitted"
    );
}

// ---------- Phase C: match and if-let ----------

#[test]
fn match_int_wild_ok() {
    check_ok(
        "fun main() {\n val s = match (1) {\n 0 -> \"zero\",\n _ -> \"many\",\n }\n print(s)\n}\n",
    );
}

#[test]
fn match_non_exhaustive_rejected() {
    let codes =
        check_codes("fun main() {\n val s = match (1) {\n 0 -> \"zero\",\n }\n print(s)\n}\n");
    assert!(codes.contains(&"E0204"), "{:?}", codes);
}

#[test]
fn match_arms_disagree_rejected() {
    let codes = check_codes(
        "fun main() {\n val s = match (true) {\n true -> 1,\n false -> \"no\",\n }\n print(s)\n}\n",
    );
    assert!(codes.contains(&"E0201"), "{:?}", codes);
}

#[test]
fn match_enum_ok() {
    check_ok("enum S { A, B(Int32) }\nfun f(s: S): Int32 {\n match (s) {\n S::A -> 0,\n S::B(v) -> v,\n }\n}\nfun main() { print(f(S::B(3))) }\n");
}

#[test]
fn match_unknown_variant_rejected() {
    let codes = check_codes(
        "enum S { A }\nfun f(s: S): Int32 {\n match (s) {\n S::Z -> 0,\n _ -> 1,\n }\n}\nfun main() { print(f(S::A)) }\n",
    );
    assert!(codes.contains(&"E0401"), "{:?}", codes);
}

#[test]
fn if_let_ok() {
    check_ok("enum O { Some(Int32), None }\nfun main() {\n val o = O::Some(1)\n if let O::Some(v) = o {\n print(v)\n }\n}\n");
}

#[test]
fn match_guard_falls_through() {
    // Guards must not capture the else-chain: failing guard tries next arm.
    let s = src("fun f(n: Int32): String {\n match (n) {\n x if x > 0 -> \"pos\",\n _ -> \"other\",\n }\n}\nfun main() { print(f(0 - 1)) }\n");
    let checked = pipeline::front_end(&s).expect("front end");
    let c = valvet_core::codegen::lower(&checked).expect("lower");
    // Sequential ifs (correct) rather than else-if chain (wrong on guard-false).
    // Scoped to the lowered function: the shared prelude legitimately
    // contains `else if` chains of its own.
    let body = &c[c.find("v_f_a1_fn").expect("lowered f")..];
    assert!(
        !body.contains("else if"),
        "guard fallthrough needs sequential ifs"
    );
}

#[test]
fn or_pattern_same_bindings_ok() {
    check_ok("fun main() {\n val s = match (2) {\n 1 | 2 -> \"small\",\n _ -> \"big\",\n }\n print(s)\n}\n");
}

// ---------- Phase D: Option/Result ----------

#[test]
fn option_some_none_ok() {
    check_ok("fun main() {\n val a: Option<Int32> = Some(41)\n val b = match (a) {\n Some(v) -> v + 1,\n None -> 0,\n }\n print(b)\n}\n");
}

#[test]
fn none_needs_annotation() {
    let codes = check_codes("fun main() {\n val x = None\n print(0)\n}\n");
    assert!(codes.contains(&"E0201"), "{:?}", codes);
}

#[test]
fn ok_needs_annotation() {
    let codes = check_codes("fun main() {\n val x = Ok(1)\n print(0)\n}\n");
    assert!(codes.contains(&"E0201"), "{:?}", codes);
}

#[test]
fn result_exhaustiveness() {
    let codes = check_codes("fun main() {\n val r: Result<Int32, String> = Ok(1)\n val s = match (r) {\n Ok(n) -> n,\n }\n print(s)\n}\n");
    assert!(codes.contains(&"E0204"), "{:?}", codes);
}

#[test]
fn generic_arity_rejected() {
    let codes = check_codes("fun main() {\n val x: Option<Int32, String> = None\n print(0)\n}\n");
    assert!(codes.contains(&"E0201"), "{:?}", codes);
}

#[test]
fn vec_macro_ok() {
    check_ok("fun main() {\n val v = vec![1, 2, 3]\n print(v.len())\n}\n");
}

#[test]
fn optfield_string_borrows() {
    check_ok("struct U { name: String }\nfun main() {\n val u: Option<U> = Some(U { name: \"a\" })\n val n = match (u?.name) {\n Some(s) -> s,\n None -> \"?\",\n }\n print(n)\n print(1)\n}\n");
}

#[test]
fn option_drop_fn_generated() {
    let s = src("fun main() {\n val a: Option<String> = Some(\"x\")\n print(1)\n}\n");
    let checked = pipeline::front_end(&s).expect("front end");
    let c = valvet_core::codegen::lower(&checked).expect("lower");
    assert!(
        c.contains("v_drop_Option_String"),
        "owned instantiations need drop helpers"
    );
}

// ---------- Phase E: ? operator ----------

#[test]
fn try_result_ok() {
    check_ok("fun f(): Result<Int32, String> {\n return Ok(1)\n}\nfun g(): Result<Int32, String> {\n val x = f()?\n return Ok(x + 1)\n}\nfun main() { print(1) }\n");
}

#[test]
fn try_wrong_context_rejected() {
    let codes = check_codes(
        "fun f(): Result<Int32, String> {\n return Ok(1)\n}\nfun main() {\n val x = f()?\n print(x)\n}\n",
    );
    assert!(codes.contains(&"E0702"), "{:?}", codes);
}

#[test]
fn try_non_result_rejected() {
    let codes = check_codes(
        "fun main(): Result<Int32, String> {\n val x = 5?\n print(x)\n return Ok(0)\n}\n",
    );
    assert!(codes.contains(&"E0702"), "{:?}", codes);
}

#[test]
fn try_error_mismatch_rejected() {
    let codes = check_codes(
        "fun f(): Result<Int32, String> {\n return Ok(1)\n}\nfun g(): Result<Int32, Int32> {\n val x = f()?\n return Ok(x)\n}\nfun main() { print(1) }\n",
    );
    assert!(codes.contains(&"E0702"), "{:?}", codes);
}

#[test]
fn try_option_ok() {
    check_ok("fun f(n: Int32): Option<Int32> {\n if (n > 0) {\n return Some(n)\n }\n return None\n}\nfun g(n: Int32): Option<Int32> {\n val m = f(n)?\n return Some(m + 1)\n}\nfun main() { print(1) }\n");
}

#[test]
fn try_main_result_ok() {
    check_ok(
        "fun f(): Result<Int32, String> {\n return Ok(1)\n}\nfun main(): Result<Int32, String> {\n val x = f()?\n print(x)\n return Ok(0)\n}\n",
    );
}

#[test]
fn try_early_return_drops() {
    // The early-return path must run scope frees (checked in codegen text).
    let s = src("fun f(s: String): Result<Int32, String> {\n val t = g(s)?\n return Ok(t)\n}\nfun g(s: String): Result<Int32, String> {\n return Ok(1)\n}\nfun main() { print(1) }\n");
    let checked = pipeline::front_end(&s).expect("front end");
    let c = valvet_core::codegen::lower(&checked).expect("lower");
    assert!(c.contains("vstr_free"), "early-? path must run Drops");
}

// ---------- Phase G: references & NLL ----------

#[test]
fn borrow_shared_ok() {
    check_ok("fun main() {\n val x = 1\n val r = &x\n val s = &x\n print(*r)\n print(*s)\n}\n");
}

#[test]
fn borrow_mut_write_ok() {
    check_ok("fun main() {\n var y = 1\n val m = &mut y\n *m = *m + 1\n print(y)\n}\n");
}

#[test]
fn borrow_nll_ends_at_last_use() {
    // THE NLL test: after r's last use, x is writable again.
    check_ok("fun main() {\n var x = 1\n val r = &x\n print(*r)\n x = 2\n print(x)\n}\n");
}

#[test]
fn borrow_exclusive_rejected() {
    let codes =
        check_codes("fun main() {\n var y = 1\n val m = &mut y\n print(y)\n print(*m)\n}\n");
    assert!(codes.contains(&"E0302"), "{:?}", codes);
}

#[test]
fn borrow_escape_rejected() {
    let codes =
        check_codes("fun bad(x: &Int32): &Int32 {\n val y = 1\n &y\n}\nfun main() { print(1) }\n");
    assert!(codes.contains(&"E0303"), "{:?}", codes);
}

#[test]
fn borrow_temporary_rejected() {
    let codes = check_codes("fun main() {\n val r = &(1 + 2)\n print(*r)\n}\n");
    assert!(codes.contains(&"E0303"), "{:?}", codes);
}

#[test]
fn borrow_mut_of_val_rejected() {
    let codes = check_codes("fun main() {\n val x = 1\n val m = &mut x\n print(*m)\n}\n");
    assert!(codes.contains(&"E0205"), "{:?}", codes);
}

#[test]
fn borrow_move_while_borrowed_rejected() {
    let codes = check_codes(
        "fun main() {\n var s = \"hi\"\n val r = &s\n val t = s\n print(*r)\n print(t)\n}\n",
    );
    assert!(codes.contains(&"E0301"), "{:?}", codes);
}

#[test]
fn borrow_elision_ambiguous_rejected() {
    let codes =
        check_codes("fun amb(a: &Int32, b: &Int32): &Int32 {\n a\n}\nfun main() { print(1) }\n");
    assert!(codes.contains(&"E0303"), "{:?}", codes);
}

#[test]
fn borrow_ref_in_struct_rejected() {
    let codes = check_codes("struct S { r: &Int32 }\nfun main() { print(1) }\n");
    assert!(codes.contains(&"E0201"), "{:?}", codes);
}

#[test]
fn borrow_deref_clones_string() {
    // `*r` of a String deep-clones (no buffer alias); both live on.
    let s =
        src("fun main() {\n var s = \"hi\"\n val r = &s\n val t = *r\n print(t)\n print(s)\n}\n");
    let checked = pipeline::front_end(&s).expect("front end");
    let c = valvet_core::codegen::lower(&checked).expect("lower");
    assert!(c.contains("vstr_clone"), "deref of owned values must clone");
}

#[test]
fn borrow_newline_after_borrow_is_deref() {
    // Regression: `*m` starting a line after `&mut y` is dereference, not `*` continuation.
    check_ok("fun main() {\n var y = 1\n val m = &mut y\n *m = 2\n print(y)\n}\n");
}

// ---------- Phase F: Vec ----------

#[test]
fn vec_macro_len_ok() {
    check_ok("fun main() {\n val v = vec![1, 2, 3]\n print(v.len())\n}\n");
}

#[test]
fn vec_mixed_rejected() {
    let codes = check_codes("fun main() {\n val v = vec![1, \"two\"]\n print(v.len())\n}\n");
    assert!(codes.contains(&"E0201"), "{:?}", codes);
}

#[test]
fn vec_empty_needs_annotation() {
    let codes = check_codes("fun main() {\n val v = vec![]\n print(v.len())\n}\n");
    assert!(codes.contains(&"E0201"), "{:?}", codes);
}

#[test]
fn vec_unknown_method_rejected() {
    let codes = check_codes("fun main() {\n val v = vec![1]\n print(v.frobnicate())\n}\n");
    assert!(codes.contains(&"E0401"), "{:?}", codes);
}

#[test]
fn vec_sort_accepted() {
    check_ok("fun main() {\n var v = vec![3, 1, 2]\n v.sort()\n print(v.len())\n}\n");
    check_ok("fun main() {\n val v = vec![\"b\", \"a\"]\n print(v.order())\n}\n");
    check_ok("fun main() {\n var v = vec![true, false]\n v.sort()\n print(v.len())\n}\n");
}

#[test]
fn vec_sort_rejected() {
    let codes = check_codes("fun main() {\n var v = vec![3, 1]\n v.sort(1)\n print(v.len())\n}\n");
    assert!(codes.contains(&"E0201"), "{:?}", codes);
    let codes = check_codes("struct S { x: Int32 }\nfun main() {\n var v = vec![S { x: 1 }]\n v.sort()\n print(v.len())\n}\n");
    assert!(codes.contains(&"E0201"), "{:?}", codes);
    let codes = check_codes("fun main() {\n val v = vec![2, 1]\n print(v.order(0))\n}\n");
    assert!(codes.contains(&"E0201"), "{:?}", codes);
}

#[test]
fn vec_push_wrong_type_rejected() {
    let codes =
        check_codes("fun main() {\n var v = vec![1.0]\n v.push(\"s\")\n print(v.len())\n}\n");
    assert!(codes.contains(&"E0201"), "{:?}", codes);
}

#[test]
fn vec_method_on_scalar_rejected() {
    let codes = check_codes("fun main() {\n print((5).len())\n}\n");
    assert!(codes.contains(&"E0401"), "{:?}", codes);
}

#[test]
fn vec_for_each_ok() {
    check_ok("fun main() {\n var t = 0\n for (x in vec![1, 2]) { t += x }\n print(t)\n}\n");
}

#[test]
fn vec_for_nonvec_rejected() {
    let codes = check_codes("fun main() {\n for (x in 5) { print(x) }\n}\n");
    assert!(codes.contains(&"E0201"), "{:?}", codes);
}

#[test]
fn vec_drop_fn_generated() {
    let s = src("fun main() {\n var v = vec![\"a\"]\n v.push(\"b\")\n print(v.len())\n}\n");
    let checked = pipeline::front_end(&s).expect("front end");
    let c = valvet_core::codegen::lower(&checked).expect("lower");
    assert!(
        c.contains("v_drop_Vec_String"),
        "owned vecs need drop helpers"
    );
    assert!(
        c.contains("vvec_Vec_String_push"),
        "push runtime must exist"
    );
}

// ---------- Phase H: modules/imports ----------

#[test]
fn import_single_parses() {
    let s = src("import math::double;\nfun main() {\n print(double(2))\n}\n");
    let prog = pipeline::parse_unit(&s).expect("parse");
    assert_eq!(prog.imports.len(), 1);
    assert_eq!(prog.imports[0].path, vec!["math".to_string()]);
    assert_eq!(prog.imports[0].items[0].name, "double");
    assert!(prog.imports[0].items[0].alias.is_none());
}

#[test]
fn import_braces_and_alias_parse() {
    let s = src("import shapes::{Point as Pt, origin};\nfun main() {\n print(1)\n}\n");
    let prog = pipeline::parse_unit(&s).expect("parse");
    assert_eq!(prog.imports.len(), 1);
    let items = &prog.imports[0].items;
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].name, "Point");
    assert_eq!(items[0].alias.as_deref(), Some("Pt"));
    assert_eq!(items[1].name, "origin");
}

#[test]
fn import_dotted_rejected() {
    let codes = check_codes("import math.double;\nfun main() {\n print(1)\n}\n");
    assert!(codes.contains(&"E0602"), "{:?}", codes);
}

#[test]
fn import_glob_rejected() {
    let codes = check_codes("import math::*;\nfun main() {\n print(1)\n}\n");
    assert!(codes.contains(&"E0800"), "{:?}", codes);
}

#[test]
fn import_needs_file_mode() {
    // Single-source compilation cannot resolve imports.
    let codes = check_codes("import math::double;\nfun main() {\n print(double(2))\n}\n");
    assert!(codes.contains(&"E0701"), "{:?}", codes);
}

#[test]
fn qname_shapes() {
    assert_eq!(valvet_core::resolver::qname("", "f"), "f");
    assert_eq!(valvet_core::resolver::qname("a.b", "Point"), "a.b::Point");
}

/// Write a multi-file project under a unique temp dir for file-mode tests.
fn write_project(name: &str, files: &[(&str, &str)]) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("valvet_h_{}_{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("mkdir");
    for (rel, text) in files {
        std::fs::write(dir.join(rel), text).expect("write");
    }
    dir
}

#[test]
fn file_mode_basic_import() {
    let dir = write_project(
        "basic",
        &[
            (
                "main.valvet",
                "import math::double;\nfun main() {\n print(double(21))\n}\n",
            ),
            (
                "math.valvet",
                "module math;\n\npub fun double(n: Int32): Int32 {\n n * 2\n}\n",
            ),
        ],
    );
    let checked = pipeline::front_end_file(&dir.join("main.valvet")).expect("check");
    let c = valvet_core::codegen::lower(&checked).expect("lower");
    assert!(
        c.contains("v_math___double_a1_fn"),
        "imported fns mangle with their module"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn file_mode_private_leak() {
    let dir = write_project(
        "private",
        &[
            (
                "main.valvet",
                "import math::secret;\nfun main() {\n print(secret(1))\n}\n",
            ),
            (
                "math.valvet",
                "module math;\n\nfun secret(n: Int32): Int32 {\n n\n}\n",
            ),
        ],
    );
    match pipeline::front_end_file(&dir.join("main.valvet")) {
        Ok(_) => panic!("expected E0401"),
        Err(fd) => assert!(fd.codes().contains(&"E0401"), "{:?}", fd.codes()),
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn file_mode_cycle() {
    let dir = write_project(
        "cycle",
        &[
            (
                "main.valvet",
                "import a::fa;\nfun main() {\n print(fa())\n}\n",
            ),
            (
                "a.valvet",
                "module a;\n\nimport b::fb;\n\npub fun fa(): Int32 {\n fb()\n}\n",
            ),
            (
                "b.valvet",
                "module b;\n\nimport a::fa;\n\npub fun fb(): Int32 {\n fa()\n}\n",
            ),
        ],
    );
    match pipeline::front_end_file(&dir.join("main.valvet")) {
        Ok(_) => panic!("expected E0601"),
        Err(fd) => assert!(fd.codes().contains(&"E0601"), "{:?}", fd.codes()),
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn file_mode_module_mismatch() {
    let dir = write_project(
        "mismatch",
        &[
            (
                "main.valvet",
                "import real::ok;\nfun main() {\n print(ok())\n}\n",
            ),
            (
                "real.valvet",
                "module wrong;\n\npub fun ok(): Int32 {\n 1\n}\n",
            ),
        ],
    );
    match pipeline::front_end_file(&dir.join("main.valvet")) {
        Ok(_) => panic!("expected E0701"),
        Err(fd) => assert!(fd.codes().contains(&"E0701"), "{:?}", fd.codes()),
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn file_mode_forward_call_gets_prototype() {
    // Single-file forward calls now link too (prototypes precede definitions).
    let s = src("fun main() {\n print(f())\n}\nfun f(): Int32 {\n 1\n}\n");
    let checked = pipeline::front_end(&s).expect("front end");
    let c = valvet_core::codegen::lower(&checked).expect("lower");
    assert!(
        c.contains("v_f_a0_fn(void);"),
        "prototypes must precede definitions"
    );
}

#[test]
fn file_mode_enum_across_modules() {
    // Variant construction + match coverage must use qualified enum names.
    let dir = write_project(
        "enumx",
        &[
            (
                "main.valvet",
                "import lib::{Shade, shade_n};\nfun main() {\n val s = Shade::Dark\n print(shade_n(s))\n}\n",
            ),
            (
                "lib.valvet",
                "module lib;\n\npub enum Shade { Light, Dark }\n\npub fun shade_n(s: Shade): Int32 {\n match (s) {\n Shade::Light -> 10,\n Shade::Dark -> 20,\n }\n}\n",
            ),
        ],
    );
    let checked = pipeline::front_end_file(&dir.join("main.valvet")).expect("check");
    let c = valvet_core::codegen::lower(&checked).expect("lower");
    assert!(
        c.contains("Vlib__Shade"),
        "imported enum layouts sanitize the qualifier"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------- Phase I: println ----------

#[test]
fn print_deprecated_warns() {
    // `print` still compiles (same behavior) but warns W1002 per call site.
    let s = src("fun main() {\n print(1)\n print(2)\n}\n");
    let checked = pipeline::front_end(&s).expect("print still compiles");
    let warns: Vec<&str> = checked.warnings.iter().map(|d| d.code).collect();
    assert_eq!(warns, vec!["W1002", "W1002"], "{:?}", warns);
    assert!(checked.warnings.iter().all(|d| d.warning));
    let rendered = checked.warnings[0].render(&s);
    assert!(rendered.starts_with("warning[W1002]"), "{}", rendered);
}

#[test]
fn println_no_warning() {
    let s = src("fun main() {\n println(1)\n}\n");
    let checked = pipeline::front_end(&s).expect("front end");
    assert!(checked.warnings.is_empty());
}

#[test]
fn println_composites_ok() {
    check_ok("fun main() {\n println(vec![1])\n println(Some(1))\n val n: Option<Int32> = None\n println(n)\n val o: Result<Int32, String> = Ok(1)\n println(o)\n}\n");
}

#[test]
fn println_unprintable_rejected() {
    let codes =
        check_codes("struct P { x: Int32 }\nfun main() {\n val p = P { x: 1 }\n println(p)\n}\n");
    assert!(codes.contains(&"E0201"), "{:?}", codes);
    let codes = check_codes("fun main() {\n val x = 1\n val r = &x\n println(r)\n}\n");
    assert!(codes.contains(&"E0201"), "{:?}", codes);
}

#[test]
fn println_nested_helpers_generated() {
    // Nested composites need layouts + formatters for every level.
    let s = src("fun main() {\n val z: Option<Int32> = None\n println(vec![Some(1), z])\n}\n");
    let checked = pipeline::front_end(&s).expect("front end");
    let c = valvet_core::codegen::lower(&checked).expect("lower");
    assert!(c.contains("vfmt_Vec_Option_Int32"), "nested vec formatter");
    assert!(c.contains("vfmt_Option_Int32"), "inner option formatter");
    assert!(c.contains("} VOption_Int32;"), "inner layout emitted");
}

#[test]
fn recursive_type_through_vec_rejected() {
    let codes = check_codes("struct S { v: Vec<S> }\nfun main() {\n println(1)\n}\n");
    assert!(codes.contains(&"E0201"), "{:?}", codes);
}

#[test]
fn script_top_level_accepted_single_source() {
    check_ok("val x = 40 + 2;\nprintln(x);\n");
}

#[test]
fn script_exp_header_rejects_statements() {
    let codes = check_codes("mode exp;\nprintln(1);\n");
    assert!(codes.contains(&"E0604"), "{:?}", codes);
}

#[test]
fn script_statements_and_main_clash() {
    let codes = check_codes("println(1);\nfun main() {\n println(2)\n}\n");
    assert!(codes.contains(&"E0604"), "{:?}", codes);
}

// ---------- Error-reporting upgrades: snippets, poison, move notes ----------

#[test]
fn snippet_renders_source_line_and_caret() {
    let s = src("val x = 1 as Int64\nval y = x + \"s\"\n");
    match pipeline::front_end(&s) {
        Ok(_) => panic!("expected failure"),
        Err(ds) => {
            assert_eq!(ds.len(), 1, "{:?}", pipeline::render_diags(&s, &ds));
            let r = ds[0].render(&s);
            assert!(r.contains("val y = x + \"s\""), "{}", r);
            assert!(r.contains("^^^"), "{}", r);
            assert!(r.contains("--> test.valvet:2:"), "{}", r);
        }
    }
}

#[test]
fn no_cascade_unknown_var() {
    // One root cause, one diagnostic (used to append E0201 for println).
    assert_eq!(check_codes("println(nope)\n"), vec!["E0401"]);
}

#[test]
fn no_cascade_binop() {
    assert_eq!(check_codes("val x = nope + 1\n"), vec!["E0401"]);
}

#[test]
fn no_cascade_chained() {
    // Poison flows through `a`, `b`, `c` without further diagnostics.
    assert_eq!(
        check_codes("val a = nope\nval b = a + 1\nval c = b + 2\nprintln(c)\n"),
        vec!["E0401"]
    );
}

#[test]
fn move_note_labels_move_site() {
    let s = src("val v = vec![1 as Int64]\nval w = v\nprintln(v)\n");
    match pipeline::front_end(&s) {
        Ok(_) => panic!("expected failure"),
        Err(ds) => {
            assert_eq!(ds.len(), 1, "{:?}", pipeline::render_diags(&s, &ds));
            let d = &ds[0];
            assert_eq!(d.code, "E0301");
            assert_eq!(d.labels.len(), 1, "{:?}", d.labels);
            assert_eq!(d.labels[0].1, "value moved here");
            let r = d.render(&s);
            assert!(r.contains("note: value moved here"), "{}", r);
            assert!(r.contains("val w = v"), "{}", r);
        }
    }
}

#[test]
fn no_cascade_optfield_payload() {
    // `?.` on a non-struct payload used to append a println E0201.
    assert_eq!(
        check_codes("val o = Some(1 as Int64)\nprintln(o?.nosuch)\n"),
        vec!["E0201"]
    );
}

#[test]
fn no_cascade_try_mismatch() {
    assert_eq!(
        check_codes("val o: Option<Int64> = None\nprintln(o?)\n"),
        vec!["E0702"]
    );
}

#[test]
fn no_cascade_try_init() {
    // Init and println follow-ons are gone; only the E0702 remains.
    assert_eq!(
        check_codes("val o: Option<Int64> = None\nval y = o?\nprintln(y)\n"),
        vec!["E0702"]
    );
}

#[test]
fn no_cascade_vec_empty_no_context() {
    assert_eq!(check_codes("val v = vec![]\nprintln(v)\n"), vec!["E0201"]);
}

#[test]
fn no_cascade_none_no_context() {
    assert_eq!(check_codes("val x = None\n"), vec!["E0201"]);
}

#[test]
fn no_cascade_if_no_else_value() {
    // `if` as a value without `else` no longer cascades into the binding.
    assert_eq!(
        check_codes("val y = if (true) { 1 as Int64 }\nprintln(y)\n"),
        vec!["E0201"]
    );
}

#[test]
fn broken_top_level_statement_errors() {
    // A top-level statement that fails to parse must diagnose (used to
    // vanish silently and continue with later lines).
    assert_eq!(check_codes("println(o ??)\n"), vec!["E0005"]);
}

#[test]
fn sugar_unless_until_accepted() {
    check_ok("unless (false) {\nprintln(1)\n}\nvar i = 0\nuntil (i == 1) {\ni = i + 1\n}\n");
}

#[test]
fn sugar_unless_let_rejected() {
    assert_eq!(
        check_codes("val o = Some(1 as Int64)\nunless let Some(x) = o {\nprintln(x)\n}\n"),
        vec!["E0005"]
    );
}

#[test]
fn sugar_postfix_if_accepted() {
    check_ok("val ok = true\nprintln(1) if ok\nprintln(2) unless ok\n");
}

#[test]
fn sugar_postfix_decl_rejected() {
    // Postfix conditions apply to action statements, not declarations.
    assert_eq!(check_codes("val c = true\nval y = 1 if c\n"), vec!["E0004"]);
}

#[test]
fn sugar_postfix_next_line_is_new_statement() {
    // A newline before `if` means two statements, not postfix.
    check_ok("val ok = true\nprintln(1)\nif (ok) {\nprintln(2)\n}\n");
}

#[test]
fn sugar_println_multi_accepted() {
    check_ok("println(1, \"a\", true)\n");
}

#[test]
fn sugar_index_from_end_accepted() {
    check_ok("val v = vec![1 as Int64]\nprintln(v[^1])\n");
}

#[test]
fn sugar_coalesce_some_none() {
    check_ok("val o = Some(1 as Int64)\nprintln(o ?? 2 as Int64)\nval n: Option<Int64> = None\nprintln(n ?? 2 as Int64)\n");
}

#[test]
fn sugar_coalesce_int_rejected() {
    // `??` on a non-Option is one match-scrutinee error, nothing more.
    assert_eq!(check_codes("println(5 ?? 6)\n"), vec!["E0201"]);
}

#[test]
fn sugar_chained_cmp_accepted() {
    check_ok("val x = 5 as Int64\nprintln((0 as Int64) < x < (10 as Int64))\nprintln(1 < 2 < 3)\n");
}

#[test]
fn sugar_while_let_accepted() {
    check_ok("var vs = vec![1 as Int64]\nwhile let Some(x) = vs.pop() {\nprintln(x)\n}\n");
}

#[test]
fn sugar_until_let_rejected() {
    assert_eq!(
        check_codes("var vs = vec![1 as Int64]\nuntil let Some(x) = vs.pop() {\nprintln(x)\n}\n"),
        vec!["E0005"]
    );
}

#[test]
fn sugar_if_let_chain_accepted() {
    check_ok("val o = Some(5 as Int64)\nif let Some(x) = o && x > (3 as Int64) {\nprintln(x)\n} else {\nprintln(0)\n}\n");
}

#[test]
fn sugar_if_let_double_chain_accepted() {
    check_ok("val a = Some(1 as Int64)\nval b = Some(2 as Int64)\nif let Some(x) = a && let Some(y) = b {\nprintln(x + y)\n} else {\nprintln(0)\n}\n");
}

#[test]
fn sugar_iflet_or_needs_parens() {
    assert_eq!(
        check_codes("val o = Some(1 as Int64)\nif let Some(x) = o || true {\nprintln(x)\n}\n"),
        vec!["E0005"]
    );
}

#[test]
fn sugar_let_else_accepted() {
    check_ok("fun get(v: Int64): Option<Int64> {\nif (v > (0 as Int64)) {\nreturn Some(v)\n}\nreturn None\n}\nfun show(v: Int64) {\nlet Some(x) = get(v) else {\nprintln(0)\nreturn\n};\nprintln(x)\n}\nfun main() {\nshow(5 as Int64)\n}\n");
}

#[test]
fn sugar_let_else_nodiv_rejected() {
    // One E0201 for the non-diverging else, nothing more.
    assert_eq!(
        check_codes("fun main() {\nval o = Some(3 as Int64)\nlet Some(x) = o else {\nprintln(0)\n}\nprintln(x)\n}\n"),
        vec!["E0201"]
    );
}

#[test]
fn sugar_suffix_pins_types() {
    check_ok("val x = 1i64\nval y: Int64 = x\nval a = 5i32\nval f = 2.5f64\nprintln(y)\nprintln(a)\nprintln(f)\n");
}

#[test]
fn sugar_suffix_bad_rejected() {
    assert_eq!(check_codes("val x = 1u8\n"), vec!["E0010"]);
}

#[test]
fn sugar_suffix_range_rejected() {
    assert_eq!(check_codes("val x = 99999999999i32\n"), vec!["E0201"]);
}

#[test]
fn sugar_ufcs_accepted() {
    check_ok("fun double(x: Int64): Int64 {\nx * (2 as Int64)\n}\nfun main() {\nval v = 21 as Int64\nprintln(v.double())\n}\n");
}

#[test]
fn sugar_ufcs_moved_rejected() {
    // The receiver is a real first argument: moving it flags later uses.
    assert_eq!(
        check_codes("struct Wrap {\ns: String,\n}\nfun eat(w: Wrap): Int64 {\nprintln(w.s)\n1 as Int64\n}\nfun main() {\nval w = Wrap { s: \"hi\" }\nprintln(w.eat())\nprintln(w.eat())\n}\n"),
        vec!["E0301"]
    );
}

#[test]
fn sugar_ufcs_vec_method_wins() {
    // Real methods beat same-named free functions: still the Vec error.
    assert_eq!(
        check_codes("fun first(v: Vec<Int64>): Int64 {\nv[0]\n}\nfun main() {\nval v = vec![7 as Int64]\nprintln(v.first())\n}\n"),
        vec!["E0401"]
    );
}

#[test]
fn sugar_multi_assign_accepted() {
    check_ok("var a = 1\nvar b = 2\na, b = b, a\nprintln(a, b)\n");
}

#[test]
fn sugar_multi_arity_rejected() {
    assert_eq!(
        check_codes("var a = 1\nvar b = 2\na, b = b\n"),
        vec!["E0005"]
    );
}

#[test]
fn sugar_interp_accepted() {
    check_ok("val name = \"world\"\nval n = 42\nprintln(\"hello $name, n=$n!\")\nprintln(\"${n + 1}\")\nprintln(\"braces {ok} and price $5\")\n");
}

#[test]
fn sugar_interp_unknown_var_single_error() {
    // Poison flows through concat without follow-ons.
    assert_eq!(check_codes("println(\"a${nope}b\")\n"), vec!["E0401"]);
}

#[test]
fn sugar_interp_adjacent_strings_still_error() {
    assert_eq!(check_codes("println(\"a\" \"b\")\n"), vec!["E0005"]);
}

#[test]
fn exp_denies_print_alias() {
    assert_eq!(
        check_codes("mode exp;\nfun main() {\nprint(\"hi\")\n}\n"),
        vec!["W1002"]
    );
}

#[test]
fn imp_warns_print_alias_only() {
    // Same program without the header stays well-formed (warning only).
    check_ok("fun main() {\nprint(\"hi\")\n}\n");
}

#[test]
fn lint_needless_cast() {
    assert_eq!(
        check_codes("mode exp;\nfun main() {\nval x = 5 as Int32\nprintln(x)\n}\n"),
        vec!["W1003"]
    );
}

#[test]
fn lint_shadowing() {
    assert_eq!(
        check_codes("mode exp;\nfun main() {\nval x = 1\nif (true) {\nval x = 2\nprintln(x)\n}\nprintln(x)\n}\n"),
        vec!["W1004"]
    );
}

#[test]
fn script_args_implicit() {
    check_ok("println(args.len())\nprintln(args)\n");
}

#[test]
fn script_args_user_binding_wins() {
    check_ok("val args = vec![\"mine\"]\nprintln(args)\n");
}

#[test]
fn script_args_absent_in_exp() {
    assert_eq!(
        check_codes("mode exp;\nfun main() {\nprintln(args)\n}\n"),
        vec!["E0401"]
    );
}

#[test]
fn repl_echo_bare_expr() {
    assert!(pipeline::is_bare_expr("1 + 2"));
    assert!(pipeline::is_bare_expr("println(\"hi\")"));
    assert!(pipeline::is_bare_expr("  x  "));
}

#[test]
fn repl_echo_rejects_statements() {
    assert!(!pipeline::is_bare_expr("val x = 1"));
    assert!(!pipeline::is_bare_expr("1; 2"));
    assert!(!pipeline::is_bare_expr("nope("));
    assert!(!pipeline::is_bare_expr(""));
}

#[test]
fn sys_run_accepted_imp() {
    check_ok("fun main() {\nprintln(sys_run(\"true\"))\nprintln(sys_capture(\"echo hi\"))\n}\n");
}

#[test]
fn sys_run_rejected_exp() {
    assert_eq!(
        check_codes("mode exp;\nfun main() {\nprintln(sys_run(\"true\"))\n}\n"),
        vec!["E0401"]
    );
}

#[test]
fn sys_run_arity_checked() {
    assert_eq!(
        check_codes("fun main() {\nprintln(sys_run(\"a\", \"b\"))\n}\n"),
        vec!["E0201"]
    );
}

#[test]
fn sys_getenv_option() {
    check_ok("fun main() {\nval v = sys_getenv(\"HOME\")\nif let None = v {\nprintln(0)\n} else {\nprintln(1)\n}\n}\n");
}

#[test]
fn prelude_header_parses() {
    let s = src("prelude;\nprintln(1)\n");
    let prog = pipeline::parse_unit(&s).expect("parses");
    assert!(prog.prelude);
}

#[test]
fn prelude_header_either_order() {
    let s = src("prelude;\nmode imp;\nprintln(1)\n");
    let prog = pipeline::parse_unit(&s).expect("parses");
    assert!(prog.prelude);
    let s = src("mode imp;\nprelude;\nprintln(1)\n");
    let prog = pipeline::parse_unit(&s).expect("parses");
    assert!(prog.prelude);
}

#[test]
fn prelude_bare_word_not_header() {
    // `prelude` without `;` stays an ordinary identifier.
    check_ok("val prelude = 1\nprintln(prelude)\n");
}

#[test]
fn prelude_single_source_rejected() {
    assert_eq!(check_codes("prelude;\nprintln(1)\n"), vec!["E0604"]);
}

#[test]
fn seq_accepted() {
    check_ok("println(seq(1, 5, 1))\nprintln(seq(5, 1, -2))\n");
}

#[test]
fn seq_arity_rejected() {
    assert_eq!(check_codes("println(seq(1, 5))\n"), vec!["E0201"]);
}

#[test]
fn arith_literal_pins_int() {
    check_ok("val x = 2 as Int64\nprintln(1 + x)\nprintln(x * 3)\n");
}

#[test]
fn arith_literal_pins_float() {
    check_ok("println(1 + 2.0)\n");
}

#[test]
fn arith_literal_inexact_rejected() {
    // Beyond 2^53 the int-to-float pin would lie: still E0201.
    assert_eq!(
        check_codes("println(99999999999999999 + 1.0)\n"),
        vec!["E0201"]
    );
}

#[test]
fn arith_suffixed_literal_rejected() {
    assert_eq!(check_codes("println(1i32 + (2 as Int64))\n"), vec!["E0201"]);
}

#[test]
fn arith_mixed_vars_rejected() {
    assert_eq!(
        check_codes("val a = 1 as Int32\nval b = 2 as Int64\nprintln(a + b)\n"),
        vec!["E0201"]
    );
}

#[test]
fn vec_recycle_runs() {
    // Runtime behaviour is fixture-checked; here just well-formedness.
    check_ok("fun main() {\nval a = vec![1 as Int64, 2 as Int64]\nval b = vec![10 as Int64]\nlet Ok(v) = a.clone().add(b.clone()) else {\nreturn\n};\nprintln(v)\n}\n");
}

#[test]
fn is_in_accepted() {
    check_ok("println(is_in(vec![1 as Int64], vec![2 as Int64]))\n");
}

#[test]
fn is_in_mismatch_rejected() {
    assert_eq!(
        check_codes("println(is_in(vec![1 as Int64], vec![\"a\"]))\n"),
        vec!["E0201"]
    );
}

#[test]
fn mat_outer_kron_accepted() {
    check_ok("println(mat_outer(vec![1.0], vec![2.0]))\nprintln(mat_kron(mat_identity(1 as Int64), mat_identity(1 as Int64)))\n");
}

#[test]
fn comprehension_accepted() {
    check_ok("fun main() {\nval v = vec![1 as Int64]\nval w: Vec<Int64> = [x + (1 as Int64) for x in v]\nprintln(w)\n}\n");
}

#[test]
fn comprehension_filtered_accepted() {
    check_ok("fun main() {\nval v = vec![1 as Int64, 2 as Int64]\nval w: Vec<Int64> = [x for x in v if x > (1 as Int64)]\nprintln(w)\n}\n");
}

#[test]
fn comprehension_needs_annotation() {
    assert_eq!(
        check_codes(
            "fun main() {\nval v = vec![1 as Int64]\nval w = [x for x in v]\nprintln(w)\n}\n"
        ),
        vec!["E0201"]
    );
}

#[test]
fn par_map_accepted() {
    check_ok("fun double(x: Float64): Float64 {\nx * 2.0\n}\nfun main() {\nval v = vec![1.0]\nprintln(par_map(v.clone(), double, 1 as Int64))\n}\n");
}

#[test]
fn par_map_arity_rejected() {
    assert_eq!(
        check_codes("fun double(x: Float64): Float64 {\nx * 2.0\n}\nfun main() {\nval v = vec![1.0]\nprintln(par_map(v.clone(), double))\n}\n"),
        vec!["E0201"]
    );
}

#[test]
fn match_guard_call_accepted() {
    check_ok("fun p(s: String): Bool {\nreturn s.len() > (1 as Int64);\n}\nfun main() {\nprintln(match (\"ab\") { s if p(s) -> 1 as Int64, _ -> 0 as Int64, })\n}\n");
}

#[test]
fn broadcast_accepted() {
    check_ok("fun double(x: Float64): Float64 {\nx * 2.0\n}\nfun main() {\nval v = vec![1.0]\nprintln(double.(v))\n}\n");
}

#[test]
fn broadcast_arity_rejected() {
    // Two E0005s: the arity error plus the systemic recovery cascade for
    // abortive in-function parses (same as unclosed calls/indices).
    assert_eq!(
        check_codes("fun double(x: Float64): Float64 {\nx * 2.0\n}\nfun main() {\nval v = vec![1.0]\nprintln(double.(v, v))\n}\n"),
        vec!["E0005", "E0005"]
    );
}

#[test]
fn str_unicode_accepted() {
    check_ok("fun main() {\nprintln(\"hello\".nchars())\nprintln(\"HeLLo\".to_upper())\nprintln(\"HeLLo\".to_lower())\n}\n");
}

#[test]
fn trig_builtins_accepted() {
    check_ok("fun main() {\nprintln(sin(1.0))\nprintln(cos(1.0))\nprintln(atan2(1.0, 2.0))\n}\n");
}

#[test]
fn trig_builtins_reject_int() {
    assert_eq!(
        check_codes("fun main() {\nprintln(sin(1))\n}\n"),
        vec!["E0201"]
    );
}

#[test]
fn overload_arity_accepted() {
    check_ok("fun add(a: Int64, b: Int64): Int64 {\na + b\n}\nfun add(a: Int64): Int64 {\na * (2 as Int64)\n}\nfun main() {\nprintln(add(1 as Int64, 2 as Int64))\nprintln(add(21 as Int64))\n}\n");
}

#[test]
fn overload_arities_listed() {
    assert_eq!(
        check_codes("fun add(a: Int64, b: Int64): Int64 {\na + b\n}\nfun main() {\nprintln(add(1 as Int64))\n}\n"),
        vec!["E0201"]
    );
}

#[test]
fn overload_duplicate_rejected() {
    assert_eq!(
        check_codes("fun dup(x: Int64): Int64 {\nx\n}\nfun dup(x: Int64): Int64 {\nx\n}\nfun main() {\nprintln(1)\n}\n"),
        vec!["E0401"]
    );
}

#[test]
fn overload_dispatch_and_errors() {
    // Most specific wins; identical signatures stay duplicates; genuine
    // ties report one E0201.
    check_ok("fun f(x: Int32): Int32 {\nreturn x\n}\nfun f(x: Float64): Float64 {\nreturn x\n}\nfun main() {\nprintln(f(1))\n}\n");
    assert_eq!(
        check_codes("fun f(x: Int32): Int32 {\nreturn x\n}\nfun f(x: Int32): Int32 {\nreturn x\n}\nfun main() {\nprintln(1)\n}\n"),
        vec!["E0401"]
    );
    assert_eq!(
        check_codes("fun f(x: Int32): Int32 {\nreturn x\n}\nfun f(x: Int64): Int64 {\nreturn x\n}\nfun main() {\nval v: Vec<Int32> = vec![1]\nprintln(f(v))\n}\n"),
        vec!["E0201"]
    );
}
