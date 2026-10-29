use valvet_core::{codegen, pipeline, source::Source};

fn lower(text: &str) -> String {
    let s = Source::new("test.valvet".to_string(), text.to_string());
    let checked = pipeline::front_end(&s).expect("front end");
    codegen::lower(&checked).expect("lower")
}

#[test]
fn sqrt_builtin_is_guarded_and_emitted_at_call_site() {
    let c = lower("fun main() { println(sqrt(4.0)) }\n");
    assert!(
        c.contains("static double vsqrt_builtin(double x)"),
        "guarded sqrt helper must be in the prelude"
    );
    assert!(
        c.matches("vsqrt_builtin(").count() >= 2,
        "call site must use the helper, not only define it"
    );
    assert!(
        c.contains("if (x < 0.0) return 0.0 / 0.0;"),
        "negative inputs must produce NaN"
    );
}

#[test]
fn floating_point_flags_are_explicit() {
    let dev = codegen::cc_flags("dev", false);
    let release = codegen::cc_flags("release", false);
    assert!(dev.contains(&"-ffp-contract=off".to_string()));
    assert!(release.contains(&"-ffp-contract=off".to_string()));
    assert!(dev.contains(&"-fno-math-errno".to_string()));
    assert!(release.contains(&"-fno-math-errno".to_string()));
    assert!(!dev.iter().any(|f| f == "-ffast-math"));
    assert!(!release.iter().any(|f| f == "-ffast-math"));
}

#[test]
fn vec_literal_and_repeat_allocate_once_without_push() {
    let literal = lower("fun main() { val a = vec![1, 2, 3]; println(a.len()) }\n");
    let main = literal.split("int main(").nth(1).expect("main");
    assert_eq!(
        main.matches("valloc_or_abort((size_t)3 * sizeof(int32_t))")
            .count(),
        1,
        "literal must allocate once: {main}"
    );
    assert!(!main.contains("_push(&"), "literal must not push: {main}");

    let repeat = lower("fun main() { val a = vec![2.5; 4]; println(a.len()) }\n");
    let main = repeat.split("int main(").nth(1).expect("main");
    assert_eq!(
        main.matches("valloc_or_abort((size_t)4 * sizeof(double))")
            .count(),
        1,
        "repeat must allocate once: {main}"
    );
    assert!(!main.contains("_push(&"), "repeat must not push: {main}");
    assert!(main.contains("for (int64_t _vri = 0; _vri < 4; _vri++)"));

    let empty = lower("fun main() { val a: Vec<Int32> = vec![]; println(a.len()) }\n");
    let main = empty.split("int main(").nth(1).expect("main");
    assert!(
        !main.contains("valloc_or_abort"),
        "empty vec must not allocate"
    );
}

#[test]
fn vec_capacity_and_owned_ops_lower_to_kernels() {
    let c = lower(
        "fun main() {\n\
         val a = Vec<Float64>::with_capacity(3 as Int64);\n\
         a.reserve(2 as Int64);\n\
         a.resize(4 as Int64, 1.0);\n\
         a.fill(2.0);\n\
         val b = vec![3.0, 4.0];\n\
         a.copy_from(b);\n\
         println(a.capacity());\n\
         }\n",
    );
    assert!(c.contains("vvec_Vec_Float64_with_capacity((int64_t)"));
    assert!(c.contains("vvec_Vec_Float64_reserve("));
    assert!(c.contains("vvec_Vec_Float64_resize("));
    assert!(c.contains("vvec_Vec_Float64_fill("));
    assert!(c.contains("vvec_Vec_Float64_copy_from("));
    assert!(
        c.contains("static void vvec_Vec_Float64_copy_from(VVec_Float64 *v, VVec_Float64 *src)")
    );
}

#[test]
fn write_uses_formatters_without_newline() {
    let c = lower("fun main() { write(\"a\"); write(1) }\n");
    let main = c.split("int main(").nth(1).expect("main");
    assert!(main.contains("vfmt_str("));
    assert!(main.contains("vfmt_i32("));
    assert!(
        !main.contains("vprint_"),
        "write must not call vprint_*: {main}"
    );
}

#[test]
fn dev_profile_garbage_collects_unused_sections_only() {
    let dev = codegen::cc_flags("dev", false);
    assert!(dev.contains(&"-ffunction-sections".to_string()));
    assert!(dev.contains(&"-fdata-sections".to_string()));
    assert!(dev.contains(&"-Wl,--gc-sections".to_string()));
    let release = codegen::cc_flags("release", false);
    assert!(!release.contains(&"-ffunction-sections".to_string()));
    assert!(!release.contains(&"-fdata-sections".to_string()));
    assert!(!release.contains(&"-Wl,--gc-sections".to_string()));
}

#[test]
fn string_literals_preserve_nul_and_escape_trigraphs() {
    let c = lower("fun main() { println(\"a\\0b\"); println(\"??/\") }\n");
    assert!(
        c.contains("vstr_from_bytes(\"a\\000b\", 3)"),
        "NUL must be a fixed-width octal escape and keep its byte length: {c}"
    );
    assert!(
        c.contains("vstr_from_bytes(\"\\?\\?/\", 3)"),
        "question marks must be escaped against trigraph replacement: {c}"
    );
    assert!(
        !c.contains("\\x00"),
        "greedy hex escapes must not be emitted"
    );
}

#[test]
fn while_condition_temporaries_are_freed_before_break() {
    let c = lower(
        "fun main() {\n\
         var i = 0 as Int64;\n\
         while (i < vec![\"a\".clone()].len()) {\n\
         i = i + (1 as Int64);\n\
         }\n\
         }\n",
    );
    let main = c.split("int main(").nth(1).expect("main");
    let drop_at = main
        .find("v_drop_Vec_String(&_vt")
        .expect("condition temporary must be dropped");
    let break_at = main.find("if (!(_vt").expect("while condition break");
    assert!(
        drop_at < break_at,
        "condition temporary must be freed before break: {main}"
    );
}

#[test]
fn matrix_shapes_use_checked_helpers() {
    let c = lower("fun main() { val m = mat_zeros(2, 2); println(m.rows()) }\n");
    assert!(c.contains("#define VMAT_MAX_ELEMENTS"));
    assert!(c.contains("vmat_checked_mul"));
    assert!(c.contains("vmat_checked_elems"));
    assert!(c.contains("vmat_alloc("));
    assert!(
        !c.contains("int64_t n = r * c;"),
        "matrix constructors must not multiply dimensions directly"
    );
}
