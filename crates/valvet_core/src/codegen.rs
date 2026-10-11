use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::ast::*;
use crate::ast::{EnumDef, StructDef};
use crate::diag::Ice;
use crate::sema::{is_copy_ty, Checked};

/// C11 runtime prelude: owned strings without GC, deterministic free on NULL-check.
const PRELUDE: &str = r#"
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <inttypes.h>
#include <math.h>

static int32_t v_f2i32(double v) {
    if (isnan(v)) return 0;
    if (v <= -2147483648.0) return INT32_MIN;
    if (v >= 2147483648.0) return INT32_MAX;
    return (int32_t)v;
}
static int64_t v_f2i64(double v) {
    if (isnan(v)) return 0;
    if (v <= -9223372036854775808.0) return INT64_MIN;
    if (v >= 9223372036854775808.0) return INT64_MAX;
    return (int64_t)v;
}

typedef struct { char *s; int64_t len; } VStr;

static VStr vstr_from_c(const char *c) {
    size_t n = strlen(c);
    char *p = (char *)malloc(n + 1);
    if (!p) { fprintf(stderr, "alloc error\n"); abort(); }
    memcpy(p, c, n + 1);
    return (VStr){p, (int64_t)n};
}
static void vstr_free(VStr *v) {
    if (v->s) { free(v->s); v->s = NULL; v->len = 0; }
}
static VStr vstr_clone(VStr v) {
    if (!v.s) return (VStr){NULL, 0};
    char *p = (char *)malloc((size_t)v.len + 1);
    if (!p) { fprintf(stderr, "alloc error\n"); abort(); }
    memcpy(p, v.s, (size_t)v.len + 1);
    return (VStr){p, v.len};
}
static VStr vstr_concat(VStr a, VStr b) {
    int64_t alen = a.s ? a.len : 0, blen = b.s ? b.len : 0;
    char *p = (char *)malloc((size_t)(alen + blen) + 1);
    if (!p) { fprintf(stderr, "alloc error\n"); abort(); }
    if (alen) memcpy(p, a.s, (size_t)alen);
    if (blen) memcpy(p + alen, b.s, (size_t)blen);
    p[alen + blen] = '\0';
    return (VStr){p, alen + blen};
}
static int vstr_eq(VStr a, VStr b) {
    int64_t alen = a.s ? a.len : 0, blen = b.s ? b.len : 0;
    if (alen != blen) return 0;
    if (alen == 0) return 1;
    return memcmp(a.s, b.s, (size_t)alen) == 0;
}
/// J1 conversions + string kernels (all allocate fresh owned values).
static VStr vstr_from_i32(int32_t v) {
    char buf[32];
    snprintf(buf, sizeof buf, "%" PRId32, v);
    return vstr_from_c(buf);
}
static VStr vstr_from_i64(int64_t v) {
    char buf[32];
    snprintf(buf, sizeof buf, "%" PRId64, v);
    return vstr_from_c(buf);
}
static VStr vstr_from_f64(double v) {
    char buf[32];
    snprintf(buf, sizeof buf, "%.17g", v);
    return vstr_from_c(buf);
}
static VStr vstr_from_bool(int v) {
    return vstr_from_c(v ? "true" : "false");
}
static VStr vstr_trim(VStr v) {
    int64_t a = 0, b = v.s ? v.len : 0;
    while (a < b && (v.s[a] == ' ' || v.s[a] == '\t' || v.s[a] == '\n' || v.s[a] == '\r')) a++;
    while (b > a && (v.s[b - 1] == ' ' || v.s[b - 1] == '\t' || v.s[b - 1] == '\n' || v.s[b - 1] == '\r')) b--;
    char *p = (char *)malloc((size_t)(b - a) + 1);
    if (!p) { fprintf(stderr, "alloc error\n"); abort(); }
    if (b > a) memcpy(p, v.s + a, (size_t)(b - a));
    p[b - a] = '\0';
    return (VStr){p, b - a};
}
/// Element formatters (no trailing newline; composite `vfmt_` helpers call
/// these per element, and top-level `vprint_` wraps them with `"\n"`).
static void vfmt_i32(int32_t v) { printf("%" PRId32, v); }
static void vfmt_i64(int64_t v) { printf("%" PRId64, v); }
static void vfmt_f64(double v) { printf("%.17g", v); }
static void vfmt_bool(int v) { printf("%s", v ? "true" : "false"); }
static void vfmt_str(VStr v) {
    if (v.s) fwrite(v.s, 1, (size_t)v.len, stdout);
}
static void vprint_i32(int32_t v) { vfmt_i32(v); printf("\n"); }
static void vprint_i64(int64_t v) { vfmt_i64(v); printf("\n"); }
static void vprint_f64(double v) { vfmt_f64(v); printf("\n"); }
static void vprint_bool(int v) { vfmt_bool(v); printf("\n"); }
static void vprint_str(VStr v) { vfmt_str(v); printf("\n"); }
"#;

/// J2 file + CSV/JSON kernels. Row buffers use plain layouts identical to
/// the generated `VVec_String` / `VVec_Vec_String` structs, so the typed
/// wrappers (emitted post-layout) convert by field copy, no element traffic.
const PRELUDE_J2: &str = r##"
typedef struct { VStr *fields; int64_t nfields, capfields; } VCsvRow;
typedef struct { VCsvRow *rows; int64_t nrows, caprows; } VCsvTable;
typedef struct { int64_t *rows; int64_t n, cap; } VBadRows;
typedef struct { VStr text; VStr err; int has_err; } VFileOut;
static void vcsv_row_push_field(VCsvRow *r, char *p, int64_t len) {
    if (r->nfields == r->capfields) {
        int64_t ncap = r->capfields ? r->capfields * 2 : 4;
        VStr *nd = (VStr *)realloc(r->fields, (size_t)ncap * sizeof(VStr));
        if (!nd) { fprintf(stderr, "alloc error\n"); abort(); }
        r->fields = nd; r->capfields = ncap;
    }
    r->fields[r->nfields++] = (VStr){p, len};
}
static void vcsv_table_push_row(VCsvTable *t, VCsvRow r) {
    if (t->nrows == t->caprows) {
        int64_t ncap = t->caprows ? t->caprows * 2 : 4;
        VCsvRow *nd = (VCsvRow *)realloc(t->rows, (size_t)ncap * sizeof(VCsvRow));
        if (!nd) { fprintf(stderr, "alloc error\n"); abort(); }
        t->rows = nd; t->caprows = ncap;
    }
    t->rows[t->nrows++] = r;
}
static void vbad_push(VBadRows *b, int64_t r) {
    if (b->n == b->cap) {
        int64_t ncap = b->cap ? b->cap * 2 : 4;
        int64_t *nd = (int64_t *)realloc(b->rows, (size_t)ncap * sizeof(int64_t));
        if (!nd) { fprintf(stderr, "alloc error\n"); abort(); }
        b->rows = nd; b->cap = ncap;
    }
    b->rows[b->n++] = r;
}
static void vcsv_free_row(VCsvRow *r) {
    for (int64_t j = 0; j < r->nfields; j++) free(r->fields[j].s);
    free(r->fields); r->fields = NULL; r->nfields = r->capfields = 0;
}
static void vcsv_free_table(VCsvTable *t) {
    for (int64_t i = 0; i < t->nrows; i++) {
        VCsvRow *r = &t->rows[i];
        for (int64_t j = 0; j < r->nfields; j++) free(r->fields[j].s);
        free(r->fields);
    }
    free(t->rows); t->rows = NULL; t->nrows = t->caprows = 0;
}
static VFileOut v_read_file(VStr path) {
    VFileOut o = {{NULL, 0}, {NULL, 0}, 0};
    if (!path.s || path.len == 0) { o.err = vstr_from_c("empty path"); o.has_err = 1; return o; }
    FILE *f = fopen(path.s, "rb");
    if (!f) {
        VStr pre = vstr_from_c("cannot open file: ");
        o.err = vstr_concat(pre, path);
        vstr_free(&pre);
        o.has_err = 1;
        return o;
    }
    if (fseek(f, 0, SEEK_END) != 0) { fclose(f); o.err = vstr_from_c("cannot seek file"); o.has_err = 1; return o; }
    long n = ftell(f);
    if (n < 0) { fclose(f); o.err = vstr_from_c("cannot size file"); o.has_err = 1; return o; }
    rewind(f);
    char *p = (char *)malloc((size_t)n + 1);
    if (!p) { fclose(f); fprintf(stderr, "alloc error\n"); abort(); }
    size_t got = fread(p, 1, (size_t)n, f);
    fclose(f);
    p[got] = '\0';
    o.text = (VStr){p, (int64_t)got};
    return o;
}
static void vcsv_buf_put(char **buf, int64_t *bcap, int64_t *blen, char c) {
    if (*blen + 1 >= *bcap) {
        *bcap *= 2;
        char *nb = (char *)realloc(*buf, (size_t)*bcap);
        if (!nb) { fprintf(stderr, "alloc error\n"); abort(); }
        *buf = nb;
    }
    (*buf)[(*blen)++] = c;
}
static VStr vcsv_buf_take(char **buf, int64_t *blen) {
    char *p = (char *)malloc((size_t)*blen + 1);
    if (!p) { fprintf(stderr, "alloc error\n"); abort(); }
    if (*blen) memcpy(p, *buf, (size_t)*blen);
    p[*blen] = '\0';
    VStr f = {p, *blen};
    *blen = 0;
    return f;
}
static VStr vcsv_err(int64_t rec_line, const char *reason) {
    char num[32];
    snprintf(num, sizeof num, "%lld", (long long)rec_line);
    VStr a = vstr_from_c("row ");
    VStr b = vstr_from_c(num);
    VStr c = vstr_from_c(": ");
    VStr d = vstr_from_c(reason);
    VStr ab = vstr_concat(a, b);
    VStr cd = vstr_concat(c, d);
    VStr m = vstr_concat(ab, cd);
    vstr_free(&a); vstr_free(&b); vstr_free(&c);
    vstr_free(&d); vstr_free(&ab); vstr_free(&cd);
    return m;
}
// RFC 4180 parse. mode 0 = strict (stop at first bad record, report);
// mode 1 = lenient (drop the bad record, log its line, resync past it).
// Returns 0 when input is consumed; strict-malformed returns the record
// start line with *emsg set (caller frees the partial table + buffer).
static int64_t vcsv_run(VCsvTable *t, VBadRows *bad, const char *s, int64_t n, int mode, VStr *emsg) {
    int64_t bcap = 64;
    char *buf = (char *)malloc((size_t)bcap);
    if (!buf) { fprintf(stderr, "alloc error\n"); abort(); }
    int64_t blen = 0;
    VCsvRow row = {NULL, 0, 0};
    int in_q = 0, started = 0, closed = 0, row_has = 0;
    int64_t line = 1, rec_line = 1;
    int64_t i = 0;
    int done = 0;
    while (!done) {
        char c = (i < n) ? s[i] : '\0';
        int at_end = (i >= n);
        if (in_q) {
            if (c == '"') {
                if (!at_end && i + 1 < n && s[i + 1] == '"') {
                    vcsv_buf_put(&buf, &bcap, &blen, '"');
                    i += 2;
                } else { in_q = 0; closed = 1; i++; }
            } else if (at_end) {
                if (mode == 0) {
                    vcsv_free_row(&row); vcsv_free_table(t); free(buf);
                    *emsg = vcsv_err(rec_line, "unterminated quoted field");
                    return rec_line;
                }
                if (bad) vbad_push(bad, rec_line);
                vcsv_free_row(&row); free(buf);
                return 0;
            } else {
                vcsv_buf_put(&buf, &bcap, &blen, c);
                if (c == '\n') line++;
                i++;
            }
        } else if (c == '"') {
            if (started) goto malformed_stray;
            in_q = 1; started = 1; i++;
        } else if (c == ',') {
            VStr f = vcsv_buf_take(&buf, &blen);
            vcsv_row_push_field(&row, f.s, f.len);
            started = 0; closed = 0; row_has = 1; i++;
        } else if (c == '\n' || c == '\r' || at_end) {
            if (at_end && !started && blen == 0 && !row_has) { done = 1; }
            else {
                VStr f = vcsv_buf_take(&buf, &blen);
                vcsv_row_push_field(&row, f.s, f.len);
                vcsv_table_push_row(t, row);
                row.fields = NULL; row.nfields = row.capfields = 0;
                started = 0; closed = 0; row_has = 0;
                if (at_end) { done = 1; }
                else {
                    if (c == '\r' && i + 1 < n && s[i + 1] == '\n') i++;
                    line++; rec_line = line; i++;
                }
            }
        } else {
            if (closed) goto malformed_after;
            vcsv_buf_put(&buf, &bcap, &blen, c);
            started = 1; i++;
        }
        continue;
malformed_stray:
        if (mode == 0) {
            vcsv_free_row(&row); vcsv_free_table(t); free(buf);
            *emsg = vcsv_err(rec_line, "stray quote in unquoted field");
            return rec_line;
        }
        goto skip_record;
malformed_after:
        if (mode == 0) {
            vcsv_free_row(&row); vcsv_free_table(t); free(buf);
            *emsg = vcsv_err(rec_line, "unexpected character after closing quote");
            return rec_line;
        }
skip_record:
        if (bad) vbad_push(bad, rec_line);
        vcsv_free_row(&row);
        row.fields = NULL; row.nfields = row.capfields = 0;
        blen = 0; in_q = 0; started = 0; closed = 0; row_has = 0;
        while (i < n && s[i] != '\n') i++;
        if (i < n) { i++; line++; }
        rec_line = line;
    }
    free(buf);
    return 0;
}
static void vjson_skip_ws(const char *s, int64_t n, int64_t *i, int64_t *line) {
    while (*i < n && (s[*i] == ' ' || s[*i] == '\t' || s[*i] == '\n' || s[*i] == '\r')) {
        if (s[*i] == '\n') (*line)++;
        (*i)++;
    }
}
static VStr vjson_err(int64_t off, const char *reason) {
    char num[32];
    snprintf(num, sizeof num, "%lld", (long long)off);
    VStr a = vstr_from_c("offset ");
    VStr b = vstr_from_c(num);
    VStr c = vstr_from_c(": ");
    VStr d = vstr_from_c(reason);
    VStr ab = vstr_concat(a, b);
    VStr cd = vstr_concat(c, d);
    VStr m = vstr_concat(ab, cd);
    vstr_free(&a); vstr_free(&b); vstr_free(&c);
    vstr_free(&d); vstr_free(&ab); vstr_free(&cd);
    return m;
}
static void vjson_push_utf8(char **buf, int64_t *bcap, int64_t *blen, unsigned cp) {
    if (cp < 0x80) { vcsv_buf_put(buf, bcap, blen, (char)cp); }
    else if (cp < 0x800) {
        vcsv_buf_put(buf, bcap, blen, (char)(0xC0 | (cp >> 6)));
        vcsv_buf_put(buf, bcap, blen, (char)(0x80 | (cp & 0x3F)));
    } else if (cp < 0x10000) {
        vcsv_buf_put(buf, bcap, blen, (char)(0xE0 | (cp >> 12)));
        vcsv_buf_put(buf, bcap, blen, (char)(0x80 | ((cp >> 6) & 0x3F)));
        vcsv_buf_put(buf, bcap, blen, (char)(0x80 | (cp & 0x3F)));
    } else {
        vcsv_buf_put(buf, bcap, blen, (char)(0xF0 | (cp >> 18)));
        vcsv_buf_put(buf, bcap, blen, (char)(0x80 | ((cp >> 12) & 0x3F)));
        vcsv_buf_put(buf, bcap, blen, (char)(0x80 | ((cp >> 6) & 0x3F)));
        vcsv_buf_put(buf, bcap, blen, (char)(0x80 | (cp & 0x3F)));
    }
}
// One backslash escape at s[*i] (must be '\\'); advances past it.
// Returns 0 on malformed escape. Surrogate pairs combine; lone
// surrogates and out-of-range values become U+FFFD.
static int vjson_hex(const char *s, int64_t n, int64_t i, unsigned *out);
static int vjson_escape(char **buf, int64_t *bcap, int64_t *blen, const char *s, int64_t n, int64_t *i) {
    char e = s[*i + 1];
    if (e == '"' || e == '\\' || e == '/') { vcsv_buf_put(buf, bcap, blen, e); *i += 2; return 1; }
    if (e == 'n') { vcsv_buf_put(buf, bcap, blen, '\n'); *i += 2; return 1; }
    if (e == 't') { vcsv_buf_put(buf, bcap, blen, '\t'); *i += 2; return 1; }
    if (e == 'r') { vcsv_buf_put(buf, bcap, blen, '\r'); *i += 2; return 1; }
    if (e == 'b') { vcsv_buf_put(buf, bcap, blen, '\b'); *i += 2; return 1; }
    if (e == 'f') { vcsv_buf_put(buf, bcap, blen, '\f'); *i += 2; return 1; }
    if (e == 'u') {
        unsigned cp;
        if (!vjson_hex(s, n, *i + 2, &cp)) return 0;
        *i += 6;
        if (cp >= 0xD800 && cp <= 0xDBFF && *i + 1 < n && s[*i] == '\\' && s[*i + 1] == 'u') {
            unsigned lo;
            if (vjson_hex(s, n, *i + 2, &lo) && lo >= 0xDC00 && lo <= 0xDFFF) {
                cp = 0x10000 + ((cp - 0xD800) << 10) + (lo - 0xDC00);
                *i += 6;
            } else { cp = 0xFFFD; }
        } else if (cp >= 0xD800 && cp <= 0xDFFF) { cp = 0xFFFD; }
        if (cp > 0x10FFFF) cp = 0xFFFD;
        vjson_push_utf8(buf, bcap, blen, cp);
        return 1;
    }
    return 0;
}
static int vjson_hex(const char *s, int64_t n, int64_t i, unsigned *out) {
    if (i + 4 > n) return 0;
    unsigned v = 0;
    for (int k = 0; k < 4; k++) {
        char c = s[i + k];
        v *= 16;
        if (c >= '0' && c <= '9') v += (unsigned)(c - '0');
        else if (c >= 'a' && c <= 'f') v += (unsigned)(c - 'a' + 10);
        else if (c >= 'A' && c <= 'F') v += (unsigned)(c - 'A' + 10);
        else return 0;
    }
    *out = v;
    return 1;
}
// Flat JSON only: top-level array of flat objects with string/number/
// true/false/null values. Row 0 of the table is the key union in first-
// appearance order; missing keys become "". Returns 0 on success, else
// the byte offset+1 with *emsg set.
static int64_t vjson_parse(VCsvTable *t, const char *s, int64_t n, VStr *emsg) {
    int64_t bcap = 64;
    char *buf = (char *)malloc((size_t)bcap);
    if (!buf) { fprintf(stderr, "alloc error\n"); abort(); }
    int64_t blen = 0;
    VCsvRow keys = {NULL, 0, 0};
    int64_t line = 1;
    int64_t i = 0;
    vjson_skip_ws(s, n, &i, &line);
    if (i >= n || s[i] != '[') {
        free(buf);
        *emsg = vjson_err(i, "top level must be an array");
        return i + 1;
    }
    i++;
    int first = 1;
    VCsvRow cur = {NULL, 0, 0};
    int in_obj = 0;
    for (;;) {
        vjson_skip_ws(s, n, &i, &line);
        if (i < n && s[i] == ']') { i++; break; }
        if (!first) {
            if (i >= n || s[i] != ',') goto jfail_comma;
            i++;
            vjson_skip_ws(s, n, &i, &line);
        }
        first = 0;
        if (i >= n || s[i] != '{') goto jfail_obj;
        i++;
        cur.fields = NULL; cur.nfields = cur.capfields = 0;
        in_obj = 1;
        int need_comma = 0;
        for (;;) {
            vjson_skip_ws(s, n, &i, &line);
            if (i < n && s[i] == '}') { i++; break; }
            if (need_comma) {
                if (i >= n || s[i] != ',') goto jfail_comma_in;
                i++;
                vjson_skip_ws(s, n, &i, &line);
            }
            need_comma = 1;
            if (i >= n || s[i] != '"') goto jfail_key;
            i++;
            blen = 0;
            while (i < n && s[i] != '"') {
                if (s[i] == '\\' && i + 1 < n) {
                    if (!vjson_escape(&buf, &bcap, &blen, s, n, &i)) goto jfail_esc;
                } else if (s[i] == '\n') { line++; vcsv_buf_put(&buf, &bcap, &blen, s[i]); i++; }
                else { vcsv_buf_put(&buf, &bcap, &blen, s[i]); i++; }
            }
            if (i >= n) goto jfail_key;
            i++;
            VStr kf = vcsv_buf_take(&buf, &blen);
            int64_t kidx = -1;
            for (int64_t k = 0; k < keys.nfields; k++) {
                if (keys.fields[k].len == kf.len && memcmp(keys.fields[k].s, kf.s, (size_t)kf.len) == 0) { kidx = k; break; }
            }
            if (kidx < 0) {
                vcsv_row_push_field(&keys, kf.s, kf.len);
                kidx = keys.nfields - 1;
                for (int64_t r = 0; r < t->nrows; r++) {
                    while (t->rows[r].nfields < kidx) {
                        char *zp = (char *)malloc(1);
                        if (!zp) { fprintf(stderr, "alloc error\n"); abort(); }
                        zp[0] = '\0';
                        vcsv_row_push_field(&t->rows[r], zp, 0);
                    }
                }
            } else { free(kf.s); }
            while (cur.nfields < kidx) {
                char *zp = (char *)malloc(1);
                if (!zp) { fprintf(stderr, "alloc error\n"); abort(); }
                zp[0] = '\0';
                vcsv_row_push_field(&cur, zp, 0);
            }
            vjson_skip_ws(s, n, &i, &line);
            if (i >= n || s[i] != ':') goto jfail_colon;
            i++;
            vjson_skip_ws(s, n, &i, &line);
            if (i >= n) goto jfail_val;
            if (s[i] == '"') {
                i++;
                blen = 0;
                while (i < n && s[i] != '"') {
                    if (s[i] == '\\' && i + 1 < n) {
                        if (!vjson_escape(&buf, &bcap, &blen, s, n, &i)) goto jfail_esc;
                    } else if (s[i] == '\n') { line++; vcsv_buf_put(&buf, &bcap, &blen, s[i]); i++; }
                    else { vcsv_buf_put(&buf, &bcap, &blen, s[i]); i++; }
                }
                if (i >= n) goto jfail_val;
                i++;
                VStr vf = vcsv_buf_take(&buf, &blen);
                vcsv_row_push_field(&cur, vf.s, vf.len);
            } else if ((s[i] >= '0' && s[i] <= '9') || s[i] == '-') {
                int64_t st = i;
                if (s[i] == '-') i++;
                int digits = 0;
                while (i < n && s[i] >= '0' && s[i] <= '9') { i++; digits = 1; }
                if (i < n && s[i] == '.') { i++; while (i < n && s[i] >= '0' && s[i] <= '9') { i++; digits = 1; } }
                if (i < n && (s[i] == 'e' || s[i] == 'E')) {
                    i++;
                    if (i < n && (s[i] == '+' || s[i] == '-')) i++;
                    int expd = 0;
                    while (i < n && s[i] >= '0' && s[i] <= '9') { i++; expd = 1; }
                    if (!expd) goto jfail_val;
                }
                if (!digits) goto jfail_val;
                int64_t vlen = i - st;
                char *vp = (char *)malloc((size_t)vlen + 1);
                if (!vp) { fprintf(stderr, "alloc error\n"); abort(); }
                memcpy(vp, s + st, (size_t)vlen);
                vp[vlen] = '\0';
                vcsv_row_push_field(&cur, vp, vlen);
            } else if (i + 4 <= n && memcmp(s + i, "true", 4) == 0) {
                char *vp = (char *)malloc(5);
                if (!vp) { fprintf(stderr, "alloc error\n"); abort(); }
                memcpy(vp, "true", 5);
                vcsv_row_push_field(&cur, vp, 4);
                i += 4;
            } else if (i + 5 <= n && memcmp(s + i, "false", 5) == 0) {
                char *vp = (char *)malloc(6);
                if (!vp) { fprintf(stderr, "alloc error\n"); abort(); }
                memcpy(vp, "false", 6);
                vcsv_row_push_field(&cur, vp, 5);
                i += 5;
            } else if (i + 4 <= n && memcmp(s + i, "null", 4) == 0) {
                char *vp = (char *)malloc(1);
                if (!vp) { fprintf(stderr, "alloc error\n"); abort(); }
                vp[0] = '\0';
                vcsv_row_push_field(&cur, vp, 0);
                i += 4;
            } else if (s[i] == '{' || s[i] == '[') {
                *emsg = vjson_err(i, "nested values need flat objects");
                goto jfail_free;
            } else goto jfail_val;
        }
        while (cur.nfields < keys.nfields) {
            char *zp = (char *)malloc(1);
            if (!zp) { fprintf(stderr, "alloc error\n"); abort(); }
            zp[0] = '\0';
            vcsv_row_push_field(&cur, zp, 0);
        }
        vcsv_table_push_row(t, cur);
        cur.fields = NULL; cur.nfields = cur.capfields = 0;
        in_obj = 0;
    }
    vjson_skip_ws(s, n, &i, &line);
    if (i < n) {
        *emsg = vjson_err(i, "trailing characters");
        goto jfail_free;
    }
    {
        VCsvRow hdr = {NULL, 0, 0};
        for (int64_t k = 0; k < keys.nfields; k++) {
            vcsv_row_push_field(&hdr, keys.fields[k].s, keys.fields[k].len);
        }
        free(keys.fields);
        VCsvTable combo = {NULL, 0, 0};
        vcsv_table_push_row(&combo, hdr);
        for (int64_t r = 0; r < t->nrows; r++) {
            vcsv_table_push_row(&combo, t->rows[r]);
        }
        free(t->rows);
        *t = combo;
    }
    free(buf);
    return 0;
jfail_comma:
    *emsg = vjson_err(i, "expected `,` between objects");
    goto jfail_free;
jfail_comma_in:
    *emsg = vjson_err(i, "expected `,` between members");
    goto jfail_free;
jfail_obj:
    *emsg = vjson_err(i, "expected object");
    goto jfail_free;
jfail_key:
    *emsg = vjson_err(i, "expected string key");
    goto jfail_free;
jfail_esc:
    *emsg = vjson_err(i, "bad string escape");
    goto jfail_free;
jfail_colon:
    *emsg = vjson_err(i, "expected `:`");
    goto jfail_free;
jfail_val:
    *emsg = vjson_err(i, "expected value");
    goto jfail_free;
jfail_free:
    if (in_obj) vcsv_free_row(&cur);
    vcsv_free_table(t);
    vcsv_free_row(&keys);
    free(buf);
    return i + 1;
}
"##;

/// HIR (checked AST) -> C11 source.
///
/// Memory discipline (mirrors ownership.rs, D4 §1–2):
/// - Copy scalars are plain C values.
/// - Every owned String lives in exactly one slot: a local/param (freed at scope
///   end, NULL skips), a named temporary (freed at statement end, NULL skips),
///   or transferred into another slot (source NULLed at transfer).
/// - `cur_move` threads the move-context so borrowed uses (operands, `print`,
///   conditions) never NULL their source while moved uses do.
pub fn lower(checked: &Checked) -> Result<String, Ice> {
    let mut g = Gen {
        checked,
        out: String::new(),
        tmp: 0,
        scopes: vec![],
        owned: HashMap::new(),
        vartypes: HashMap::new(),
        nulls: vec![],
        tmp_frames: vec![],
        cur_move: false,
        fn_ret: Ty::Unit,
        loop_stack: vec![],
        cur_module: String::new(),
        aliases: checked.aliases.clone(),
        fun_set: checked
            .program
            .funs
            .iter()
            .map(|f| (f.module.clone(), f.name.clone()))
            .collect(),
        type_set: checked
            .program
            .structs
            .iter()
            .map(|d| (d.module.clone(), d.name.clone()))
            .chain(
                checked
                    .program
                    .enums
                    .iter()
                    .map(|d| (d.module.clone(), d.name.clone())),
            )
            .collect(),
        print_insts: checked.print_insts.clone(),
        structs: checked.structs.clone(),
        enums: checked.enums.clone(),
        generic_insts: checked.generic_insts.clone(),
    };
    g.out.push_str(PRELUDE);
    g.out.push_str(PRELUDE_J2);
    g.out.push('\n');
    g.emit_adts()?;
    // Prototypes for every function (cross-module calls + forward references;
    // C23 rejects implicit declarations). `main` is never called: no proto.
    for f in &checked.program.funs {
        if f.name == "main" {
            continue;
        }
        g.emit_proto(f);
    }
    for f in &checked.program.funs {
        g.emit_fun(f)?;
    }
    Ok(g.out)
}

/// One C layout: a struct/enum ADT or a builtin-generic instantiation
/// (`Vec` array layouts vs `Option`/`Result` tagged unions emit differently).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum LNode {
    Struct(String),
    Enum(String),
    Vec(String),
    Union(String),
}

struct Gen<'a> {
    checked: &'a Checked,
    out: String,
    tmp: u32,
    /// enclosing scopes' owned locals (name, Valvet type) for frees.
    scopes: Vec<Vec<(String, Ty)>>,
    /// mangled names holding owned values (locals + params) with their types.
    owned: HashMap<String, Ty>,
    /// mangled names of ALL locals/params with their types (incl. Copy, for
    /// `->` emission through reference-typed slots).
    vartypes: HashMap<String, Ty>,
    /// pending moved-from slots to poison: (lvalue, type).
    nulls: Vec<(String, Ty)>,
    /// ADT tables for layout/drop/poison lowering.
    structs: HashMap<String, StructDef>,
    enums: HashMap<String, EnumDef>,
    /// Builtin generic instantiations: mangled key -> (base, arg types).
    generic_insts: HashMap<String, (String, Vec<Ty>)>,
    /// per-statement owned temporaries (name, C type) to destroy at end.
    tmp_frames: Vec<Vec<(String, String)>>,
    /// whether the current value position moves Strings (mirrors ownership.rs).
    cur_move: bool,
    /// enclosing function return type (for `return` inside value-`loop` bodies).
    fn_ret: Ty,
    /// lexically enclosing loops (innermost last) for break/continue lowering.
    loop_stack: Vec<LoopInfo>,
    /// Module whose function is being emitted ("" for the entry).
    cur_module: String,
    /// Import aliases from sema: (from, local) -> (target module, target name).
    aliases: HashMap<(String, String), (String, String)>,
    /// All functions by (module, name) for callee-module resolution.
    fun_set: std::collections::HashSet<(String, String)>,
    /// All struct/enum types by (module, name) for qualifier resolution.
    type_set: std::collections::HashSet<(String, String)>,
    /// Composite instantiations printed via `println` (keys into
    /// `generic_insts`); format helpers emit exactly for these.
    print_insts: std::collections::HashSet<String>,
}

/// Innermost-enclosing-loop info for `break`/`continue` codegen.
struct LoopInfo {
    /// Gen scope depth at loop-body entry; scopes at/after it must be freed
    /// before breaking/continuing out.
    body_depth: usize,
    /// Tmp-frame depth at loop-body entry; abandoned statement temps must be
    /// freed too (their transferred values were already nulled).
    tmp_depth: usize,
    /// `None` for statement loops; result slot + end label for value loops.
    value: Option<LoopValue>,
}

struct LoopValue {
    res: String,
    label: String,
}

impl<'a> Gen<'a> {
    fn ty_of(&self, id: NodeId) -> Ty {
        self.checked.types.get(&id).cloned().unwrap_or(Ty::Unit)
    }

    /// Resolve a callee name from the current module: own items first, then
    /// imports (mirrors sema's `resolve_fun`; sema already rejected bad
    /// programs, so misses here are ICEs). Returns (module, name).
    fn resolve_fun(&self, name: &str) -> Option<(String, String)> {
        if self
            .fun_set
            .contains(&(self.cur_module.clone(), name.to_string()))
        {
            return Some((self.cur_module.clone(), name.to_string()));
        }
        self.aliases
            .get(&(self.cur_module.clone(), name.to_string()))
            .cloned()
    }

    /// Resolve a type name like `resolve_fun` (structs + enums share one
    /// namespace). Returns the qualified name for the global tables.
    fn resolve_type(&self, name: &str) -> Option<String> {
        let own = (self.cur_module.clone(), name.to_string());
        if self.type_set.contains(&own) {
            return Some(crate::resolver::qname(&self.cur_module, name));
        }
        if let Some((tm, tn)) = self
            .aliases
            .get(&(self.cur_module.clone(), name.to_string()))
        {
            let tq = crate::resolver::qname(tm, tn);
            if self.type_set.contains(&(tm.clone(), tn.clone())) {
                return Some(tq);
            }
        }
        None
    }

    /// Resolve a pattern qualifier like `resolve_type` (None stays None;
    /// builtins pass through; anything else must resolve or it is an ICE —
    /// sema validated every pattern).
    fn resolve_pat_ty(&self, q: &Option<String>) -> Result<Option<String>, Ice> {
        match q {
            None => Ok(None),
            Some(t) if t == "Option" || t == "Result" || t == "Missing" => Ok(Some(t.clone())),
            Some(t) => Ok(Some(
                self.resolve_type(t)
                    .ok_or_else(|| Ice(format!("unknown enum {}", t)))?,
            )),
        }
    }

    /// Prototype for one function (`;`-terminated header, no body).
    fn emit_proto(&mut self, f: &FunDecl) {
        let ret = self.cty(&f.ret).to_string();
        if f.params.is_empty() {
            self.out.push_str(&format!(
                "static {} {}_fn(void);\n",
                ret,
                cfun(&f.module, &f.name)
            ));
        } else {
            self.out.push_str(&format!(
                "static {} {}_fn({});\n",
                ret,
                cfun(&f.module, &f.name),
                f.params
                    .iter()
                    .map(|p| format!("{} {}", self.cty(&p.ty), mangle(&p.name)))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
    }

    /// Whether a place denotes a reference-typed slot (for `->` emission).
    /// Consults all locals/params (shared refs are Copy and untracked in
    /// `owned`, so `owned` alone is insufficient).
    fn base_is_ref(&self, place: &Place) -> bool {
        match place {
            Place::Var(n, _) => matches!(self.vartypes.get(&mangle(n)), Some(Ty::Reference { .. })),
            _ => false,
        }
    }

    fn cty(&self, t: &Ty) -> String {
        match t {
            Ty::Int32 => "int32_t".to_string(),
            Ty::Int64 => "int64_t".to_string(),
            Ty::Float64 => "double".to_string(),
            Ty::Bool => "int".to_string(),
            Ty::Str => "VStr".to_string(),
            Ty::Unit => "void".to_string(),
            Ty::Struct(n) | Ty::Enum(n) => ctype_name(n),
            Ty::Reference { inner, .. } => format!("{} *", self.cty(inner)),
            // Sema resolves every nominal type in place; reaching this means a
            // compiler bug, never user input.
            Ty::Named(n) => unreachable!("unresolved type {}", n),
            Ty::Generic { base, args } => ctype_inst(base, args),
        }
    }

    /// C type for value positions. `Unit` erases to `int` holding 0
    /// (C has no void values); `cty` ("void") is for fn signatures only.
    fn cty_val(&self, t: &Ty) -> String {
        match t {
            Ty::Unit => "int".to_string(),
            other => self.cty(other),
        }
    }

    /// Whether values of this type move (need poison-on-move + Drops).
    /// Mirrors `sema::is_copy_ty`: primitives copy, Strings + ADTs with
    /// owned content move.
    fn moves(&self, t: &Ty) -> bool {
        !is_copy_ty(t, &self.structs, &self.enums)
    }

    fn fresh_tmp(&mut self) -> String {
        self.tmp += 1;
        format!("_vt{}", self.tmp)
    }

    /// Declare an arm binding in the current (arm) scope.
    fn declare(&mut self, name: String, ty: Ty) {
        if let Some(sc) = self.scopes.last_mut() {
            sc.push((name, ty));
        }
    }

    // ----- temporary frames -----

    fn push_frame(&mut self) {
        self.tmp_frames.push(vec![]);
    }

    /// Emit destruction for the current frame's temps and discard it.
    /// Slots already poisoned (moved out) are safely skipped.
    fn pop_frame(&mut self) {
        if let Some(frame) = self.tmp_frames.pop() {
            for (t, cty) in frame.iter().rev() {
                if cty == "VStr" {
                    self.out.push_str(&format!("vstr_free(&{});\n", t));
                } else {
                    let vty = self.valvet_ty_of_cty(cty);
                    self.emit_destroy(t, &vty);
                }
            }
        }
    }

    /// Emit destruction for every live temp in every frame WITHOUT
    /// popping: for `return` inside a partially-evaluated construct, the
    /// enclosing owners still pop (and destroy) on fallthrough paths, and
    /// exactly one path runs at runtime. Popping here orphaned those pops:
    /// they found nothing, leaking every temp on non-return paths.
    fn destroy_all_temps(&mut self) {
        let live: Vec<(String, String)> = self
            .tmp_frames
            .iter()
            .rev()
            .flat_map(|f| f.iter().rev().cloned())
            .collect();
        for (t, cty) in &live {
            if cty == "VStr" {
                self.out.push_str(&format!("vstr_free(&{});\n", t));
            } else {
                let vty = self.valvet_ty_of_cty(cty);
                self.emit_destroy(t, &vty);
            }
        }
    }

    /// Best-effort reverse map from temp C type to Valvet type (temps only
    /// ever hold Strings or nominal ADTs, both unambiguous here).
    fn valvet_ty_of_cty(&self, cty: &str) -> Ty {
        if cty == "VStr" {
            return Ty::Str;
        }
        if let Some(stripped) = cty.strip_prefix('V') {
            if self.structs.contains_key(stripped) {
                return Ty::Struct(stripped.to_string());
            }
            if self.enums.contains_key(stripped) {
                return Ty::Enum(stripped.to_string());
            }
            // Instantiation names embed base + args (`VOption_Int32`).
            for (key, (base, args)) in &self.generic_insts {
                let _ = key;
                if ctype_inst(base, args) == *cty {
                    return Ty::Generic {
                        base: base.clone(),
                        args: args.clone(),
                    };
                }
            }
        }
        Ty::Unit
    }

    /// Declare `<cty> <tmp> = <init>;` registered with its type so frame
    /// frees destroy it correctly (drop fns for ADTs).
    fn new_tmp_ty(&mut self, cty: &str, init: &str) -> String {
        let t = self.fresh_tmp();
        self.out.push_str(&format!("{} {} = {};\n", cty, t, init));
        if let Some(f) = self.tmp_frames.last_mut() {
            f.push((t.clone(), cty.to_string()));
        }
        t
    }

    fn is_tmp(&self, name: &str) -> bool {
        self.tmp_frames
            .iter()
            .any(|f| f.iter().any(|(t, _)| t == name))
    }

    /// Schedule poisoning of a moved-from slot (owned var or live temp).
    /// Resolution is type-aware: Strings get `.s = NULL`, structs/enums get
    /// their owned field paths poisoned (tag-dispatched for enums).
    fn null_slot(&mut self, name: &str) {
        if let Some(ty) = self.owned.get(name).cloned() {
            self.nulls.push((name.to_string(), ty));
        } else if let Some(ty) = self.tmp_ty(name) {
            self.nulls.push((name.to_string(), ty));
        }
    }

    /// Declared type of a frame temp, if any.
    fn tmp_ty(&self, name: &str) -> Option<Ty> {
        for f in self.tmp_frames.iter().rev() {
            for (t, cty) in f.iter().rev() {
                if t == name {
                    return Some(self.valvet_ty_of_cty(cty));
                }
            }
        }
        None
    }

    fn flush_nulls(&mut self) {
        let mut seen = HashSet::new();
        for (n, t) in std::mem::take(&mut self.nulls) {
            if seen.insert(n.clone()) {
                self.emit_poison(&n, &t);
            }
        }
    }

    /// Emit poisoning statements for moved-from lvalue `lval` of type `ty`.
    fn emit_poison(&mut self, lval: &str, ty: &Ty) {
        match ty {
            Ty::Str => {
                self.out.push_str(&format!("{}.s = NULL;\n", lval));
            }
            Ty::Struct(n) => {
                if let Some(def) = self.structs.get(n).cloned() {
                    for (f, ft) in &def.fields {
                        if self.moves(ft) {
                            let sub = format!("{}.{}", lval, cfield(f));
                            self.emit_poison(&sub, ft);
                        }
                    }
                }
            }
            Ty::Generic { base, args } if base == "Vec" => {
                // Neutering a moved-from Vec: drop skips NULL buffers.
                let _ = args;
                self.out.push_str(&format!("{}.data = NULL;\n", lval));
                self.out.push_str(&format!("{}.len = 0;\n", lval));
            }
            Ty::Generic { base, args } => {
                let variants = self.generic_variants(base, args);
                if variants
                    .iter()
                    .any(|(_, pts)| pts.iter().any(|t| self.moves(t)))
                {
                    self.out.push_str(&format!("switch ({}.tag) {{\n", lval));
                    for (i, (_, pts)) in variants.iter().enumerate() {
                        let mut body = String::new();
                        for (j, pt) in pts.iter().enumerate() {
                            if self.moves(pt) {
                                let sub = format!(
                                    "{}.u.{}.{}",
                                    lval,
                                    cmember(i),
                                    cfield(&format!("_{}", j))
                                );
                                body.push_str(&self.poison_text(&sub, pt));
                            }
                        }
                        if !body.is_empty() {
                            self.out.push_str(&format!("case {}: {}break;\n", i, body));
                        }
                    }
                    self.out.push_str("default: break;\n}\n");
                }
            }
            Ty::Enum(n) => {
                if let Some(def) = self.enums.get(n).cloned() {
                    let mut any = false;
                    for (_, pts) in &def.variants {
                        if pts.iter().any(|t| self.moves(t)) {
                            any = true;
                            break;
                        }
                    }
                    if !any {
                        return;
                    }
                    self.out.push_str(&format!("switch ({}.tag) {{\n", lval));
                    for (i, (_, pts)) in def.variants.iter().enumerate() {
                        let mut body = String::new();
                        for (j, pt) in pts.iter().enumerate() {
                            if self.moves(pt) {
                                let sub = format!(
                                    "{}.u.{}.{}",
                                    lval,
                                    cmember(i),
                                    cfield(&format!("_{}", j))
                                );
                                body.push_str(&self.poison_text(&sub, pt));
                            }
                        }
                        if !body.is_empty() {
                            self.out.push_str(&format!("case {}: {}break;\n", i, body));
                        }
                    }
                    self.out.push_str("default: break;\n}\n");
                }
            }
            _ => {}
        }
    }

    /// Poison statements as text (for nesting inside case arms).
    fn poison_text(&self, lval: &str, ty: &Ty) -> String {
        match ty {
            Ty::Str => format!("{}.s = NULL;\n", lval),
            Ty::Struct(n) => {
                let mut out = String::new();
                if let Some(def) = self.structs.get(n) {
                    for (f, ft) in &def.fields {
                        if self.moves(ft) {
                            out.push_str(&self.poison_text(&format!("{}.{}", lval, cfield(f)), ft));
                        }
                    }
                }
                out
            }
            Ty::Generic { base, args } if base == "Vec" => {
                format!("{}.data = NULL;\n{}.len = 0;\n", lval, lval)
            }
            Ty::Generic { base, args } => {
                let mut out = String::new();
                for (i, (_, pts)) in self.generic_variants(base, args).iter().enumerate() {
                    for (j, pt) in pts.iter().enumerate() {
                        if self.moves(pt) {
                            let sub =
                                format!("{}.u.{}.{}", lval, cmember(i), cfield(&format!("_{}", j)));
                            out.push_str(&format!(
                                "if ({}.tag == {}) {{ {} }}",
                                lval,
                                i,
                                self.poison_text(&sub, pt)
                            ));
                        }
                    }
                }
                out
            }
            Ty::Enum(n) => {
                let mut out = String::new();
                if let Some(def) = self.enums.get(n) {
                    for (i, (_, pts)) in def.variants.iter().enumerate() {
                        for (j, pt) in pts.iter().enumerate() {
                            if self.moves(pt) {
                                let sub = format!(
                                    "{}.u.{}.{}",
                                    lval,
                                    cmember(i),
                                    cfield(&format!("_{}", j))
                                );
                                out.push_str(&format!(
                                    "if ({}.tag == {}) {{ {} }}",
                                    lval,
                                    i,
                                    self.poison_text(&sub, pt)
                                ));
                            }
                        }
                    }
                }
                out
            }
            _ => String::new(),
        }
    }

    // ----- value materialization -----

    /// Materialize an owned-typed expression into a *name* holding its value:
    /// a plain owned variable (lvalue, move handled via `cur_move`) or a fresh
    /// frame temp of the right C type (rvalues: literals, concats, calls,
    /// if-values, struct/enum constructions). Copy values emit inline.
    fn mat_str(&mut self, e: &Expr) -> Result<String, Ice> {
        let t = self.ty_of(e.id);
        if !self.moves(&t) {
            return self.emit_expr(e);
        }
        if let ExprKind::Var(name) = &e.kind {
            let cname = mangle(name);
            // Bare `None`/`Missing` (unshadowed — sema guarantees) is a constructor.
            // Handled here too because this fast path bypasses emit_expr.
            if (name == "None" || name == "Missing") && !self.owned.contains_key(&cname) {
                if let Ty::Generic { base, args } = self.ty_of(e.id) {
                    if base == "Option" || base == "Missing" {
                        return Ok(format!("({}){{ .tag = 1 }}", ctype_inst(&base, &args)));
                    }
                }
            }
            if self.cur_move {
                self.null_slot(&cname);
            }
            return Ok(cname);
        }
        if let ExprKind::Field { .. } = &e.kind {
            // Field lvalues need their full path for poisoning; emit the path
            // inline and let the caller transfer ownership out of it.
            // Dereferences likewise: `(*v)` is an lvalue aliasing borrowed
            // data — wrapping it in an owned temp would free the lender's
            // buffer on drop (use-after-free/double-free). Owned derefs
            // clone out in move position (see the `Deref` arm), so inline is
            // correct in both positions.
            let path = self.emit_expr(e)?;
            return Ok(path);
        }
        if let ExprKind::Unary {
            op: UnOp::Deref, ..
        } = &e.kind
        {
            let path = self.emit_expr(e)?;
            return Ok(path);
        }
        let inline = self.emit_expr(e)?;
        if self.is_tmp(&inline) {
            // Value-position `if` already materialized into a live frame temp;
            // reuse it instead of aliasing it into a second temp (double free).
            return Ok(inline);
        }
        Ok(self.new_tmp_ty(&self.cty_val(&t), &inline))
    }

    /// After a move-position use of a materialized name, transfer ownership out:
    /// poison the source slot so neither scope frees nor frame frees release it.
    fn consume(&mut self, name: &str) {
        self.null_slot(name);
    }

    /// Resolve the declared type of `base.field` (base must be a struct value).
    fn field_ty_of(&self, base: &Expr, field: &str) -> Option<Ty> {
        match self.ty_of(base.id) {
            Ty::Struct(n) => self
                .structs
                .get(&n)
                .and_then(|d| d.fields.iter().find(|(f, _)| f == field))
                .map(|(_, t)| t.clone()),
            _ => None,
        }
    }

    // ----- algebraic data types -----

    /// Emit C layouts + drop/clone helpers for all declared ADTs.
    /// Deterministic order (sorted by name) keeps builds reproducible.
    /// Topological layout order over ADTs + instantiations (C needs complete
    /// types for by-value members and declared typedefs for pointer members).
    /// Dependencies: struct/enum fields+payloads (recursive through builtin
    /// generics), instantiation args. Sema rejects type cycles, so this is a
    /// DAG; a gray-node guard keeps a pathological input from hanging codegen.
    fn layout_order(&self) -> Vec<LNode> {
        use std::collections::{HashMap, HashSet};
        // Node dependencies.
        let mut deps: HashMap<LNode, Vec<LNode>> = HashMap::new();
        let dep_of_ty = |ty: &Ty, out: &mut Vec<LNode>| {
            match ty {
                Ty::Struct(n) => {
                    if self.structs.contains_key(n) {
                        out.push(LNode::Struct(n.clone()));
                    }
                }
                Ty::Enum(n) => {
                    if self.enums.contains_key(n) {
                        out.push(LNode::Enum(n.clone()));
                    }
                }
                Ty::Generic { base, args } => {
                    // Key lookup by value (never recompute sema's key format).
                    if let Some(key) = self.inst_key_for(base, args) {
                        if base == "Vec" {
                            out.push(LNode::Vec(key));
                        } else {
                            out.push(LNode::Union(key));
                        }
                    }
                    // (transitive deps resolve through the graph, not here)
                }
                _ => {}
            }
        };
        for (n, def) in &self.structs {
            let mut d = vec![];
            for (_, ft) in &def.fields {
                dep_of_ty(ft, &mut d);
            }
            // Self-edge (unit structs: none) harmless via visited set.
            d.retain(|x| *x != LNode::Struct(n.clone()));
            deps.insert(LNode::Struct(n.clone()), d);
        }
        for (n, def) in &self.enums {
            let mut d = vec![];
            for (_, pts) in &def.variants {
                for pt in pts {
                    dep_of_ty(pt, &mut d);
                }
            }
            d.retain(|x| *x != LNode::Enum(n.clone()));
            deps.insert(LNode::Enum(n.clone()), d);
        }
        for (key, (base, args)) in &self.generic_insts {
            let mut d = vec![];
            for a in args {
                dep_of_ty(a, &mut d);
            }
            let node = if base == "Vec" {
                LNode::Vec(key.clone())
            } else {
                LNode::Union(key.clone())
            };
            d.retain(|x| *x != node);
            deps.insert(node, d);
        }
        // Deterministic DFS post-order (neighbors + roots sorted).
        let mut order = vec![];
        let mut done: HashSet<LNode> = HashSet::new();
        let mut busy: HashSet<LNode> = HashSet::new();
        fn visit(
            n: &LNode,
            deps: &HashMap<LNode, Vec<LNode>>,
            done: &mut HashSet<LNode>,
            busy: &mut HashSet<LNode>,
            order: &mut Vec<LNode>,
        ) {
            if done.contains(n) || busy.contains(n) {
                return;
            }
            busy.insert(n.clone());
            if let Some(ds) = deps.get(n) {
                let mut ds = ds.clone();
                ds.sort();
                ds.dedup();
                for d in &ds {
                    visit(d, deps, done, busy, order);
                }
            }
            busy.remove(n);
            done.insert(n.clone());
            order.push(n.clone());
        }
        let mut roots: Vec<LNode> = deps.keys().cloned().collect();
        roots.sort();
        for r in &roots {
            visit(r, &deps, &mut done, &mut busy, &mut order);
        }
        order
    }

    /// Instantiation key for `(base, args)` by value scan (never recompute
    /// sema's key format; 0.1.0 programs are small).
    fn inst_key_for(&self, base: &str, args: &[Ty]) -> Option<String> {
        self.generic_insts
            .iter()
            .find(|(_, (b, a))| b == base && a == args)
            .map(|(k, _)| k.clone())
    }

    /// One struct layout (`VName{ fields }`).
    fn emit_struct_layout(&mut self, n: &str) -> Result<(), Ice> {
        let def = self.structs.get(n).cloned().unwrap_or(StructDef {
            fields: vec![],
            is_tuple: false,
        });
        self.out.push_str("typedef struct {\n");
        if def.fields.is_empty() {
            self.out.push_str("char _empty;\n");
        }
        for (f, ft) in &def.fields {
            self.out
                .push_str(&format!("{} {};\n", self.cty(ft), cfield(f)));
        }
        self.out.push_str(&format!("}} {};\n", ctype_name(n)));
        Ok(())
    }

    /// One enum layout (tagged union).
    fn emit_enum_layout(&mut self, n: &str) -> Result<(), Ice> {
        let def = self
            .enums
            .get(n)
            .cloned()
            .unwrap_or(EnumDef { variants: vec![] });
        self.out.push_str("typedef struct {\nint tag;\nunion {\n");
        for (i, (_, pts)) in def.variants.iter().enumerate() {
            if pts.is_empty() {
                self.out
                    .push_str(&format!("struct {{ char _u; }} {};\n", cmember(i)));
            } else {
                self.out.push_str("struct {\n");
                for (j, pt) in pts.iter().enumerate() {
                    self.out.push_str(&format!(
                        "{} {};\n",
                        self.cty(pt),
                        cfield(&format!("_{}", j))
                    ));
                }
                self.out.push_str(&format!("}} {};\n", cmember(i)));
            }
        }
        self.out
            .push_str(&format!("}} u;\n}} {};\n", ctype_name(n)));
        Ok(())
    }

    /// One `Vec<T>` layout (pointer members: order-free, but topo keeps it simple).
    fn emit_vec_layout(&mut self, key: &str) -> Result<(), Ice> {
        let (base, args) = self.generic_insts.get(key).cloned().unwrap_or_default();
        if base != "Vec" || args.len() != 1 {
            return Ok(());
        }
        let ct = ctype_inst(&base, &args);
        let et = self.cty(&args[0]);
        self.out.push_str(&format!(
            "typedef struct {{ {} *data; int64_t len; int64_t cap; }} {};\n",
            et, ct
        ));
        Ok(())
    }

    /// One `Option`/`Result` tagged-union layout.
    fn emit_union_layout(&mut self, key: &str) -> Result<(), Ice> {
        let (base, args) = self.generic_insts.get(key).cloned().unwrap_or_default();
        if base == "Vec" {
            return Ok(());
        }
        let ct = ctype_inst(&base, &args);
        let variants = self.generic_variants(&base, &args);
        self.out.push_str("typedef struct {\nint tag;\nunion {\n");
        for (i, (_, pts)) in variants.iter().enumerate() {
            if pts.is_empty() {
                self.out
                    .push_str(&format!("struct {{ char _u; }} {};\n", cmember(i)));
            } else {
                self.out.push_str("struct {\n");
                for (j, pt) in pts.iter().enumerate() {
                    self.out.push_str(&format!(
                        "{} {};\n",
                        self.cty(pt),
                        cfield(&format!("_{}", j))
                    ));
                }
                self.out.push_str(&format!("}} {};\n", cmember(i)));
            }
        }
        self.out.push_str(&format!("}} u;\n}} {};\n", ct));
        Ok(())
    }

    fn emit_adts(&mut self) -> Result<(), Ice> {
        // Layout order (C needs complete types for by-value members): `Vec`
        // layouts first (pointer members: order-free), then ADT layouts
        // (structs may hold Vecs by value), then tagged unions depth-sorted
        // (payloads by value: Vecs, structs, shallower unions first).
        let mut snames: Vec<String> = self.structs.keys().cloned().collect();
        snames.sort();
        let mut enames: Vec<String> = self.enums.keys().cloned().collect();
        enames.sort();
        let mut ikeys: Vec<String> = self.generic_insts.keys().cloned().collect();
        ikeys.sort();
        // Layouts in topological order (see `layout_order`).
        let order = self.layout_order();
        for node in &order {
            match node {
                LNode::Struct(n) => self.emit_struct_layout(n)?,
                LNode::Enum(n) => self.emit_enum_layout(n)?,
                LNode::Vec(key) => self.emit_vec_layout(key)?,
                LNode::Union(key) => self.emit_union_layout(key)?,
            }
        }

        // Forward declarations: generated helpers call each other across
        // emission order (C23 rejects implicit declarations).
        for n in snames.iter() {
            if self.type_needs_drop(&Ty::Struct(n.clone())) {
                let ct = ctype_name(n);
                self.out.push_str(&format!(
                    "static void v_drop_{}({ct} *v);\nstatic {ct} v_clone_{}({ct} v);\n",
                    dname(n),
                    dname(n)
                ));
            }
        }
        for n in enames.iter() {
            if self.type_needs_drop(&Ty::Enum(n.clone())) {
                let ct = ctype_name(n);
                self.out.push_str(&format!(
                    "static void v_drop_{}({ct} *v);\nstatic {ct} v_clone_{}({ct} v);\n",
                    dname(n),
                    dname(n)
                ));
            }
        }
        for key in ikeys.iter() {
            let (base, args) = self.generic_insts.get(key).cloned().unwrap_or_default();
            if base == "Vec" && args.len() == 1 {
                let t = args[0].clone();
                let ct = ctype_inst(&base, &args);
                let iname = ct.strip_prefix('V').unwrap_or(&ct).to_string();
                let et = self.cty(&t);
                let oct = ctype_inst("Option", std::slice::from_ref(&t));
                self.out
                    .push_str(&format!("static void v_drop_{iname}({ct} *v);\n"));
                self.out
                    .push_str(&format!("static {ct} v_clone_{iname}({ct} v);\n"));
                self.out
                    .push_str(&format!("static {ct} vvec_{iname}_new(void);\n"));
                self.out.push_str(&format!(
                    "static void vvec_{iname}_push({ct} *v, {et} x);\n"
                ));
                self.out
                    .push_str(&format!("static {oct} vvec_{iname}_pop({ct} *v);\n"));
                self.out.push_str(&format!(
                    "static {oct} vvec_{iname}_get({ct} *v, int64_t i);\n"
                ));
                self.out
                    .push_str(&format!("static int64_t vvec_{iname}_len({ct} *v);\n"));
                if matches!(t, Ty::Float64 | Ty::Int32 | Ty::Int64) {
                    let vct = ct.clone();
                    let shape = Ty::Struct("ShapeError".to_string());
                    let vt = Ty::Generic {
                        base: "Vec".to_string(),
                        args: vec![t.clone()],
                    };
                    let rct = ctype_inst("Result", &[vt.clone(), shape.clone()]);
                    self.out.push_str(&format!(
                        "static {vct} vvec_{iname}_add_scalar({vct} *v, {et} s);\n"
                    ));
                    // Kernel prototypes only when their Result layouts exist
                    // (mirrors the definition gates below).
                    if self.has_inst("Result", &[vt.clone(), shape.clone()]) {
                        self.out.push_str(&format!(
                            "static {rct} vvec_{iname}_add({vct} *a, {vct} *b);\n"
                        ));
                        self.out.push_str(&format!(
                            "static {rct} vvec_{iname}_mul({vct} *a, {vct} *b);\n"
                        ));
                    }
                    for dt in [&Ty::Float64, &Ty::Int32, &Ty::Int64] {
                        if *dt == t {
                            continue;
                        }
                        if !self.has_inst("Vec", std::slice::from_ref(dt)) {
                            continue;
                        }
                        let vct2 = ctype_inst("Vec", std::slice::from_ref(dt));
                        let iname2 = vct2.strip_prefix('V').unwrap_or(&vct2).to_string();
                        let dct = self.cty(dt);
                        self.out.push_str(&format!(
                            "static {vct2} vvec_{iname}_cast_{iname2}({vct} *v);\n"
                        ));
                        let _ = dct;
                    }
                    if t == Ty::Float64
                        && self.has_inst(
                            "Result",
                            &[Ty::Float64, Ty::Struct("ShapeError".to_string())],
                        )
                    {
                        let rfct = ctype_inst(
                            "Result",
                            &[Ty::Float64, Ty::Struct("ShapeError".to_string())],
                        );
                        self.out.push_str(&format!(
                            "static {rfct} vvec_{iname}_dot({vct} *a, {vct} *b);\n"
                        ));
                    }
                }
            } else {
                let t = Ty::Generic {
                    base: base.clone(),
                    args: args.clone(),
                };
                if self.type_needs_drop(&t) {
                    let ct = ctype_inst(&base, &args);
                    let iname = ct.strip_prefix('V').unwrap_or(&ct).to_string();
                    self.out.push_str(&format!(
                        "static void v_drop_{iname}({ct} *v);\nstatic {ct} v_clone_{iname}({ct} v);\n"
                    ));
                }
            }
        }
        // Drop + clone helpers for ADTs with owned content.
        for n in &snames {
            if self.type_needs_drop(&Ty::Struct(n.clone())) {
                self.emit_struct_drop(n)?;
                self.emit_struct_clone(n)?;
            }
        }
        for n in &enames {
            if self.type_needs_drop(&Ty::Enum(n.clone())) {
                self.emit_enum_drop(n)?;
                self.emit_enum_clone(n)?;
            }
        }
        for key in &ikeys {
            let (base, args) = self.generic_insts.get(key).cloned().unwrap_or_default();
            let t = Ty::Generic {
                base: base.clone(),
                args: args.clone(),
            };
            if base == "Vec" && args.len() == 1 {
                self.emit_vec_fns(&args)?;
                self.emit_vec_drop(&args)?;
                self.emit_vec_clone(&args)?;
                continue;
            }
            if self.type_needs_drop(&t) {
                self.emit_inst_drop(&base, &args)?;
                self.emit_inst_clone(&base, &args)?;
            }
        }
        // J1 string kernels (need vec/union layouts above to exist).
        self.emit_str_kernels()?;
        // J2 CSV/JSON/file kernels (need vec/union layouts above to exist).
        self.emit_csv_kernels()?;
        // `println` format helpers, gated on printed instantiations (sorted
        // for reproducibility; protos first — formatters call each other).
        let mut pkeys: Vec<String> = self.print_insts.iter().cloned().collect();
        pkeys.sort();
        for key in &pkeys {
            let (base, args) = self
                .generic_insts
                .get(key)
                .cloned()
                .ok_or_else(|| Ice(format!("missing instantiation {}", key)))?;
            let ct = ctype_inst(&base, &args);
            self.out.push_str(&format!(
                "static void vfmt_{}({} v);\nstatic void vprint_{}({} v);\n",
                iname_of(&ct),
                ct,
                iname_of(&ct),
                ct
            ));
        }
        for key in &pkeys {
            let (base, args) = self
                .generic_insts
                .get(key)
                .cloned()
                .ok_or_else(|| Ice(format!("missing instantiation {}", key)))?;
            self.emit_print_fmt(&base, &args)?;
        }
        Ok(())
    }

    /// J1 string kernels over the generated vec/union layouts above.
    /// Each emits only when its result instantiation was recorded.
    fn emit_str_kernels(&mut self) -> Result<(), Ice> {
        if self.has_inst("Vec", &[Ty::Str]) {
            let vct = ctype_inst("Vec", std::slice::from_ref(&Ty::Str));
            let iname = iname_of(&vct);
            self.out.push_str(&format!(
                concat!(
                    "static {vct} vstr_split(VStr v, VStr d) {{",
                    "{vct} out = vvec_{iname}_new();",
                    "char sep = (d.s && d.len > 0) ? d.s[0] : '\\0';",
                    "int64_t n = v.s ? v.len : 0;int64_t start = 0;",
                    "for (int64_t i = 0; i <= n; ++i) {{",
                    "if (i == n || (sep != '\\0' && v.s[i] == sep)) {{",
                    "int64_t len = i - start;",
                    "char *p = (char *)malloc((size_t)len + 1);",
                    "if (!p) {{ fprintf(stderr, \"alloc error\\n\"); abort(); }}",
                    "if (len) memcpy(p, v.s + start, (size_t)len);p[len] = '\\0';",
                    "vvec_{iname}_push(&out, (VStr){{p, len}});start = i + 1;",
                    "}}}}return out;}}\n"
                ),
                vct = vct,
                iname = iname
            ));
        }
        if self.has_inst("Option", &[Ty::Int64]) {
            let oct = ctype_inst("Option", std::slice::from_ref(&Ty::Int64));
            let none = format!("({oct}){{ .tag = 1 }}", oct = oct);
            let some = |x: &str| {
                format!(
                    "({oct}){{ .tag = 0, .u = {{ .v0 = {{ .v___0 = {x} }} }} }}",
                    oct = oct,
                    x = x
                )
            };
            self.out.push_str(&format!(
                concat!(
                    "static {oct} vstr_parse_int(VStr v) {{",
                    "int64_t n = v.s ? v.len : 0;int64_t i = 0;int neg = 0;",
                    "if (i < n && (v.s[i] == '+' || v.s[i] == '-')) {{ neg = (v.s[i] == '-'); i++; }}",
                    "if (i >= n) return {none};",
                    "uint64_t acc = 0;",
                    "for (; i < n; i++) {{",
                    "if (v.s[i] < '0' || v.s[i] > '9') return {none};",
                    "unsigned d = (unsigned)(v.s[i] - '0');",
                    "if (acc > (18446744073709551615ULL - d) / 10) return {none};",
                    "acc = acc * 10 + d;}}",
                    "if (!neg && acc > 9223372036854775807ULL) return {none};",
                    "if (neg && acc > 9223372036854775808ULL) return {none};",
                    "int64_t val = neg ? (int64_t)(~acc + 1) : (int64_t)acc;",
                    "return {some};}}\n"
                ),
                oct = oct,
                none = none,
                some = some("val")
            ));
        }
        if self.has_inst("Option", &[Ty::Float64]) {
            let oct = ctype_inst("Option", std::slice::from_ref(&Ty::Float64));
            let none = format!("({oct}){{ .tag = 1 }}", oct = oct);
            let some = |x: &str| {
                format!(
                    "({oct}){{ .tag = 0, .u = {{ .v0 = {{ .v___0 = {x} }} }} }}",
                    oct = oct,
                    x = x
                )
            };
            self.out.push_str(&format!(
                concat!(
                    "static {oct} vstr_parse_float(VStr v) {{",
                    "int64_t n = v.s ? v.len : 0;",
                    "if (n == 0) return {none};",
                    "char *p = (char *)malloc((size_t)n + 1);",
                    "if (!p) {{ fprintf(stderr, \"alloc error\\n\"); abort(); }}",
                    "memcpy(p, v.s, (size_t)n);p[n] = '\\0';",
                    "char *end = 0;double d = strtod(p, &end);",
                    "int ok = (end != p && *end == '\\0');free(p);",
                    "if (!ok) return {none};",
                    "return {some};}}\n"
                ),
                oct = oct,
                none = none,
                some = some("d")
            ));
        }
        Ok(())
    }

    /// J1 `String` methods (sema checked name/arity/types; receiver borrows).
    fn emit_str_method(&mut self, method: &str, r: &str, args: &[Expr]) -> Result<String, Ice> {
        match method {
            "trim" => Ok(format!("vstr_trim({})", r)),
            "clone" => Ok(format!("vstr_clone({})", r)),
            "split" => {
                let a = args.first().ok_or_else(|| Ice("split arity".into()))?;
                let saved = self.cur_move;
                self.cur_move = false;
                let d = self.mat_str(a)?;
                self.cur_move = saved;
                Ok(format!("vstr_split({}, {})", r, d))
            }
            "parse_int" => Ok(format!("vstr_parse_int({})", r)),
            "parse_float" => Ok(format!("vstr_parse_float({})", r)),
            _ => Err(Ice("unknown String method".into())),
        }
    }

    /// J2 CSV/JSON/file wrappers over the prelude row buffers. The plain
    /// layouts are identical to the generated vec structs, so conversion
    /// is a field copy. Each emits only when its result type was recorded.
    fn emit_csv_kernels(&mut self) -> Result<(), Ice> {
        let vvec = |t: Ty| Ty::Generic {
            base: "Vec".to_string(),
            args: vec![t],
        };
        let rows_t = vvec(vvec(Ty::Str));
        let inner_vec_t = vvec(Ty::Str);
        // `read_file(path): Result<String, String>`.
        if self.has_inst("Result", &[Ty::Str, Ty::Str]) {
            let rct = ctype_inst("Result", &[Ty::Str, Ty::Str]);
            let some = |x: &str| {
                format!(
                    "({rct}){{ .tag = 0, .u = {{ .v0 = {{ .v___0 = {x} }} }} }}",
                    rct = rct,
                    x = x
                )
            };
            let none = |x: &str| {
                format!(
                    "({rct}){{ .tag = 1, .u = {{ .v1 = {{ .v___0 = {x} }} }} }}",
                    rct = rct,
                    x = x
                )
            };
            self.out.push_str(&format!(
                concat!(
                    "static {rct} v_read_file_wrap(VStr path) {{",
                    "VFileOut o = v_read_file(path);",
                    "if (o.has_err) return {err};",
                    "return {ok};}}\n"
                ),
                rct = rct,
                err = none("o.err"),
                ok = some("o.text")
            ));
        }
        // `csv_rows(content, strict): Result<Vec<Vec<String>>, String>`.
        if self.has_inst("Result", &[rows_t.clone(), Ty::Str]) {
            let rct = ctype_inst("Result", &[rows_t.clone(), Ty::Str]);
            let vct = ctype_inst("Vec", std::slice::from_ref(&inner_vec_t));
            let some = |x: &str| {
                format!(
                    "({rct}){{ .tag = 0, .u = {{ .v0 = {{ .v___0 = {x} }} }} }}",
                    rct = rct,
                    x = x
                )
            };
            let none = |x: &str| {
                format!(
                    "({rct}){{ .tag = 1, .u = {{ .v1 = {{ .v___0 = {x} }} }} }}",
                    rct = rct,
                    x = x
                )
            };
            self.out.push_str(&format!(
                concat!(
                    "static {rct} v_csv_rows_wrap(VStr content, int strict) {{",
                    "VCsvTable t = {{NULL, 0, 0}};VStr emsg = {{NULL, 0}};",
                    "int64_t bad = vcsv_run(&t, NULL, content.s ? content.s : \"\", content.s ? content.len : 0, strict ? 0 : 1, &emsg);",
                    "if (bad) {{ vcsv_free_table(&t); return {err}; }}",
                    "{vct} rows;rows.data = ({vinner} *)t.rows;rows.len = t.nrows;rows.cap = t.caprows;",
                    "return {ok};}}\n"
                ),
                rct = rct,
                vct = vct,
                vinner = ctype_inst("Vec", std::slice::from_ref(&Ty::Str)),
                err = none("emsg"),
                ok = some("rows")
            ));
        }
        // `csv_bad_rows(content): Vec<Int64>` (rows strict mode rejects).
        if self.has_inst("Vec", &[Ty::Int64]) {
            let vct = ctype_inst("Vec", std::slice::from_ref(&Ty::Int64));
            self.out.push_str(&format!(
                concat!(
                    "static {vct} v_csv_bad_rows_wrap(VStr content) {{",
                    "VCsvTable t = {{NULL, 0, 0}};VBadRows bad = {{NULL, 0, 0}};VStr dummy = {{NULL, 0}};",
                    "vcsv_run(&t, &bad, content.s ? content.s : \"\", content.s ? content.len : 0, 1, &dummy);",
                    "vcsv_free_table(&t);",
                    "{vct} out;out.data = bad.rows;out.len = bad.n;out.cap = bad.cap;",
                    "return out;}}\n"
                ),
                vct = vct
            ));
        }
        // `json_rows(content): Result<Vec<Vec<String>>, String>` (row 0 = keys).
        if self.has_inst("Result", &[rows_t.clone(), Ty::Str]) {
            let rct = ctype_inst("Result", &[rows_t.clone(), Ty::Str]);
            let vct = ctype_inst("Vec", std::slice::from_ref(&inner_vec_t));
            let some = |x: &str| {
                format!(
                    "({rct}){{ .tag = 0, .u = {{ .v0 = {{ .v___0 = {x} }} }} }}",
                    rct = rct,
                    x = x
                )
            };
            let none = |x: &str| {
                format!(
                    "({rct}){{ .tag = 1, .u = {{ .v1 = {{ .v___0 = {x} }} }} }}",
                    rct = rct,
                    x = x
                )
            };
            self.out.push_str(&format!(
                concat!(
                    "static {rct} v_json_rows_wrap(VStr content) {{",
                    "VCsvTable t = {{NULL, 0, 0}};VStr emsg = {{NULL, 0}};",
                    "int64_t bad = vjson_parse(&t, content.s ? content.s : \"\", content.s ? content.len : 0, &emsg);",
                    "if (bad) {{ vcsv_free_table(&t); return {err}; }}",
                    "{vct} rows;rows.data = ({vinner} *)t.rows;rows.len = t.nrows;rows.cap = t.caprows;",
                    "return {ok};}}\n"
                ),
                rct = rct,
                vct = vct,
                vinner = ctype_inst("Vec", std::slice::from_ref(&Ty::Str)),
                err = none("emsg"),
                ok = some("rows")
            ));
        }
        Ok(())
    }

    /// Dynamic-array runtime + numeric kernels for one `Vec<T>`.
    /// `Option<T>` results reuse the Option instantiation machinery.
    fn emit_vec_fns(&mut self, args: &[Ty]) -> Result<(), Ice> {
        let t = args.first().cloned().unwrap_or(Ty::Unit);
        let ct = ctype_inst("Vec", std::slice::from_ref(&t));
        let iname = ct.strip_prefix('V').unwrap_or(&ct).to_string();
        let et = self.cty(&t);
        let oct = ctype_inst("Option", std::slice::from_ref(&t));
        let shape = Ty::Struct("ShapeError".to_string());
        let shape_ct = ctype_name("ShapeError");
        self.out.push_str(&format!(
            "static {ct} vvec_{iname}_new(void) {{ {ct} v = {{NULL, 0, 0}}; return v; }}\n",
            ct = ct,
            iname = iname
        ));
        self.out.push_str(&format!(
            "static void vvec_{iname}_push({ct} *v, {et} x) {{\nif (v->len == v->cap) {{\nint64_t ncap = v->cap ? v->cap * 2 : 4;\n{et} *nd = ({et} *)realloc(v->data, (size_t)ncap * sizeof({et}));\nif (!nd) {{ fprintf(stderr, \"alloc error\\n\"); abort(); }}\nv->data = nd; v->cap = ncap;\n}}\nv->data[v->len++] = x;\n}}\n",
            iname = iname,
            ct = ct,
            et = et
        ));
        self.out.push_str(&format!(
            "static {opt} vvec_{iname}_pop({ct} *v) {{\nif (v->len == 0) {{ return ({opt}){{ .tag = 1 }}; }}\n--v->len;\nreturn ({opt}){{ .tag = 0, .u = {{ .v0 = {{ .v___0 = v->data[v->len] }} }} }};\n}}\n",
            opt = oct,
            iname = iname,
            ct = ct
        ));
        // get clones the element out (never aliases the buffer).
        let get_clone = if self.type_needs_drop(&t) {
            self.clone_call(&t, "v->data[i]")
        } else {
            "v->data[i]".to_string()
        };
        self.out.push_str(&format!(
            "static {opt} vvec_{iname}_get({ct} *v, int64_t i) {{\nif (i < 0 || i >= v->len) {{ return ({opt}){{ .tag = 1 }}; }}\nreturn ({opt}){{ .tag = 0, .u = {{ .v0 = {{ .v___0 = {gc} }} }} }};\n}}\n",
            opt = oct,
            iname = iname,
            ct = ct,
            gc = get_clone
        ));
        self.out.push_str(&format!(
            "static int64_t vvec_{iname}_len({ct} *v) {{ return v->len; }}\n",
            iname = iname,
            ct = ct
        ));
        // Numeric kernels (Float64/Int32/Int64 share shapes).
        if matches!(t, Ty::Float64 | Ty::Int32 | Ty::Int64) {
            let vt = Ty::Generic {
                base: "Vec".to_string(),
                args: vec![t.clone()],
            };
            let vct = ctype_inst("Vec", std::slice::from_ref(&t));
            let rct = ctype_inst("Result", &[vt.clone(), shape.clone()]);
            self.out.push_str(&format!(
                "static {vct} vvec_{iname}_add_scalar({vct} *v, {et} s) {{\n{vct} r = vvec_{iname}_new();\nfor (int64_t i = 0; i < v->len; ++i) {{ {et} x = v->data[i] + s; vvec_{iname}_push(&r, x); }}\nreturn r;\n}}\n",
                vct = vct,
                iname = iname,
                et = et
            ));
            let shape_lit = |a: &str, b: &str| {
                format!(
                    "({}){{ .tag = 1, .u = {{ .v1 = {{ .v___0 = ({}){{ .v_expected = {}, .v_found = {} }} }} }} }}",
                    rct, shape_ct, a, b
                )
            };
            // add/mul/dot reference their Result layouts: emit only when used.
            let vt_here = Ty::Generic {
                base: "Vec".to_string(),
                args: vec![t.clone()],
            };
            let want_vec_res = self.has_inst("Result", &[vt_here.clone(), shape.clone()]);
            if want_vec_res {
                self.out.push_str(&format!(
                    "static {rct} vvec_{iname}_add({vct} *a, {vct} *b) {{\nif (a->len != b->len) {{ return {shape}; }}\n{vct} r = vvec_{iname}_new();\nfor (int64_t i = 0; i < a->len; ++i) {{ {et} x = a->data[i] + b->data[i]; vvec_{iname}_push(&r, x); }}\n{rct} o = {{ .tag = 0, .u = {{ .v0 = {{ .v___0 = r }} }} }};\nreturn o;\n}}\n",
                    rct = rct,
                    iname = iname,
                    vct = vct,
                    et = et,
                    shape = shape_lit("a->len", "b->len")
                ));
                self.out.push_str(&format!(
                    "static {rct} vvec_{iname}_mul({vct} *a, {vct} *b) {{\nif (a->len != b->len) {{ return {shape}; }}\n{vct} r = vvec_{iname}_new();\nfor (int64_t i = 0; i < a->len; ++i) {{ {et} x = a->data[i] * b->data[i]; vvec_{iname}_push(&r, x); }}\n{rct} o = {{ .tag = 0, .u = {{ .v0 = {{ .v___0 = r }} }} }};\nreturn o;\n}}\n",
                    rct = rct,
                    iname = iname,
                    vct = vct,
                    et = et,
                    shape = shape_lit("a->len", "b->len")
                ));
            }
            // casts only to recorded target instantiations (their layouts
            // and runtimes come from their own emit pass).
            for dt in [Ty::Float64, Ty::Int32, Ty::Int64] {
                if dt == t {
                    continue;
                }
                if !self.has_inst("Vec", std::slice::from_ref(&dt)) {
                    continue;
                }
                let dct = self.cty(&dt);
                let appet = match (&t, &dt) {
                    (Ty::Float64, Ty::Int32) => "v_f2i32(v->data[i])",
                    (Ty::Float64, Ty::Int64) => "v_f2i64(v->data[i])",
                    (Ty::Int32, Ty::Int64) => "(int64_t)(v->data[i])",
                    (Ty::Int64, Ty::Int32) => "(int32_t)(v->data[i])",
                    _ => "(double)(v->data[i])",
                };
                self.out.push_str(&format!(
                    "static {vct2} vvec_{iname}_cast_{iname2}({vct} *v) {{\n{vct2} r = vvec_{iname2}_new();\nfor (int64_t i = 0; i < v->len; ++i) {{ {dct} x = {app}; vvec_{iname2}_push(&r, x); }}\nreturn r;\n}}\n",
                    vct2 = ctype_inst("Vec", std::slice::from_ref(&dt)),
                    iname2 = ctype_inst("Vec", std::slice::from_ref(&dt))
                        .strip_prefix('V')
                        .unwrap_or("X"),
                    iname = iname,
                    vct = vct,
                    dct = dct,
                    app = appet
                ));
            }
            if t == Ty::Float64 && self.has_inst("Result", &[Ty::Float64, shape.clone()]) {
                let rf = Ty::Generic {
                    base: "Result".to_string(),
                    args: vec![Ty::Float64, shape.clone()],
                };
                let rfct = ctype_inst("Result", &[Ty::Float64, shape]);
                let dot_shape = format!(
                    "({}){{ .tag = 1, .u = {{ .v1 = {{ .v___0 = ({}){{ .v_expected = a->len, .v_found = b->len }} }} }} }}",
                    rfct, shape_ct
                );
                self.out.push_str(&format!(
                    "static {rfct} vvec_{iname}_dot({vct} *a, {vct} *b) {{\nif (a->len != b->len) {{ return {shape}; }}\ndouble acc = 0.0;\nfor (int64_t i = 0; i < a->len; ++i) {{ acc += a->data[i] * b->data[i]; }}\n{rfct} o = {{ .tag = 0, .u = {{ .v0 = {{ .v___0 = acc }} }} }};\nreturn o;\n}}\n",
                    rfct = rfct,
                    iname = iname,
                    vct = vct,
                    shape = dot_shape
                ));
                let _ = rf;
            }
        }
        Ok(())
    }

    /// Drop for one `Vec<T>`: element drops (if owned) + buffer free.
    /// Neutering sets data NULL so drops skip moved-from vectors.
    fn emit_vec_drop(&mut self, args: &[Ty]) -> Result<(), Ice> {
        let t = args.first().cloned().unwrap_or(Ty::Unit);
        let ct = ctype_inst("Vec", std::slice::from_ref(&t));
        let iname = ct.strip_prefix('V').unwrap_or(&ct).to_string();
        self.out.push_str(&format!(
            "static void v_drop_{iname}({ct} *v) {{\nif (v->data) {{\n"
        ));
        if self.type_needs_drop(&t) {
            let stmt = match &t {
                Ty::Str => "vstr_free(&v->data[i]);".to_string(),
                _ => format!("{};", self.drop_call(&t, "v->data[i]")),
            };
            self.out.push_str(&format!(
                "for (int64_t i = 0; i < v->len; ++i) {{ {} }}\n",
                stmt
            ));
        }
        self.out
            .push_str("free(v->data); v->data = NULL; v->len = 0; v->cap = 0;\n}\n}\n");
        Ok(())
    }

    /// Deep clone for one `Vec<T>`.
    fn emit_vec_clone(&mut self, args: &[Ty]) -> Result<(), Ice> {
        let t = args.first().cloned().unwrap_or(Ty::Unit);
        let ct = ctype_inst("Vec", std::slice::from_ref(&t));
        let iname = ct.strip_prefix('V').unwrap_or(&ct).to_string();
        let et = self.cty(&t);
        self.out.push_str(&format!(
            "static {ct} v_clone_{iname}({ct} v) {{\n{ct} c = {{NULL, 0, 0}};\nif (v.len > 0) {{\nc.data = ({et} *)malloc((size_t)v.len * sizeof({et}));\nif (!c.data) {{ fprintf(stderr, \"alloc error\\n\"); abort(); }}\nc.len = v.len; c.cap = v.len;\nfor (int64_t i = 0; i < v.len; ++i) {{\n"
        ));
        if self.type_needs_drop(&t) {
            let cp = match &t {
                Ty::Str => "vstr_clone(v.data[i])".to_string(),
                _ => self.clone_call(&t, "v.data[i]"),
            };
            self.out.push_str(&format!("c.data[i] = {};\n", cp));
        } else {
            self.out.push_str("c.data[i] = v.data[i];\n");
        }
        self.out.push_str("}\n}\nreturn c;\n}\n");
        Ok(())
    }

    /// Value-returning `Vec` method call. Receiver borrowed via `rptr`
    /// (`&slot`); owned arguments transfer (caller nulls via consume).
    /// `res_ty` is the checked result type (needed by `cast`).
    fn emit_vec_method(
        &mut self,
        method: &str,
        _elem_ty: &Ty,
        iname: &str,
        r: &str,
        args: &[Expr],
        res_ty: &Ty,
    ) -> Result<String, Ice> {
        let rptr = format!("&{}", r);
        let arg0 = |this: &mut Self| -> Result<String, Ice> {
            let a = args.first().ok_or_else(|| Ice("method arity".into()))?;
            let saved = this.cur_move;
            this.cur_move = false;
            let v = this.emit_expr(a)?;
            this.cur_move = saved;
            Ok(v)
        };
        match method {
            "pop" => Ok(format!("vvec_{}_pop({})", iname, rptr)),
            "get" => {
                let v = arg0(self)?;
                // Index accepts Int32 (widened) or Int64.
                Ok(format!("vvec_{}_get({}, (int64_t)({}))", iname, rptr, v))
            }
            "clone" => Ok(format!("v_clone_{}({})", iname, r)),
            "cast" => {
                // Destination from the checked result type.
                let (dst, diname) = match res_ty {
                    Ty::Generic { base, args } if base == "Vec" && args.len() == 1 => {
                        let d = args[0].clone();
                        let dc = ctype_inst(base, args);
                        (d, dc.strip_prefix('V').unwrap_or(&dc).to_string())
                    }
                    _ => return Err(Ice("cast result not a Vec".into())),
                };
                let _ = dst;
                Ok(format!("vvec_{}_cast_{}({})", iname, diname, rptr))
            }
            "add_scalar" => {
                let v = arg0(self)?;
                Ok(format!("vvec_{}_add_scalar({}, {})", iname, rptr, v))
            }
            "add" => {
                let a = args.first().ok_or_else(|| Ice("method arity".into()))?;
                let saved = self.cur_move;
                self.cur_move = false;
                let o = self.mat_str(a)?;
                self.cur_move = saved;
                Ok(format!("vvec_{}_add({}, &{})", iname, rptr, o))
            }
            "mul" => {
                let a = args.first().ok_or_else(|| Ice("method arity".into()))?;
                let saved = self.cur_move;
                self.cur_move = false;
                let o = self.mat_str(a)?;
                self.cur_move = saved;
                Ok(format!("vvec_{}_mul({}, &{})", iname, rptr, o))
            }
            "dot" => {
                let a = args.first().ok_or_else(|| Ice("method arity".into()))?;
                let saved = self.cur_move;
                self.cur_move = false;
                let o = self.mat_str(a)?;
                self.cur_move = saved;
                Ok(format!("vvec_{}_dot({}, &{})", iname, rptr, o))
            }
            _ => Err(Ice("unknown Vec method".into())),
        }
    }

    /// Drop helper for one instantiation (`v_drop_Option_Int32`, ...).
    fn emit_inst_drop(&mut self, base: &str, args: &[Ty]) -> Result<(), Ice> {
        let ct = ctype_inst(base, args);
        let iname = ct.strip_prefix('V').unwrap_or(&ct).to_string();
        let variants = self.generic_variants(base, args);
        self.out.push_str(&format!(
            "static void v_drop_{}({} *v) {{\nswitch (v->tag) {{\n",
            iname, ct
        ));
        for (i, (_, pts)) in variants.iter().enumerate() {
            let mut body = String::new();
            for (j, pt) in pts.iter().enumerate() {
                let path = format!("v->u.{}.{}", cmember(i), cfield(&format!("_{}", j)));
                match pt {
                    Ty::Str => {
                        body.push_str(&format!("vstr_free(&{});", path));
                    }
                    _ if self.type_needs_drop(pt) => {
                        body.push_str(&format!("{};", self.drop_call(pt, &path)));
                    }
                    _ => {}
                }
            }
            if !body.is_empty() {
                self.out.push_str(&format!("case {}: {}break;\n", i, body));
            }
        }
        self.out.push_str("default: break;\n}\n}\n");
        Ok(())
    }

    /// Clone helper for one instantiation.
    fn emit_inst_clone(&mut self, base: &str, args: &[Ty]) -> Result<(), Ice> {
        let ct = ctype_inst(base, args);
        let iname = ct.strip_prefix('V').unwrap_or(&ct).to_string();
        let variants = self.generic_variants(base, args);
        self.out.push_str(&format!(
            "static {} v_clone_{}({} v) {{\n{} c;\nc.tag = v.tag;\nswitch (v.tag) {{\n",
            ct, iname, ct, ct
        ));
        for (i, (_, pts)) in variants.iter().enumerate() {
            let mut body = String::new();
            for (j, pt) in pts.iter().enumerate() {
                let (src, dst) = (
                    format!("v.u.{}.{}", cmember(i), cfield(&format!("_{}", j))),
                    format!("c.u.{}.{}", cmember(i), cfield(&format!("_{}", j))),
                );
                match pt {
                    Ty::Str => {
                        body.push_str(&format!("{} = vstr_clone({});", dst, src));
                    }
                    _ if self.type_needs_drop(pt) => {
                        body.push_str(&format!("{} = {};", dst, self.clone_call(pt, &src)));
                    }
                    _ => {
                        body.push_str(&format!("{} = {};", dst, src));
                    }
                }
            }
            if !body.is_empty() {
                self.out.push_str(&format!("case {}: {}break;\n", i, body));
            }
        }
        self.out.push_str("default: break;\n}\nreturn c;\n}\n");
        Ok(())
    }

    /// `vfmt_X(v)` (no trailing newline, for nesting) + `vprint_X(v)`
    /// (`vfmt` + newline, for top-level `println`) for one printed `Vec` /
    /// `Option` / `Result` instantiation. Sema guarantees printable args, so
    /// every element dispatches to an existing formatter.
    fn emit_print_fmt(&mut self, base: &str, args: &[Ty]) -> Result<(), Ice> {
        let ct = ctype_inst(base, args);
        let iname = iname_of(&ct);
        let mut body = String::new();
        match (base, args) {
            ("Vec", [t]) => {
                let el = self.fmt_call(t, "v.data[i]")?;
                body.push_str("printf(\"[\");\n");
                body.push_str("for (int64_t i = 0; i < v.len; ++i) { if (i) printf(\", \");\n");
                body.push_str(&format!("{};\n}}\n", el));
                body.push_str("printf(\"]\");\n");
            }
            ("Option", [t]) => {
                let some = self.fmt_call(t, &format!("v.u.{}.{}", cmember(0), cfield("_0")))?;
                body.push_str("if (v.tag == 1) { printf(\"None\"); }\n");
                body.push_str(&format!(
                    "else {{ printf(\"Some(\"); {}; printf(\")\"); }}\n",
                    some
                ));
            }
            ("Missing", [t]) => {
                let present = self.fmt_call(t, &format!("v.u.{}.{}", cmember(0), cfield("_0")))?;
                body.push_str("if (v.tag == 1) { printf(\"NA\"); }\n");
                body.push_str(&format!(
                    "else {{ printf(\"Present(\"); {}; printf(\")\"); }}\n",
                    present
                ));
            }
            ("Result", [t, e]) => {
                let ok = self.fmt_call(t, &format!("v.u.{}.{}", cmember(0), cfield("_0")))?;
                let err = self.fmt_call(e, &format!("v.u.{}.{}", cmember(1), cfield("_0")))?;
                body.push_str(&format!(
                    "if (v.tag == 1) {{ printf(\"Err(\"); {}; printf(\")\"); }}\n",
                    err
                ));
                body.push_str(&format!(
                    "else {{ printf(\"Ok(\"); {}; printf(\")\"); }}\n",
                    ok
                ));
            }
            _ => return Err(Ice(format!("unprintable instantiation {}<..>", base))),
        }
        self.out.push_str(&format!(
            "static void vfmt_{}({} v) {{\n{}}}\n",
            iname, ct, body
        ));
        self.out.push_str(&format!(
            "static void vprint_{}({} v) {{ vfmt_{}(v); printf(\"\\n\"); }}\n",
            iname, ct, iname
        ));
        Ok(())
    }

    /// `vfmt_X(path)` expression formatting one value of a printable type.
    fn fmt_call(&self, ty: &Ty, path: &str) -> Result<String, Ice> {
        match ty {
            Ty::Int32 => Ok(format!("vfmt_i32({})", path)),
            Ty::Int64 => Ok(format!("vfmt_i64({})", path)),
            Ty::Float64 => Ok(format!("vfmt_f64({})", path)),
            Ty::Bool => Ok(format!("vfmt_bool({})", path)),
            Ty::Str => Ok(format!("vfmt_str({})", path)),
            Ty::Generic { base, args }
                if (base == "Vec" || base == "Option" || base == "Missing") && args.len() == 1 =>
            {
                Ok(format!(
                    "vfmt_{}({})",
                    iname_of(&ctype_inst(base, args)),
                    path
                ))
            }
            Ty::Generic { base, args } if base == "Result" && args.len() == 2 => Ok(format!(
                "vfmt_{}({})",
                iname_of(&ctype_inst(base, args)),
                path
            )),
            _ => Err(Ice(format!("unprintable type in formatter: {}", ty.name()))),
        }
    }

    /// `v_drop_X(p)` / `v_clone_X(p)` expression for an owned payload type.
    fn drop_call(&self, ty: &Ty, path: &str) -> String {
        match ty {
            Ty::Str => format!("vstr_free(&{})", path),
            Ty::Struct(n) | Ty::Enum(n) => format!("v_drop_{}(&{})", dname(n), path),
            Ty::Generic { base, args } => format!(
                "v_drop_{}(&{})",
                ctype_inst(base, args).strip_prefix('V').unwrap_or("X"),
                path
            ),
            _ => String::new(),
        }
    }

    fn clone_call(&self, ty: &Ty, path: &str) -> String {
        match ty {
            Ty::Str => format!("vstr_clone({})", path),
            Ty::Struct(n) | Ty::Enum(n) => format!("v_clone_{}({})", dname(n), path),
            Ty::Generic { base, args } => format!(
                "v_clone_{}({})",
                ctype_inst(base, args).strip_prefix('V').unwrap_or("X"),
                path
            ),
            _ => path.to_string(),
        }
    }

    fn type_needs_drop(&self, ty: &Ty) -> bool {
        !is_copy_ty(ty, &self.structs, &self.enums)
    }

    /// Whether an instantiation was recorded (i.e. some kernel may use it).
    fn has_inst(&self, base: &str, args: &[Ty]) -> bool {
        self.generic_insts
            .values()
            .any(|(b, a)| b == base && a == args)
    }

    /// (variant name, payload types) for a builtin generic instantiation.
    /// Tags: `Some`=0/`None`=1, `Ok`=0/`Err`=1.
    fn generic_variants(&self, base: &str, args: &[Ty]) -> Vec<(String, Vec<Ty>)> {
        match (base, args) {
            ("Option", [t]) => vec![
                ("Some".to_string(), vec![t.clone()]),
                ("None".to_string(), vec![]),
            ],
            ("Missing", [t]) => vec![
                ("Present".to_string(), vec![t.clone()]),
                ("Missing".to_string(), vec![]),
            ],
            ("Result", [t, e]) => vec![
                ("Ok".to_string(), vec![t.clone()]),
                ("Err".to_string(), vec![e.clone()]),
            ],
            _ => vec![],
        }
    }

    /// Resolve a variant pattern/test against an enum or builtin generic.
    /// Returns (C type name, tag index, payload types).
    fn resolve_variant(
        &self,
        ty_opt: &Option<String>,
        variant: &str,
        scrut_ty: &Ty,
    ) -> Result<(String, usize, Vec<Ty>), Ice> {
        // Qualified `Option::Some` / `Result::Ok` / `Missing::Present`
        // (base name only).
        if let Some(t) = ty_opt {
            if t == "Option" || t == "Result" || t == "Missing" {
                match scrut_ty {
                    Ty::Generic { base, args } if base == t => {
                        let vs = self.generic_variants(base, args);
                        return vs
                            .iter()
                            .enumerate()
                            .find(|(_, (v, _))| v == variant)
                            .map(|(i, (_, p))| (ctype_inst(base, args), i, p.clone()))
                            .ok_or_else(|| Ice("unknown variant".into()));
                    }
                    _ => return Err(Ice("variant type mismatch".into())),
                }
            }
            // Declared enum.
            let def = self
                .enums
                .get(t)
                .cloned()
                .ok_or_else(|| Ice(format!("unknown enum {}", t)))?;
            return def
                .variants
                .iter()
                .enumerate()
                .find(|(_, (v, _))| v == variant)
                .map(|(i, (_, p))| (ctype_name(t), i, p.clone()))
                .ok_or_else(|| Ice("unknown variant".into()));
        }
        // Bare: scrutinee enum or builtin generic.
        match scrut_ty {
            Ty::Enum(n) => {
                let def = self
                    .enums
                    .get(n)
                    .cloned()
                    .ok_or_else(|| Ice(format!("unknown enum {}", n)))?;
                def.variants
                    .iter()
                    .enumerate()
                    .find(|(_, (v, _))| v == variant)
                    .map(|(i, (_, p))| (ctype_name(n), i, p.clone()))
                    .ok_or_else(|| Ice("unknown variant".into()))
            }
            Ty::Generic { base, args }
                if base == "Option" || base == "Result" || base == "Missing" =>
            {
                let vs = self.generic_variants(base, args);
                vs.iter()
                    .enumerate()
                    .find(|(_, (v, _))| v == variant)
                    .map(|(i, (_, p))| (ctype_inst(base, args), i, p.clone()))
                    .ok_or_else(|| Ice("unknown variant".into()))
            }
            _ => Err(Ice("bare variant on non-enum".into())),
        }
    }

    fn emit_struct_drop(&mut self, n: &str) -> Result<(), Ice> {
        let def = self.structs.get(n).cloned().unwrap_or(StructDef {
            fields: vec![],
            is_tuple: false,
        });
        self.out.push_str(&format!(
            "static void v_drop_{}({} *v) {{\n",
            dname(n),
            ctype_name(n)
        ));
        for (f, ft) in &def.fields {
            let path = format!("v->{}", cfield(f));
            // Dispatch covers owned Strings, nominal ADTs, and generic
            // (`Vec`/`Option`/`Result`) fields alike.
            if self.type_needs_drop(ft) {
                self.out
                    .push_str(&format!("{};\n", self.drop_call(ft, &path)));
            }
        }
        self.out.push_str("}\n");
        Ok(())
    }

    fn emit_struct_clone(&mut self, n: &str) -> Result<(), Ice> {
        let def = self.structs.get(n).cloned().unwrap_or(StructDef {
            fields: vec![],
            is_tuple: false,
        });
        let ct = ctype_name(n);
        self.out.push_str(&format!(
            "static {} v_clone_{}({} v) {{\n",
            ct,
            dname(n),
            ct
        ));
        self.out.push_str(&format!("{} c;\n", ct));
        for (f, ft) in &def.fields {
            let (src, dst) = (format!("v.{}", cfield(f)), format!("c.{}", cfield(f)));
            if self.type_needs_drop(ft) {
                self.out
                    .push_str(&format!("{} = {};\n", dst, self.clone_call(ft, &src)));
            } else {
                self.out.push_str(&format!("{} = {};\n", dst, src));
            }
        }
        self.out.push_str("return c;\n}\n");
        Ok(())
    }

    fn emit_enum_drop(&mut self, n: &str) -> Result<(), Ice> {
        let def = self
            .enums
            .get(n)
            .cloned()
            .unwrap_or(EnumDef { variants: vec![] });
        self.out.push_str(&format!(
            "static void v_drop_{}({} *v) {{\nswitch (v->tag) {{\n",
            dname(n),
            ctype_name(n)
        ));
        for (i, (_, pts)) in def.variants.iter().enumerate() {
            let mut body = String::new();
            for (j, pt) in pts.iter().enumerate() {
                let path = format!("v->u.{}.{}", cmember(i), cfield(&format!("_{}", j)));
                if self.type_needs_drop(pt) {
                    body.push_str(&format!("{};", self.drop_call(pt, &path)));
                }
            }
            if !body.is_empty() {
                self.out.push_str(&format!("case {}: {}break;\n", i, body));
            }
        }
        self.out.push_str("default: break;\n}\n}\n");
        Ok(())
    }

    fn emit_enum_clone(&mut self, n: &str) -> Result<(), Ice> {
        let def = self
            .enums
            .get(n)
            .cloned()
            .unwrap_or(EnumDef { variants: vec![] });
        let ct = ctype_name(n);
        self.out.push_str(&format!(
            "static {} v_clone_{}({} v) {{\n{} c;\nc.tag = v.tag;\nswitch (v.tag) {{\n",
            ct,
            dname(n),
            ct,
            ct
        ));
        for (i, (_, pts)) in def.variants.iter().enumerate() {
            let mut body = String::new();
            for (j, pt) in pts.iter().enumerate() {
                let (src, dst) = (
                    format!("v.u.{}.{}", cmember(i), cfield(&format!("_{}", j))),
                    format!("c.u.{}.{}", cmember(i), cfield(&format!("_{}", j))),
                );
                if self.type_needs_drop(pt) {
                    body.push_str(&format!("{} = {};", dst, self.clone_call(pt, &src)));
                } else {
                    body.push_str(&format!("{} = {};", dst, src));
                }
            }
            if !body.is_empty() {
                self.out.push_str(&format!("case {}: {}break;\n", i, body));
            }
        }
        self.out.push_str("default: break;\n}\nreturn c;\n}\n");
        Ok(())
    }

    // ----- functions -----

    fn emit_fun(&mut self, f: &FunDecl) -> Result<(), Ice> {
        self.scopes.clear();
        self.owned.clear();
        self.vartypes.clear();
        self.cur_module = f.module.clone();
        let is_main = f.name == "main";
        // `main` returning `Result<Int32, E>` / `Option<Int32>` compiles the
        // body as an inner function; a small wrapper maps to exit codes.
        let main_result = if is_main {
            match &f.ret {
                Ty::Generic { base, args }
                    if (base == "Result" && args.len() == 2)
                        || (base == "Option" && args.len() == 1) =>
                {
                    Some(f.ret.clone())
                }
                _ => None,
            }
        } else {
            None
        };
        let ret = if main_result.is_some() {
            self.cty(&f.ret)
        } else if is_main {
            "int".to_string()
        } else {
            self.cty(&f.ret).to_string()
        };
        if main_result.is_some() {
            self.out.push_str(&format!(
                "static {} {}_fn(void) {{\n",
                ret,
                mangle("valvet_main")
            ));
        } else if is_main {
            self.out.push_str("int main(void) {\n");
        } else if f.params.is_empty() {
            self.out.push_str(&format!(
                "static {} {}_fn(void) {{\n",
                ret,
                cfun(&f.module, &f.name)
            ));
        } else {
            self.out.push_str(&format!(
                "static {} {}_fn({}) {{\n",
                ret,
                cfun(&f.module, &f.name),
                f.params
                    .iter()
                    .map(|p| format!("{} {}", self.cty(&p.ty), mangle(&p.name)))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        self.scopes.push(vec![]);
        for p in &f.params {
            self.vartypes.insert(mangle(&p.name), p.ty.clone());
            if self.moves(&p.ty) {
                self.scopes
                    .last_mut()
                    .unwrap()
                    .push((mangle(&p.name), p.ty.clone()));
                self.owned.insert(mangle(&p.name), p.ty.clone());
            }
        }
        let want_ret = if main_result.is_some() {
            f.ret.clone()
        } else if is_main {
            match f.ret {
                Ty::Int32 => Ty::Int32,
                _ => Ty::Unit,
            }
        } else {
            f.ret.clone()
        };
        self.fn_ret = want_ret.clone();
        self.emit_block(&f.body, &want_ret, true)?;
        self.emit_scope_frees(self.scopes.len() - 1);
        if main_result.is_some() {
            self.out.push_str("}\n\n");
            self.scopes.pop();
            // Exit-code wrapper: Ok(n)/Some(n) -> n; Err(e) -> drop + 1.
            let rct = self.cty(&f.ret);
            self.out.push_str("int main(void) {\n");
            self.out
                .push_str(&format!("{} _vr = {}_fn();\n", rct, mangle("valvet_main")));
            self.out.push_str("if (_vr.tag == 0) {\n");
            self.out.push_str("return _vr.u.v0.v___0;\n");
            self.out.push_str("} else {\n");
            // Err payload (if any) must be destroyed before exiting — but only
            // when it needs dropping (all-Copy instantiations emit no helper).
            if self.type_needs_drop(&f.ret) {
                self.out.push_str(&format!(
                    "v_drop_{}(&_vr); return 1; }}\n}}\n",
                    rct.strip_prefix('V').unwrap_or(&rct)
                ));
            } else {
                self.out.push_str("return 1; }\n}\n");
            }
            return Ok(());
        }
        if is_main {
            self.out.push_str("return 0;\n");
        } else if f.ret != Ty::Unit {
            self.out
                .push_str("{ fprintf(stderr, \"missing return\\n\"); abort(); }\n");
        }
        self.out.push_str("}\n\n");
        self.scopes.pop();
        Ok(())
    }

    /// Emit block. If `is_fn_body`, tail becomes `return`.
    fn emit_block(&mut self, b: &Block, ret_ty: &Ty, is_fn_body: bool) -> Result<(), Ice> {
        self.scopes.push(vec![]);
        let depth = self.scopes.len() - 1;
        self.emit_block_body_only(b, ret_ty, is_fn_body)?;
        self.emit_scope_frees(depth);
        self.scopes.pop();
        Ok(())
    }

    /// Block statements + tail without scope push/pop (caller owns the scope,
    /// e.g. `for`-each loop-variable scope).
    fn emit_block_body_only(
        &mut self,
        b: &Block,
        ret_ty: &Ty,
        is_fn_body: bool,
    ) -> Result<(), Ice> {
        for s in &b.stmts {
            self.emit_stmt(s, ret_ty)?;
        }
        if let Some(t) = &b.tail {
            let tt = self.ty_of(t.id);
            if is_fn_body {
                self.emit_tail_return(t, &tt)?;
            } else if tt == Ty::Unit {
                self.push_frame();
                let saved = self.cur_move;
                self.cur_move = false;
                let e = self.emit_expr(t)?;
                self.cur_move = saved;
                self.out.push_str(&format!("{};\n", e));
                self.flush_nulls();
                self.pop_frame();
            } else {
                return Err(Ice("non-unit tail in non-function block".into()));
            }
        }
        Ok(())
    }

    fn emit_tail_return(&mut self, t: &Expr, tt: &Ty) -> Result<(), Ice> {
        self.push_frame();
        let saved = self.cur_move;
        if *tt == Ty::Unit {
            self.cur_move = false;
            let e = self.emit_expr(t)?;
            self.out.push_str(&format!("{};\n", e));
            self.flush_nulls();
            self.pop_frame();
        } else if self.moves(tt) {
            self.cur_move = true;
            let v = self.mat_str(t)?;
            let ret = self.fresh_tmp();
            self.out
                .push_str(&format!("{} {} = {};\n", self.cty_val(tt), ret, v));
            self.consume(&v);
            self.flush_nulls();
            self.pop_frame();
            self.emit_all_frees();
            self.out.push_str(&format!("return {};\n", ret));
        } else {
            self.cur_move = false;
            let e = self.emit_expr(t)?;
            // Materialize before frees: the expression may read owned locals
            // (e.g. `v.len()`), which frees would zero first (silent wrong answer).
            let ret = self.fresh_tmp();
            self.out
                .push_str(&format!("{} {} = {};\n", self.cty_val(tt), ret, e));
            self.flush_nulls();
            self.pop_frame();
            self.emit_all_frees();
            self.out.push_str(&format!("return {};\n", ret));
        }
        self.cur_move = saved;
        Ok(())
    }

    // ----- statements -----

    /// Shared `while` lowering (statement loops; `break`/`continue` target it).
    fn emit_while(&mut self, cond: &Expr, body: &Block, ret_ty: &Ty) -> Result<(), Ice> {
        self.out.push_str("while (1) {\n");
        self.push_frame();
        let saved = self.cur_move;
        self.cur_move = false;
        let c = self.emit_expr(cond)?;
        self.cur_move = saved;
        self.out.push_str(&format!("if (!({})) break;\n", c));
        self.flush_nulls();
        self.pop_frame();
        // Fresh C scope per iteration so body locals redeclare cleanly.
        self.out.push_str("{\n");
        self.loop_stack.push(LoopInfo {
            body_depth: self.scopes.len(),
            tmp_depth: self.tmp_frames.len(),
            value: None,
        });
        self.emit_block(body, ret_ty, false)?;
        self.loop_stack.pop();
        self.out.push_str("}\n");
        self.out.push_str("}\n");
        Ok(())
    }

    fn emit_stmt(&mut self, s: &Stmt, ret_ty: &Ty) -> Result<(), Ice> {
        match s {
            Stmt::Val {
                id, name, init, ty, ..
            }
            | Stmt::Var {
                id, name, init, ty, ..
            } => {
                self.push_frame();
                let it = self.ty_of(init.id);
                // Prefer sema's resolved declared type (AST annotations may
                // hold unresolved nominal types).
                let decl_ty = self
                    .checked
                    .types
                    .get(id)
                    .cloned()
                    .unwrap_or_else(|| ty.clone().unwrap_or(it));
                let cname = mangle(name);
                let saved = self.cur_move;
                if self.moves(&decl_ty) {
                    self.cur_move = true;
                    let v = self.mat_str(init)?;
                    self.out
                        .push_str(&format!("{} {} = {};\n", self.cty_val(&decl_ty), cname, v));
                    self.consume(&v);
                } else {
                    self.cur_move = false;
                    let rhs = self.emit_expr(init)?;
                    self.out.push_str(&format!(
                        "{} {} = {};\n",
                        self.cty_val(&decl_ty),
                        cname,
                        rhs
                    ));
                }
                self.cur_move = saved;
                self.flush_nulls();
                self.pop_frame();
                self.vartypes.insert(cname.clone(), decl_ty.clone());
                if self.moves(&decl_ty) {
                    self.scopes
                        .last_mut()
                        .unwrap()
                        .push((cname.clone(), decl_ty.clone()));
                    self.owned.insert(cname, decl_ty);
                }
                Ok(())
            }
            Stmt::Expr { expr, .. } => {
                self.push_frame();
                let saved = self.cur_move;
                if let ExprKind::Assign {
                    target, op, value, ..
                } = &expr.kind
                {
                    let vt = self.ty_of(value.id);
                    // J1 indexed store: check bounds first (index evaluates
                    // before the value), then reuse the plain-slot logic
                    // with the element slot as the target.
                    let cname = match target {
                        Place::Index { base, idx, .. } => {
                            if place_has_index(base) {
                                return Err(Ice("nested index store".into()));
                            }
                            let bv = place_lvalue(base, self);
                            let sv = self.cur_move;
                            self.cur_move = false;
                            let iw = self.emit_expr(idx)?;
                            self.cur_move = sv;
                            let i64_ = format!("(int64_t)({})", iw);
                            self.out.push_str(&format!(
                                "if ({i} < 0 || {i} >= {bv}.len) {{ fprintf(stderr, \"index out of bounds\\n\"); abort(); }}\n",
                                i = i64_,
                                bv = bv
                            ));
                            format!("{}.data[{}]", bv, i64_)
                        }
                        _ => place_lvalue(target, self),
                    };
                    match op {
                        AssignOp::Set => {
                            if self.moves(&vt) {
                                // Evaluate RHS first (it may borrow the target),
                                // then destroy old, install new.
                                self.cur_move = true;
                                let v = self.mat_str(value)?;
                                let hold = self.fresh_tmp();
                                self.out.push_str(&format!(
                                    "{} {} = {};\n",
                                    self.cty_val(&vt),
                                    hold,
                                    v
                                ));
                                self.consume(&v);
                                self.flush_nulls();
                                self.pop_frame();
                                self.emit_destroy(&cname, &vt);
                                self.out.push_str(&format!("{} = {};\n", cname, hold));
                            } else {
                                self.cur_move = false;
                                let rhs = self.emit_expr(value)?;
                                self.out.push_str(&format!("{} = {};\n", cname, rhs));
                                self.flush_nulls();
                                self.pop_frame();
                            }
                        }
                        _ => {
                            if *op == AssignOp::Add && vt == Ty::Str {
                                // `s += e` on strings desugars to `s = s + e`:
                                // concat copies both sides, so hold the fresh
                                // value, destroy the old, then install.
                                self.cur_move = false;
                                let r = self.mat_str(value)?;
                                let hold = self.fresh_tmp();
                                self.out.push_str(&format!(
                                    "VStr {} = vstr_concat({}, {});\n",
                                    hold, cname, r
                                ));
                                self.flush_nulls();
                                self.pop_frame();
                                self.emit_destroy(&cname, &vt);
                                self.out.push_str(&format!("{} = {};\n", cname, hold));
                            } else {
                                self.cur_move = false;
                                let rhs = self.emit_expr(value)?;
                                let cop = match op {
                                    AssignOp::Add => "+=",
                                    AssignOp::Sub => "-=",
                                    AssignOp::Mul => "*=",
                                    AssignOp::Div => "/=",
                                    _ => "%=",
                                };
                                self.out.push_str(&format!("{} {} {};\n", cname, cop, rhs));
                                self.flush_nulls();
                                self.pop_frame();
                            }
                        }
                    }
                    self.cur_move = saved;
                    return Ok(());
                }
                self.cur_move = false;
                let e = self.emit_expr(expr)?;
                self.cur_move = saved;
                self.out.push_str(&format!("{};\n", e));
                self.flush_nulls();
                self.pop_frame();
                Ok(())
            }
            Stmt::If {
                cond,
                then_b,
                else_b,
                ..
            } => {
                self.push_frame();
                let saved = self.cur_move;
                self.cur_move = false;
                let c = self.emit_expr(cond)?;
                self.cur_move = saved;
                // Header first: cond temps stay alive through the branches
                // and are freed after (freeing before would be use-after-free).
                self.out.push_str(&format!("if ({}) {{\n", c));
                self.emit_block(then_b, ret_ty, false)?;
                self.out.push('}');
                if let Some(e) = else_b {
                    self.out.push_str(" else {\n");
                    self.emit_block(e, ret_ty, false)?;
                    self.out.push('}');
                }
                self.out.push('\n');
                self.flush_nulls();
                self.pop_frame();
                Ok(())
            }
            Stmt::While { cond, body, .. } => {
                self.emit_while(cond, body, ret_ty)?;
                Ok(())
            }
            Stmt::For {
                var, iter, body, ..
            } => {
                match iter {
                    ForIter::Range { lo, hi, inclusive } => {
                        // Bounds are ints (sema); hoist into temps so they
                        // evaluate once.
                        self.push_frame();
                        let saved = self.cur_move;
                        self.cur_move = false;
                        let lo_s = self.emit_expr(lo)?;
                        let hi_s = self.emit_expr(hi)?;
                        self.cur_move = saved;
                        self.flush_nulls();
                        self.pop_frame();
                        let t = self.ty_of(lo.id);
                        let ct = self.cty(&t);
                        let lo_t = self.fresh_tmp();
                        let hi_t = self.fresh_tmp();
                        let cvar = mangle(var);
                        let cmp = if *inclusive { "<=" } else { "<" };
                        self.out.push_str("{\n");
                        self.out.push_str(&format!("{} {} = {};\n", ct, lo_t, lo_s));
                        self.out.push_str(&format!("{} {} = {};\n", ct, hi_t, hi_s));
                        self.out.push_str(&format!(
                            "for ({} {} = {}; {} {} {}; ++{}) {{\n",
                            ct, cvar, lo_t, cvar, cmp, hi_t, cvar
                        ));
                        self.loop_stack.push(LoopInfo {
                            body_depth: self.scopes.len(),
                            tmp_depth: self.tmp_frames.len(),
                            value: None,
                        });
                        self.emit_block(body, ret_ty, false)?;
                        self.loop_stack.pop();
                        self.out.push_str("}\n");
                        self.out.push_str("}\n");
                        Ok(())
                    }
                    ForIter::Each { vec } => {
                        // Snapshot-clone iteration: the vec operand is cloned
                        // up front (documented O(n) cost); the body can freely
                        // mutate the original. Loop var is an owned binding
                        // freed every iteration (registered in the body scope).
                        let vt = self.ty_of(vec.id);
                        let (elem_ct, elem_ty) = match &vt {
                            Ty::Generic { args, .. } => {
                                let et = args.first().cloned().unwrap_or(Ty::Unit);
                                (self.cty(&et), et)
                            }
                            _ => return Err(Ice("for-vec over non-Vec".into())),
                        };
                        self.push_frame();
                        let saved = self.cur_move;
                        self.cur_move = false;
                        let vsrc = self.emit_expr(vec)?;
                        self.cur_move = saved;
                        self.flush_nulls();
                        // Snapshot as a scope-owned binding (deep clone): it
                        // outlives iterations and is freed by the normal scope
                        // machinery on all exits (including break/return).
                        let snap = self.fresh_tmp();
                        let snap_ct = self.cty(&vt);
                        self.out.push_str(&format!(
                            "{} {} = {};\n",
                            snap_ct,
                            snap,
                            self.clone_call(&vt, &vsrc)
                        ));
                        self.declare(snap.clone(), vt.clone());
                        self.owned.insert(snap.clone(), vt.clone());
                        self.pop_frame();
                        let idx = self.fresh_tmp();
                        let cvar = mangle(var);
                        self.out.push_str("{\n");
                        self.out.push_str(&format!("int64_t {} = 0;\n", idx));
                        self.out
                            .push_str(&format!("int64_t {}_len = {}.len;\n", idx, snap));
                        self.out
                            .push_str(&format!("for (; {} < {}_len; ++{}) {{\n", idx, idx, idx));
                        // Per-iteration owned binding (clone out of snapshot;
                        // `Copy` elements copy plainly — no helper exists).
                        let elem_init = if self.type_needs_drop(&elem_ty) {
                            self.clone_call(&elem_ty, &format!("{}.data[{}]", snap, idx))
                        } else {
                            format!("{}.data[{}]", snap, idx)
                        };
                        self.out
                            .push_str(&format!("{} {} = {};\n", elem_ct, cvar, elem_init));
                        self.loop_stack.push(LoopInfo {
                            body_depth: self.scopes.len(),
                            tmp_depth: self.tmp_frames.len(),
                            value: None,
                        });
                        // Register the loop var so break/continue/return free it.
                        self.scopes.push(vec![]);
                        if self.moves(&elem_ty) {
                            self.scopes
                                .last_mut()
                                .unwrap()
                                .push((cvar.clone(), elem_ty.clone()));
                            self.owned.insert(cvar.clone(), elem_ty.clone());
                        }
                        self.emit_block_body_only(body, ret_ty, false)?;
                        self.emit_scope_frees(self.scopes.len() - 1);
                        self.scopes.pop();
                        self.loop_stack.pop();
                        self.out.push_str("}\n");
                        self.out.push_str("}\n");
                        Ok(())
                    }
                }
            }
            Stmt::Loop { body, .. } => {
                self.out.push_str("while (1) {\n");
                self.loop_stack.push(LoopInfo {
                    body_depth: self.scopes.len(),
                    tmp_depth: self.tmp_frames.len(),
                    value: None,
                });
                self.emit_block(body, ret_ty, false)?;
                self.loop_stack.pop();
                self.out.push_str("}\n");
                Ok(())
            }
            Stmt::Match {
                scrutinee, arms, ..
            } => {
                self.push_frame();
                self.emit_match(scrutinee, arms, None, false, ret_ty)?;
                self.flush_nulls();
                self.pop_frame();
                Ok(())
            }
            Stmt::Break { value, .. } => {
                let info = self
                    .loop_stack
                    .last()
                    .ok_or_else(|| Ice("break outside loop".into()))?;
                let depth = info.body_depth;
                let target = info
                    .value
                    .as_ref()
                    .map(|v| (v.res.clone(), v.label.clone()));
                match (value, target) {
                    (None, _) => {
                        // Plain break: abandon statement temps, run intervening
                        // Drops, then leave the loop.
                        let tmp_depth = self.loop_stack.last().map(|i| i.tmp_depth).unwrap_or(0);
                        while self.tmp_frames.len() > tmp_depth {
                            self.pop_frame();
                        }
                        self.emit_scope_frees_from(depth);
                        self.out.push_str("break;\n");
                    }
                    (Some(_), None) => {
                        // Sema rejects `break v` in statement loops; unreachable.
                        return Err(Ice("break value in statement loop".into()));
                    }
                    (Some(v), Some((res, label))) => {
                        self.push_frame();
                        let saved = self.cur_move;
                        self.cur_move = true;
                        let w = self.mat_str(v)?;
                        self.out.push_str(&format!("{} = {};\n", res, w));
                        self.consume(&w);
                        self.cur_move = saved;
                        self.flush_nulls();
                        self.pop_frame();
                        self.emit_scope_frees_from(depth);
                        self.out.push_str(&format!("goto {};\n", label));
                    }
                }
                Ok(())
            }
            Stmt::Continue { .. } => {
                let info = self
                    .loop_stack
                    .last()
                    .ok_or_else(|| Ice("continue outside loop".into()))?;
                let (depth, tmp_depth) = (info.body_depth, info.tmp_depth);
                while self.tmp_frames.len() > tmp_depth {
                    self.pop_frame();
                }
                self.emit_scope_frees_from(depth);
                self.out.push_str("continue;\n");
                Ok(())
            }
            Stmt::Return { value, .. } => {
                // Write queued move-poisons FIRST so the destroys below
                // skip transferred slots; then free every live statement
                // temp on the return path, keeping frames registered (see
                // `destroy_all_temps`): enclosing owners still pop (and
                // destroy) on fallthrough paths.
                self.flush_nulls();
                self.destroy_all_temps();
                self.push_frame();
                let saved = self.cur_move;
                match value {
                    None => {
                        self.flush_nulls();
                        self.pop_frame();
                        self.emit_all_frees();
                        self.out.push_str("return;\n");
                    }
                    Some(v) => {
                        let vt = self.ty_of(v.id);
                        if self.moves(&vt) {
                            self.cur_move = true;
                            let w = self.mat_str(v)?;
                            let ret = self.fresh_tmp();
                            self.out
                                .push_str(&format!("{} {} = {};\n", self.cty_val(&vt), ret, w));
                            self.consume(&w);
                            self.flush_nulls();
                            self.pop_frame();
                            self.emit_all_frees();
                            self.out.push_str(&format!("return {};\n", ret));
                        } else if vt == Ty::Unit {
                            self.cur_move = false;
                            let e = self.emit_expr(v)?;
                            self.out.push_str(&format!("{};\n", e));
                            self.flush_nulls();
                            self.pop_frame();
                            self.emit_all_frees();
                            self.out.push_str("return;\n");
                        } else {
                            self.cur_move = false;
                            let e = self.emit_expr(v)?;
                            // Materialize before frees: the expression may
                            // read owned locals (index, field, call), which
                            // frees would destroy first (use-after-free).
                            let ret = self.fresh_tmp();
                            self.out
                                .push_str(&format!("{} {} = {};\n", self.cty_val(&vt), ret, e));
                            self.flush_nulls();
                            self.pop_frame();
                            self.emit_all_frees();
                            self.out.push_str(&format!("return {};\n", ret));
                        }
                    }
                }
                self.cur_move = saved;
                Ok(())
            }
        }
    }

    // ----- expressions -----

    /// Emit an expression inline. String rvalues that need ownership tracking
    /// must go through `mat_str`; this returns the raw C expression.
    fn emit_expr(&mut self, e: &Expr) -> Result<String, Ice> {
        Ok(match &e.kind {
            ExprKind::Int(v) => match self.ty_of(e.id) {
                Ty::Int64 => format!("{}LL", v),
                _ => format!("(int32_t){}", v),
            },
            ExprKind::Float(v) => {
                let mut s = format!("{:?}", v);
                if !s.contains('.') && !s.contains('e') {
                    s.push_str(".0");
                }
                s
            }
            ExprKind::Bool(b) => (if *b { "1" } else { "0" }).to_string(),
            ExprKind::Str(s) => format!("vstr_from_c(\"{}\")", c_escape(s)),
            ExprKind::Var(name) => {
                let cname = mangle(name);
                // Bare `None`/`Missing` (unshadowed — sema guarantees) is a
                // constructor, not a variable reference.
                if (name == "None" || name == "Missing") && !self.owned.contains_key(&cname) {
                    if let Ty::Generic { base, args } = self.ty_of(e.id) {
                        if base == "Option" || base == "Missing" {
                            return Ok(format!("({}){{ .tag = 1 }}", ctype_inst(&base, &args)));
                        }
                    }
                }
                if self.cur_move {
                    self.null_slot(&cname);
                }
                cname
            }
            ExprKind::AddrOf { expr, .. } => {
                // Address of a place lvalue (sema: variable-rooted only).
                match expr_place(expr) {
                    Some(place) => format!("&({})", place_lvalue(&place, self)),
                    None => return Err(Ice("address of non-place".into())),
                }
            }
            ExprKind::Field { base, field, .. } => {
                // Lvalue path; the base must be a plain variable in 0.1.0
                // (field-of-temporary has no stable slot to poison).
                // Field access through a reference uses `->` (sema auto-derefs).
                let arrow = matches!(self.ty_of(base.id), Ty::Reference { .. });
                let saved = self.cur_move;
                self.cur_move = false;
                let b = self.emit_expr(base)?;
                self.cur_move = saved;
                let path = if arrow {
                    format!("{}->{}", b, cfield(field))
                } else {
                    format!("{}.{}", b, cfield(field))
                };
                if saved {
                    // Poison exactly this field path (never the whole base:
                    // sibling fields stay live).
                    let fty = self.field_ty_of(base, field);
                    if let Some(ft) = fty {
                        if self.moves(&ft) {
                            self.nulls.push((path.clone(), ft));
                        }
                    }
                }
                path
            }
            ExprKind::Index { base, idx } => {
                // Element copy out (owned elements clone, exactly like
                // `get`: the buffer is never aliased). Out-of-bounds is a
                // defined trap, mirroring integer division by zero.
                let et = match self.ty_of(base.id) {
                    Ty::Generic { base, args } if base == "Vec" && args.len() == 1 => {
                        args[0].clone()
                    }
                    _ => return Err(Ice("index on non-Vec".into())),
                };
                let saved = self.cur_move;
                self.cur_move = false;
                let b = self.mat_str(base)?;
                let iw = self.emit_expr(idx)?;
                self.cur_move = saved;
                let i64_ = format!("(int64_t)({})", iw);
                let slot = format!("{}.data[{}]", b, i64_);
                let val = if self.type_needs_drop(&et) {
                    self.clone_call(&et, &slot)
                } else {
                    slot
                };
                let zero = format!("({}){{0}}", self.cty(&et));
                format!(
                    "({i} < 0 || {i} >= {b}.len ? (fprintf(stderr, \"index out of bounds\\n\"), abort(), {zero}) : ({val}))",
                    i = i64_,
                    b = b,
                    zero = zero,
                    val = val
                )
            }
            ExprKind::Call { name, args } => {
                if name == "println" || name == "print" {
                    let a = args.first().ok_or_else(|| Ice("print arity".into()))?;
                    let t = self.ty_of(a.id);
                    let saved = self.cur_move;
                    self.cur_move = false; // print borrows
                                           // Materialize so owned rvalues (literals, concats, call results)
                                           // become frame temps freed at statement end.
                    let inner = if self.moves(&t) {
                        self.mat_str(a)?
                    } else {
                        self.emit_expr(a)?
                    };
                    self.cur_move = saved;
                    match t {
                        Ty::Int32 => format!("vprint_i32({})", inner),
                        Ty::Int64 => format!("vprint_i64({})", inner),
                        Ty::Float64 => format!("vprint_f64({})", inner),
                        Ty::Bool => format!("vprint_bool({})", inner),
                        // By value (borrow copy; callee never frees).
                        Ty::Str => format!("vprint_str({})", inner),
                        // Composites (sema validated printability + recorded
                        // the instantiation for helper generation).
                        Ty::Generic { base, args }
                            if (base == "Vec" || base == "Option" || base == "Missing")
                                && args.len() == 1 =>
                        {
                            format!("vprint_{}({})", iname_of(&ctype_inst(&base, &args)), inner)
                        }
                        Ty::Generic { base, args } if base == "Result" && args.len() == 2 => {
                            format!("vprint_{}({})", iname_of(&ctype_inst(&base, &args)), inner)
                        }
                        // Anything else is rejected by sema (structs, enums,
                        // references, Unit); reaching here is a compiler bug.
                        _ => return Err(Ice("print of non-printable type".into())),
                    }
                } else if name == "to_string" {
                    // J1 total scalar conversions (sema checked the type).
                    let a = args.first().ok_or_else(|| Ice("to_string arity".into()))?;
                    let t = self.ty_of(a.id);
                    let saved = self.cur_move;
                    self.cur_move = false;
                    let inner = self.emit_expr(a)?;
                    self.cur_move = saved;
                    match t {
                        Ty::Int32 => format!("vstr_from_i32({})", inner),
                        Ty::Int64 => format!("vstr_from_i64({})", inner),
                        Ty::Float64 => format!("vstr_from_f64({})", inner),
                        Ty::Bool => format!("vstr_from_bool({})", inner),
                        _ => return Err(Ice("to_string of non-scalar".into())),
                    }
                } else if name == "read_file"
                    || name == "csv_rows"
                    || name == "csv_bad_rows"
                    || name == "json_rows"
                {
                    // J2 file/CSV/JSON kernels (sema checked arity/types).
                    // Arguments borrow; owned rvalues materialize into
                    // statement-lived temps like `println`.
                    let mut parts = vec![];
                    for a in args {
                        let saved = self.cur_move;
                        self.cur_move = false;
                        let t = self.ty_of(a.id);
                        let v = if self.moves(&t) {
                            self.mat_str(a)?
                        } else {
                            self.emit_expr(a)?
                        };
                        self.cur_move = saved;
                        parts.push(v);
                    }
                    match name.as_str() {
                        "read_file" => format!("v_read_file_wrap({})", parts.join(", ")),
                        "csv_rows" => format!("v_csv_rows_wrap({})", parts.join(", ")),
                        "csv_bad_rows" => format!("v_csv_bad_rows_wrap({})", parts.join(", ")),
                        _ => format!("v_json_rows_wrap({})", parts.join(", ")),
                    }
                } else if name == "Some" || name == "Ok" || name == "Err" {
                    // Option/Result constructors (sema typed the result).
                    let rt = self.ty_of(e.id);
                    let variant = name;
                    let (ct, idx, _) = self.resolve_variant(&None, variant, &rt)?;
                    // Payloads move like call args (temps poisoned, statement-level flush).
                    let saved = self.cur_move;
                    let mut parts = vec![];
                    let mut moved_tmps = vec![];
                    for a in args {
                        let t = self.ty_of(a.id);
                        self.cur_move = self.moves(&t);
                        let before = self.frame_len();
                        let v = self.mat_str(a)?;
                        if self.cur_move && self.is_tmp_since(before, &v) {
                            moved_tmps.push(v.clone());
                        }
                        parts.push(v);
                    }
                    self.cur_move = saved;
                    for t in moved_tmps {
                        self.null_slot(&t);
                    }
                    let mut inits = vec![];
                    for (j, p) in parts.iter().enumerate() {
                        inits.push(format!(".{} = {}", cfield(&format!("_{}", j)), p));
                    }
                    if inits.is_empty() {
                        // Unreachable for ctors (all take exactly 1 arg).
                        format!("({}){{ .tag = {} }}", ct, idx)
                    } else {
                        format!(
                            "({}){{ .tag = {}, .u = {{ .{} = {{ {} }} }} }}",
                            ct,
                            idx,
                            cmember(idx),
                            inits.join(", ")
                        )
                    }
                } else if let Some(q) = self.resolve_type(name) {
                    let def = self
                        .structs
                        .get(&q)
                        .cloned()
                        .ok_or_else(|| Ice(format!("unknown struct {}", name)))?;
                    // Tuple-struct construction `Meters(1.0)` (sema guarantees
                    // tuple form here; named structs via literal are E0201).
                    if !def.is_tuple {
                        return Err(Ice("named struct via call syntax".into()));
                    }
                    let saved = self.cur_move;
                    let mut parts = vec![];
                    let mut moved_tmps = vec![];
                    // Shield: same hazard as calls/struct literals (an
                    // argument flushing internally must not write earlier
                    // arguments' queued poisons before assembly).
                    let mut shielded: Vec<(String, Ty)> = vec![];
                    for (j, a) in args.iter().enumerate() {
                        shielded.extend(std::mem::take(&mut self.nulls));
                        let t = self.ty_of(a.id);
                        self.cur_move = self.moves(&t);
                        let before = self.frame_len();
                        let v = self.mat_str(a)?;
                        if self.cur_move && self.is_tmp_since(before, &v) {
                            moved_tmps.push(v.clone());
                        }
                        parts.push(format!(".{} = {}", cfield(&format!("_{}", j)), v));
                    }
                    self.cur_move = saved;
                    for t in moved_tmps {
                        self.null_slot(&t);
                    }
                    self.nulls.extend(shielded);
                    format!("({}){{ {} }}", ctype_name(&q), parts.join(", "))
                } else {
                    // Arguments: move owned values into the callee (callee frees params).
                    let saved = self.cur_move;
                    let mut parts = vec![];
                    let mut moved_tmps = vec![];
                    // Shield (as in struct literals): an argument that
                    // flushes internally (e.g. a nested `vec![..]`) must
                    // not write poisons queued by earlier arguments — the
                    // call text has not been emitted yet.
                    let mut shielded: Vec<(String, Ty)> = vec![];
                    for a in args {
                        shielded.extend(std::mem::take(&mut self.nulls));
                        let t = self.ty_of(a.id);
                        self.cur_move = self.moves(&t);
                        let before = self.frame_len();
                        let v = self.mat_str(a)?;
                        if self.cur_move && self.is_tmp_since(before, &v) {
                            moved_tmps.push(v.clone());
                        }
                        parts.push(v);
                    }
                    self.cur_move = saved;
                    let (cmod, cname) = self
                        .resolve_fun(name)
                        .ok_or_else(|| Ice(format!("unknown function {}", name)))?;
                    let call = format!("{}_fn({})", cfun(&cmod, &cname), parts.join(", "));
                    for t in moved_tmps {
                        self.null_slot(&t);
                    }
                    self.nulls.extend(shielded);
                    // NOTE: no flush here — nulls are statement-level effects and must
                    // be written after the enclosing statement text (else use-after-NULL).
                    call
                }
            }
            ExprKind::Binary { op, lhs, rhs } => {
                let lt = self.ty_of(lhs.id);
                let saved = self.cur_move;
                self.cur_move = false; // operators borrow both sides
                let out = if lt == Ty::Str {
                    // Materialize operands: literal/call rvalues become frame temps
                    // (freed at statement end) instead of leaking inline.
                    let l = self.mat_str(lhs)?;
                    let r = self.mat_str(rhs)?;
                    self.cur_move = saved;
                    match op {
                        BinOp::Add => format!("vstr_concat({}, {})", l, r),
                        BinOp::Eq => format!("vstr_eq({}, {})", l, r),
                        BinOp::Ne => format!("(!vstr_eq({}, {}))", l, r),
                        _ => return Err(Ice("invalid string operator".into())),
                    }
                } else if matches!(op, BinOp::Eq | BinOp::Ne)
                    && matches!(&lt, Ty::Generic { base, args } if base == "Missing" && args.len() == 1)
                {
                    // `Missing == Missing` is true (unlike NaN); two
                    // `Present` values compare payloads with `==`, so NaN
                    // payloads stay unequal. Tags decide first.
                    self.cur_move = false;
                    let l = self.mat_str(lhs)?;
                    let r = self.mat_str(rhs)?;
                    self.cur_move = saved;
                    let t = match &lt {
                        Ty::Generic { args, .. } => args[0].clone(),
                        _ => return Err(Ice("missing payload".into())),
                    };
                    let lp = format!("{}.u.v0.v___0", l);
                    let rp = format!("{}.u.v0.v___0", r);
                    let cmp = match t {
                        Ty::Str => format!("vstr_eq({}, {})", lp, rp),
                        _ => format!("({} == {})", lp, rp),
                    };
                    let eq = format!(
                        "({}.tag != {}.tag ? 0 : (({}.tag == 1) ? 1 : ({})))",
                        l, r, l, cmp
                    );
                    if matches!(op, BinOp::Ne) {
                        format!("(!({}))", eq)
                    } else {
                        eq
                    }
                } else {
                    let l = self.emit_expr(lhs)?;
                    let r = self.emit_expr(rhs)?;
                    self.cur_move = saved;
                    let sop = match op {
                        BinOp::Add => "+",
                        BinOp::Sub => "-",
                        BinOp::Mul => "*",
                        BinOp::Div => "/",
                        BinOp::Rem => "%",
                        BinOp::Eq => "==",
                        BinOp::Ne => "!=",
                        BinOp::Lt => "<",
                        BinOp::Le => "<=",
                        BinOp::Gt => ">",
                        BinOp::Ge => ">=",
                        BinOp::And => "&&",
                        BinOp::Or => "||",
                    };
                    if matches!(op, BinOp::Div | BinOp::Rem) && matches!(lt, Ty::Int32 | Ty::Int64)
                    {
                        format!(
                            "(({r}) == 0 ? (fprintf(stderr, \"integer division by zero\\n\"), abort(), 0) : (({l}) {op} ({r})))",
                            r = r,
                            l = l,
                            op = sop
                        )
                    } else {
                        format!("(({}) {} ({}))", l, sop, r)
                    }
                };
                out
            }
            ExprKind::Unary { op, expr } => {
                let saved = self.cur_move;
                self.cur_move = false;
                let inner = self.emit_expr(expr)?;
                self.cur_move = saved;
                match op {
                    // Dereferencing an owned value in move position deep-clones
                    // (never moves out of borrowed content — no buffer aliases,
                    // same rule as for-vec snapshots and get()). Copy results
                    // and borrow-position reads are plain aliases.
                    UnOp::Deref => {
                        let rt = self.ty_of(e.id);
                        if saved && self.moves(&rt) {
                            self.clone_call(&rt, &format!("(*{})", inner))
                        } else {
                            format!("(*{})", inner)
                        }
                    }
                    UnOp::Neg => format!("(-({}))", inner),
                    UnOp::Not => format!("(!({}))", inner),
                }
            }
            ExprKind::Assign { .. } => return Err(Ice("assign in value position".into())),
            ExprKind::If {
                cond,
                then_b,
                else_b,
            } => {
                let tt = self.ty_of(e.id);
                let saved = self.cur_move;
                self.cur_move = false;
                let c = self.emit_expr(cond)?;
                let mv = saved; // arms inherit the if's move context
                if self.moves(&tt) {
                    let res = self
                        .new_tmp_ty(&self.cty_val(&tt), &format!("({}){{0}}", self.cty_val(&tt)));
                    self.out.push_str(&format!("if ({}) {{\n", c));
                    self.emit_arm(then_b, &res, &tt, mv)?;
                    self.out.push_str("} else {\n");
                    match else_b {
                        Some(b) => self.emit_arm(b, &res, &tt, mv)?,
                        None => self.out.push_str("{}"),
                    }
                    self.out.push_str("}\n");
                    self.cur_move = saved;
                    res
                } else if tt == Ty::Unit {
                    // Statement-position `if`: no result slot (a `void`
                    // tail has no value to store). Emit both branches as
                    // plain blocks and yield a dummy inline value.
                    self.out.push_str(&format!("if ({}) {{\n", c));
                    let fn_ret = self.fn_ret.clone();
                    self.emit_block(then_b, &fn_ret, false)?;
                    self.out.push('}');
                    if let Some(b) = else_b {
                        self.out.push_str(" else {\n");
                        self.emit_block(b, &fn_ret, false)?;
                        self.out.push('}');
                    }
                    self.out.push('\n');
                    self.cur_move = saved;
                    "0".to_string()
                } else {
                    // Non-string value-if: lower via a temp variable,
                    // registered so pops/transfers track it.
                    let res = self.fresh_tmp();
                    let res_ct = self.cty_val(&tt);
                    self.out.push_str(&format!("{} {};\n", res_ct, res));
                    if let Some(f) = self.tmp_frames.last_mut() {
                        f.push((res.clone(), res_ct));
                    }
                    self.out.push_str(&format!("if ({}) {{\n", c));
                    self.emit_arm_copy(then_b, &res)?;
                    self.out.push_str("} else {\n");
                    match else_b {
                        Some(b) => self.emit_arm_copy(b, &res)?,
                        None => self.out.push_str("{}"),
                    }
                    self.out.push_str("}\n");
                    self.cur_move = saved;
                    res
                }
            }
            ExprKind::Match { scrutinee, arms } => {
                let tt = self.ty_of(e.id);
                let mv = self.cur_move;
                // Unit-typed matches (statement tails) take no result slot:
                // a `void` tail has no value to store. Emit arms as plain
                // blocks, exactly like a statement match.
                let res_ty = if tt == Ty::Unit { None } else { Some(&tt) };
                // A borrowed owned result is cloned into res by the arm
                // logic; the res temp itself lives in our frame and is
                // freed/transferred by the caller, exactly like value-`if`.
                self.emit_match(scrutinee, arms, res_ty, mv, &self.fn_ret.clone())?
            }
            ExprKind::Loop { body } => {
                let tt = self.ty_of(e.id);
                // Statement-level lowering emitted inline (same hoisting pattern
                // as value-`if`): callers use the returned name after it.
                // C labels live in their own namespace, so temps are fine.
                let value = if tt == Ty::Unit {
                    None
                } else {
                    let res = self.fresh_tmp();
                    let res_ct = self.cty_val(&tt);
                    self.out.push_str(&format!("{} {};\n", res_ct, res));
                    if let Some(f) = self.tmp_frames.last_mut() {
                        f.push((res.clone(), res_ct));
                    }
                    Some((res, self.fresh_tmp()))
                };
                let label = value.as_ref().map(|(_, l)| l.clone());
                self.out.push_str("while (1) {\n");
                let target = value.as_ref().map(|(res, label)| LoopValue {
                    res: res.clone(),
                    label: label.clone(),
                });
                self.loop_stack.push(LoopInfo {
                    body_depth: self.scopes.len(),
                    tmp_depth: self.tmp_frames.len(),
                    value: target,
                });
                let fn_ret = self.fn_ret.clone();
                self.emit_block(body, &fn_ret, false)?;
                self.loop_stack.pop();
                self.out.push_str("}\n");
                match (value, label) {
                    (Some((res, _)), Some(lab)) => {
                        self.out.push_str(&format!("{}:;\n", lab));
                        res
                    }
                    _ => "0".to_string(),
                }
            }
            ExprKind::StructLit { name, fields, .. } => {
                // Compound literal; field values inherit the ambient move
                // context (construction consumes owned fields).
                let q = self
                    .resolve_type(name)
                    .ok_or_else(|| Ice(format!("unknown struct {}", name)))?;
                let def = self
                    .structs
                    .get(&q)
                    .cloned()
                    .ok_or_else(|| Ice(format!("unknown struct {}", name)))?;
                if def.is_tuple {
                    return Err(Ice("tuple struct via literal syntax".into()));
                }
                let saved = self.cur_move;
                let mut parts = vec![];
                let mut moved_tmps = vec![];
                // Shield: poisons queued by earlier fields must not be
                // written by flushes inside later fields (e.g. a nested
                // `vec![..]` flushes after each push) — the literal has
                // not been assembled yet, so early writes would blank live
                // sources. Each field sees only its own poisons; earlier
                // ones re-queue after the literal text is built.
                let mut shielded: Vec<(String, Ty)> = vec![];
                for (fname, _, fexpr) in fields {
                    shielded.extend(std::mem::take(&mut self.nulls));
                    let t = self.ty_of(fexpr.id);
                    self.cur_move = self.moves(&t);
                    let before = self.frame_len();
                    let v = self.mat_str(fexpr)?;
                    if self.cur_move && self.is_tmp_since(before, &v) {
                        moved_tmps.push(v.clone());
                    }
                    parts.push(format!(".{} = {}", cfield(fname), v));
                }
                self.cur_move = saved;
                for t in moved_tmps {
                    self.null_slot(&t);
                }
                self.nulls.extend(shielded);
                // Missing fields are rejected by sema; fill nothing here.
                // Unit structs: empty initializer list.
                format!("({}){{ {} }}", ctype_name(&q), parts.join(", "))
            }
            ExprKind::OptField { base, field, .. } => {
                // `base?.field`: None short-circuits; else a fresh Some holding
                // a *clone* of the field (borrow context — never moves base).
                let rt = self.ty_of(e.id);
                let saved = self.cur_move;
                self.cur_move = false;
                // Single evaluation: side-effecting bases must not repeat.
                let b = match &base.kind {
                    ExprKind::Var(name) => mangle(name),
                    _ => {
                        let inline = self.emit_expr(base)?;
                        let bt = self.ty_of(base.id);
                        self.new_tmp_ty(&self.cty_val(&bt), &inline)
                    }
                };
                self.cur_move = saved;
                // Resolve payload struct + field for paths and types.
                let fty = match self.ty_of(base.id) {
                    Ty::Generic { args, .. } => match args.first() {
                        Some(Ty::Struct(n)) => self
                            .structs
                            .get(n)
                            .and_then(|d| d.fields.iter().find(|(f, _)| f == field))
                            .map(|(_, t)| t.clone()),
                        _ => None,
                    },
                    _ => None,
                };
                let fty = fty.ok_or_else(|| Ice("?. payload has no such field".into()))?;
                // Payload sits one wrapper level down: member holds `_0: P`.
                let fpath = format!("{}.u.{}.{}.{}", b, cmember(0), cfield("_0"), cfield(field));
                let fval = if self.moves(&fty) {
                    // Deep copy into the fresh result (generic payloads too).
                    if self.type_needs_drop(&fty) {
                        self.clone_call(&fty, &fpath)
                    } else {
                        fpath
                    }
                } else {
                    fpath
                };
                let rct = self.cty(&rt);
                format!(
                    "(({}.tag == 0) ? ({}){{ .tag = 0, .u = {{ .{} = {{ .{} = {} }} }} }} : ({}){{ .tag = 1 }})",
                    b,
                    rct,
                    cmember(0),
                    cfield("_0"),
                    fval,
                    rct
                )
            }
            ExprKind::Variant {
                ty, variant, args, ..
            } => {
                // Qualified declared enums and builtin
                // `Option::`/`Result::`/`Missing::` both resolve here (sema
                // validated the spelling). User-written enum names resolve
                // through the current module + imports.
                let scrut = self.ty_of(e.id);
                let qty = if ty == "Option" || ty == "Result" || ty == "Missing" {
                    ty.clone()
                } else {
                    self.resolve_type(ty)
                        .ok_or_else(|| Ice(format!("unknown enum {}", ty)))?
                };
                let (_, idx, _) = self.resolve_variant(&Some(qty), variant, &scrut)?;
                // Payloads move owned values into the construction (like call
                // args): materialize + poison temps, flush at statement level.
                let saved = self.cur_move;
                let mut parts = vec![];
                let mut moved_tmps = vec![];
                for a in args {
                    let t = self.ty_of(a.id);
                    self.cur_move = self.moves(&t);
                    let before = self.frame_len();
                    let v = self.mat_str(a)?;
                    if self.cur_move && self.is_tmp_since(before, &v) {
                        moved_tmps.push(v.clone());
                    }
                    parts.push(v);
                }
                self.cur_move = saved;
                for t in moved_tmps {
                    self.null_slot(&t);
                }
                let mut inits = vec![];
                for (j, p) in parts.iter().enumerate() {
                    inits.push(format!(".{} = {}", cfield(&format!("_{}", j)), p));
                }
                // C type comes from the resolved result type (instantiations
                // like `VOption_Int32`, not the bare `Option` qualifier).
                let rct2 = self.cty(&scrut);
                if inits.is_empty() {
                    format!("({}){{ .tag = {} }}", rct2, idx)
                } else {
                    format!(
                        "({}){{ .tag = {}, .u = {{ .{} = {{ {} }} }} }}",
                        rct2,
                        idx,
                        cmember(idx),
                        inits.join(", ")
                    )
                }
            }
            ExprKind::Try { expr: operand, .. } => {
                // T-Prop: test the discriminant; on failure move the payload
                // into an early return of the enclosing function type.
                let opty = self.ty_of(operand.id);
                let fret = self.fn_ret.clone();
                let (succ_ty, is_res) = match (&opty, &fret) {
                    (
                        Ty::Generic {
                            base: ob,
                            args: oargs,
                        },
                        Ty::Generic { base: fb, .. },
                    ) if ob == "Result" && fb == "Result" && oargs.len() == 2 => {
                        (oargs[0].clone(), true)
                    }
                    (
                        Ty::Generic {
                            base: ob,
                            args: oargs,
                        },
                        Ty::Generic { base: fb, .. },
                    ) if ob == "Option" && fb == "Option" && oargs.len() == 1 => {
                        (oargs[0].clone(), false)
                    }
                    _ => return Err(Ice("`?` shape rejected by sema".into())),
                };
                let saved = self.cur_move;
                self.cur_move = false;
                let slot = if self.moves(&opty) {
                    self.mat_str(operand)?
                } else {
                    self.emit_expr(operand)?
                };
                self.cur_move = saved;
                self.flush_nulls();
                // Success value temp (caller transfers or frees it).
                let succ = self.new_tmp_ty(
                    &self.cty_val(&succ_ty),
                    &format!("({}){{0}}", self.cty_val(&succ_ty)),
                );
                // Payload paths (Some/Ok live in variant 0; Err in variant 1).
                let ok_path = format!("{}.u.{}.{}", slot, cmember(0), cfield("_0"));
                self.out.push_str(&format!("if ({}.tag == 1) {{\n", slot));
                if is_res {
                    // Move the error payload out, abandon everything, return it.
                    let ety = match &opty {
                        Ty::Generic { args, .. } => args[1].clone(),
                        _ => Ty::Unit,
                    };
                    let ephemeral = self.fresh_tmp();
                    self.out.push_str(&format!(
                        "{} {} = {};\n",
                        self.cty_val(&ety),
                        ephemeral,
                        ok_path.replace(".u.v0.", ".u.v1.")
                    ));
                    self.nulls
                        .push((ok_path.replace(".u.v0.", ".u.v1."), ety.clone()));
                    self.flush_nulls();
                    while !self.tmp_frames.is_empty() {
                        self.pop_frame();
                    }
                    self.emit_all_frees();
                    let frct = self.cty(&fret);
                    let emember = cmember(1);
                    let efield = cfield("_0");
                    self.out.push_str(&format!(
                        "return ({}){{ .tag = 1, .u = {{ .{} = {{ .{} = {} }} }} }};\n",
                        frct, emember, efield, ephemeral
                    ));
                } else {
                    // Option: no payload to move; return None of fn type.
                    while !self.tmp_frames.is_empty() {
                        self.pop_frame();
                    }
                    self.emit_all_frees();
                    let frct = self.cty(&fret);
                    self.out
                        .push_str(&format!("return ({}){{ .tag = 1 }};\n", frct));
                }
                self.out.push_str("}\n");
                // Success: move payload into the success temp.
                self.out.push_str(&format!("{} = {};\n", succ, ok_path));
                self.nulls.push((ok_path, succ_ty));
                self.flush_nulls();
                succ
            }
            ExprKind::VecMacro { elems } => {
                // Build by repeated push into a frame temp (transfers out).
                let vt = self.ty_of(e.id);
                let (elem_ty, vct) = match &vt {
                    Ty::Generic { base, args } if base == "Vec" && args.len() == 1 => {
                        (args[0].clone(), ctype_inst(base, args))
                    }
                    _ => return Err(Ice("vec! with non-Vec type".into())),
                };
                let iname = vct.strip_prefix('V').unwrap_or(&vct).to_string();
                let tmp = self.new_tmp_ty(&vct, &format!("vvec_{}_new()", iname));
                for el in elems {
                    let saved = self.cur_move;
                    self.cur_move = self.moves(&elem_ty);
                    let v = self.mat_str(el)?;
                    self.cur_move = saved;
                    self.out
                        .push_str(&format!("vvec_{}_push(&{}, {});\n", iname, tmp, v));
                    self.consume(&v);
                }
                self.flush_nulls();
                tmp
            }
            ExprKind::Method {
                receiver,
                method,
                args,
                ..
            } => {
                let rt = self.ty_of(receiver.id);
                // J1 `String` methods (receiver borrows like `Vec`).
                if rt == Ty::Str {
                    let saved = self.cur_move;
                    self.cur_move = false;
                    let r = self.mat_str(receiver)?;
                    self.cur_move = saved;
                    return self.emit_str_method(method, &r, args);
                }
                let (elem_ty, iname) = match &rt {
                    Ty::Generic { base, args } if base == "Vec" && args.len() == 1 => {
                        let t = args[0].clone();
                        let vct = ctype_inst(base, args);
                        (t, vct.strip_prefix('V').unwrap_or(&vct).to_string())
                    }
                    _ => return Err(Ice("method on non-Vec".into())),
                };
                // Receiver borrows (pointer model — never moves).
                let saved = self.cur_move;
                self.cur_move = false;
                let r = self.mat_str(receiver)?;
                self.cur_move = saved;
                let rptr = format!("&{}", r);
                match method.as_str() {
                    "len" => format!("vvec_{}_len({})", iname, rptr),
                    "push" => {
                        // In-place push; the driver emits the call and flushes.
                        let a = args.first().ok_or_else(|| Ice("push arity".into()))?;
                        let sv = self.cur_move;
                        self.cur_move = self.moves(&elem_ty);
                        let v = self.mat_str(a)?;
                        self.cur_move = sv;
                        self.consume(&v);
                        format!("vvec_{}_push({}, {})", iname, rptr, v)
                    }
                    "pop" | "get" | "clone" | "cast" | "add_scalar" | "add" | "mul" | "dot" => {
                        let rt = self.ty_of(e.id);
                        self.emit_vec_method(method, &elem_ty, &iname, &r, args, &rt)?
                    }
                    _ => return Err(Ice("unknown Vec method".into())),
                }
            }
            ExprKind::Cast { expr, target, .. } => {
                let saved = self.cur_move;
                self.cur_move = false; // casts borrow
                let inner = self.emit_expr(expr)?;
                self.cur_move = saved;
                let st = self.ty_of(expr.id);
                cast_expr(&st, target, &inner)
            }
        })
    }

    fn frame_len(&self) -> usize {
        self.tmp_frames.last().map(|f| f.len()).unwrap_or(0)
    }

    fn is_tmp_since(&self, before: usize, name: &str) -> bool {
        self.tmp_frames
            .last()
            .map(|f| f.iter().skip(before).any(|(t, _)| t == name))
            .unwrap_or(false)
    }

    /// String-valued arm: tail assigns into `target`; ownership follows `mv`.
    /// Owned-valued arm: tail transfers into `target` of type `target_ty`.
    /// Moves NULL the source; borrows deep-clone (the arm scope dies).
    fn emit_arm(&mut self, b: &Block, target: &str, target_ty: &Ty, mv: bool) -> Result<(), Ice> {
        self.scopes.push(vec![]);
        let depth = self.scopes.len() - 1;
        self.push_frame();
        for s in &b.stmts {
            self.emit_stmt(s, &Ty::Unit)?;
        }
        if let Some(t) = &b.tail {
            let saved = self.cur_move;
            self.cur_move = mv;
            let v = self.mat_str(t)?;
            if mv {
                // Move into the result slot; poison the source.
                self.out.push_str(&format!("{} = {};\n", target, v));
                self.consume(&v);
            } else if *target_ty == Ty::Str {
                // Borrow position: the result slot needs its own copy.
                self.out
                    .push_str(&format!("{} = vstr_clone({});\n", target, v));
            } else if let Ty::Struct(n) | Ty::Enum(n) = target_ty {
                self.out
                    .push_str(&format!("{} = v_clone_{}({});\n", target, dname(n), v));
            } else if let Ty::Generic { base, args } = target_ty {
                let ct = ctype_inst(base, args);
                let iname = ct.strip_prefix('V').unwrap_or(&ct);
                self.out
                    .push_str(&format!("{} = v_clone_{}({});\n", target, iname, v));
            } else {
                return Err(Ice("owned arm with unexpected type".into()));
            }
            self.cur_move = saved;
            self.flush_nulls();
        }
        self.pop_frame();
        self.emit_scope_frees(depth);
        self.scopes.pop();
        Ok(())
    }

    // ----- match lowering (Phase C) -----

    /// Normalize arms: expand `|` alternatives into separate arms sharing the
    /// body AST (emitted once per expansion; documents code-size tradeoff).
    fn expand_arms(&self, arms: &[MatchArm]) -> Vec<MatchArm> {
        let mut out = vec![];
        for arm in arms {
            match &arm.pat {
                Pat::Or(alts) => {
                    for a in alts {
                        out.push(MatchArm {
                            pat: a.clone(),
                            guard: arm.guard.clone(),
                            body: arm.body.clone(),
                            span: arm.span,
                        });
                    }
                }
                _ => out.push(arm.clone()),
            }
        }
        out
    }

    /// Whether an unguarded pattern matches everything (catch-all position).
    fn pat_is_catch_all(pat: &Pat) -> bool {
        matches!(pat, Pat::Wild | Pat::Bind(_, _))
    }

    /// Pure test expression for a pattern (no bindings emitted).
    /// Test expression for a pattern at lvalue `slot` of type `ty`.
    /// Literals compare; variants check tags; structs conjoin sub-tests.
    /// Full nesting supported via path composition.
    fn pat_test(&mut self, pat: &Pat, ty: &Ty, slot: &str) -> Result<String, Ice> {
        match pat {
            Pat::Wild => Ok("1".to_string()),
            Pat::Bind(_, _) => Ok("1".to_string()),
            Pat::LitInt(v) => Ok(format!("({} == {})", slot, v)),
            Pat::LitBool(b) => Ok(format!("({} == {})", slot, if *b { "1" } else { "0" })),
            Pat::LitFloat(v) => {
                let mut lit = format!("{:?}", v);
                if !lit.contains('.') && !lit.contains('e') {
                    lit.push_str(".0");
                }
                Ok(format!("({} == {})", slot, lit))
            }
            Pat::LitStr(lit) => {
                let tmp = self.new_tmp_ty("VStr", &format!("vstr_from_c(\"{}\")", c_escape(lit)));
                Ok(format!("(vstr_eq({}, {}))", slot, tmp))
            }
            Pat::Variant {
                ty: q,
                variant,
                sub,
                ..
            } => {
                let rq = self.resolve_pat_ty(q)?;
                let (_, idx, pts) = self.resolve_variant(&rq, variant, ty)?;
                let mut tests = vec![format!("({}.tag == {})", slot, idx)];
                for (j, sp) in sub.iter().enumerate() {
                    let ppath =
                        format!("{}.u.{}.{}", slot, cmember(idx), cfield(&format!("_{}", j)));
                    let pty = pts.get(j).cloned().unwrap_or(Ty::Unit);
                    let t = self.pat_test(sp, &pty, &ppath)?;
                    if t != "1" {
                        tests.push(t);
                    }
                }
                if tests.len() == 1 {
                    Ok(tests.pop().unwrap())
                } else {
                    Ok(format!("({})", tests.join(" && ")))
                }
            }
            Pat::Struct { name, fields } => {
                let def = self
                    .structs
                    .get(name)
                    .cloned()
                    .ok_or_else(|| Ice(format!("unknown struct {}", name)))?;
                let mut tests = vec![];
                for (fname, sub) in fields {
                    if let Some(p) = sub {
                        let fty = def
                            .fields
                            .iter()
                            .find(|(f, _)| f == fname)
                            .map(|(_, t)| t.clone())
                            .ok_or_else(|| Ice("unknown field".into()))?;
                        let fpath = format!("{}.{}", slot, cfield(fname));
                        let t = self.pat_test(p, &fty, &fpath)?;
                        if t != "1" {
                            tests.push(t);
                        }
                    }
                }
                if tests.is_empty() {
                    Ok("1".to_string())
                } else {
                    Ok(format!("({})", tests.join(" && ")))
                }
            }
            Pat::Or(alts) => {
                let mut tests = vec![];
                for a in alts {
                    tests.push(self.pat_test(a, ty, slot)?);
                }
                Ok(format!("({})", tests.join(" || ")))
            }
        }
    }

    /// Binding declarations for a pattern at lvalue `slot` of type `ty`.
    /// Owned bindings move out (poisoned paths); Copy bindings plain-copy.
    fn pat_binds(&mut self, pat: &Pat, ty: &Ty, slot: &str) -> Result<(), Ice> {
        match pat {
            Pat::Wild => Ok(()),
            Pat::Bind(name, _) => {
                let cname = mangle(name);
                self.out
                    .push_str(&format!("{} {} = {};\n", self.cty_val(ty), cname, slot));
                self.declare(cname.clone(), ty.clone());
                if self.moves(ty) {
                    self.owned.insert(cname.clone(), ty.clone());
                    // Move-out: poison the source path.
                    self.nulls.push((slot.to_string(), ty.clone()));
                    self.flush_nulls();
                }
                Ok(())
            }
            Pat::LitInt(_) | Pat::LitFloat(_) | Pat::LitBool(_) | Pat::LitStr(_) => Ok(()),
            Pat::Variant {
                ty: q,
                variant,
                sub,
                ..
            } => {
                let rq = self.resolve_pat_ty(q)?;
                let (_, idx, pts) = self.resolve_variant(&rq, variant, ty)?;
                for (j, sp) in sub.iter().enumerate() {
                    let ppath =
                        format!("{}.u.{}.{}", slot, cmember(idx), cfield(&format!("_{}", j)));
                    let pty = pts.get(j).cloned().unwrap_or(Ty::Unit);
                    self.pat_binds(sp, &pty, &ppath)?;
                }
                Ok(())
            }
            Pat::Struct { name, fields } => {
                let def = self
                    .structs
                    .get(name)
                    .cloned()
                    .ok_or_else(|| Ice(format!("unknown struct {}", name)))?;
                for (fname, sub) in fields {
                    let fty = def
                        .fields
                        .iter()
                        .find(|(f, _)| f == fname)
                        .map(|(_, t)| t.clone())
                        .ok_or_else(|| Ice("unknown field".into()))?;
                    let fpath = format!("{}.{}", slot, cfield(fname));
                    match sub {
                        None => {
                            let cname = mangle(fname);
                            self.out.push_str(&format!(
                                "{} {} = {};\n",
                                self.cty_val(&fty),
                                cname,
                                fpath
                            ));
                            self.declare(cname.clone(), fty.clone());
                            if self.moves(&fty) {
                                self.owned.insert(cname.clone(), fty.clone());
                                self.nulls.push((fpath, fty));
                                self.flush_nulls();
                            }
                        }
                        Some(p) => self.pat_binds(p, &fty, &fpath)?,
                    }
                }
                Ok(())
            }
            // Or-patterns are expanded into separate arms before lowering;
            // a stray nested Or binds from its first alternative (sema
            // guarantees all alternatives agree).
            Pat::Or(alts) => match alts.first() {
                Some(a) => self.pat_binds(a, ty, slot),
                None => Ok(()),
            },
        }
    }

    /// Arm body for matches: statement arms emit bare; value arms assign res.
    fn emit_match_body(
        &mut self,
        b: &Block,
        res: Option<(&str, &Ty, bool)>,
        ret_ty: &Ty,
    ) -> Result<(), Ice> {
        for s in &b.stmts {
            self.emit_stmt(s, ret_ty)?;
        }
        if let Some(t) = &b.tail {
            match res {
                None => {
                    let saved = self.cur_move;
                    self.cur_move = false;
                    let e = self.emit_expr(t)?;
                    self.cur_move = saved;
                    self.out.push_str(&format!("{};\n", e));
                    self.flush_nulls();
                }
                Some((target, ty, mv)) => {
                    let saved = self.cur_move;
                    self.cur_move = mv;
                    if self.moves(ty) {
                        let v = self.mat_str(t)?;
                        if mv {
                            // Move into res; poison the source.
                            self.out.push_str(&format!("{} = {};\n", target, v));
                            self.consume(&v);
                        } else if *ty == Ty::Str {
                            // Borrow: res outlives the arm temps, so clone.
                            self.out
                                .push_str(&format!("{} = vstr_clone({});\n", target, v));
                        } else if let Ty::Struct(n) | Ty::Enum(n) = ty {
                            self.out.push_str(&format!(
                                "{} = v_clone_{}({});\n",
                                target,
                                dname(n),
                                v
                            ));
                        } else if let Ty::Generic { base, args } = ty {
                            let ct = ctype_inst(base, args);
                            let iname = ct.strip_prefix('V').unwrap_or(&ct);
                            self.out
                                .push_str(&format!("{} = v_clone_{}({});\n", target, iname, v));
                        } else {
                            return Err(Ice("owned arm with unexpected type".into()));
                        }
                    } else {
                        let v = self.emit_expr(t)?;
                        self.out.push_str(&format!("{} = {};\n", target, v));
                    }
                    self.cur_move = saved;
                    self.flush_nulls();
                }
            }
        }
        Ok(())
    }

    /// Emit a whole match. `res_ty`: value-match result type, if any.
    /// `mv`: whether arm tails move into res (mirrors ownership move_ctx).
    /// Returns the inline value name for value matches.
    fn emit_match(
        &mut self,
        scrutinee: &Expr,
        arms: &[MatchArm],
        res_ty: Option<&Ty>,
        mv: bool,
        ret_ty: &Ty,
    ) -> Result<String, Ice> {
        // Scrutinee into a stable slot: plain vars stay direct, everything
        // else materializes into exactly one temp (multi-use arm tests must
        // never re-evaluate side-effecting scrutinees like `v.pop()`).
        let st = self.ty_of(scrutinee.id);
        let saved = self.cur_move;
        self.cur_move = false;
        let slot = match &scrutinee.kind {
            ExprKind::Var(name) => mangle(name),
            _ => {
                let inline = self.emit_expr(scrutinee)?;
                self.new_tmp_ty(&self.cty_val(&st), &inline)
            }
        };
        self.cur_move = saved;
        self.flush_nulls();
        // Normalize + find trailing catch-all.
        let arms = self.expand_arms(arms);
        let mut cutoff = arms.len();
        for (i, a) in arms.iter().enumerate() {
            if a.guard.is_none() && Self::pat_is_catch_all(&a.pat) {
                cutoff = i + 1;
                break;
            }
        }
        let arms = &arms[..cutoff.min(arms.len())];
        // Result slot for value matches.
        let res = match res_ty {
            Some(t) => {
                let r = self.new_tmp_ty(&self.cty_val(t), &format!("({}){{0}}", self.cty_val(t)));
                Some(r)
            }
            None => None,
        };
        let mend = self.fresh_tmp();
        // Arms are SEQUENTIAL `if`s (never an else-chain): a taken arm leaves
        // via `goto`, so guard-false and test-false both fall through to the
        // next arm. (An else-chain would wrongly skip later arms when an early
        // test passed but its guard failed.)
        for (i, arm) in arms.iter().enumerate() {
            let is_last_catch =
                i + 1 == arms.len() && arm.guard.is_none() && Self::pat_is_catch_all(&arm.pat);
            if is_last_catch {
                // Trailing catch-all: reached only when nothing took yet.
                self.out.push_str("{\n");
            } else {
                let test = self.pat_test(&arm.pat, &st, &slot)?;
                self.out.push_str(&format!("if ({}) {{\n", test));
            }
            self.scopes.push(vec![]);
            let depth = self.scopes.len() - 1;
            self.push_frame();
            self.pat_binds(&arm.pat, &st, &slot)?;
            // res context: value matches move tails into res.
            let res_ctx = res.as_ref().map(|r| {
                let tt: &Ty = res_ty.unwrap_or(&Ty::Unit);
                (r.as_str(), tt, mv)
            });
            if let Some(g) = &arm.guard {
                let gs = self.cur_move;
                self.cur_move = false;
                let gc = self.emit_expr(g)?;
                self.cur_move = gs;
                self.flush_nulls();
                self.out.push_str(&format!("if ({}) {{\n", gc));
                self.emit_match_body(&arm.body, res_ctx, ret_ty)?;
                self.emit_scope_frees(depth);
                self.flush_nulls();
                self.pop_frame();
                self.out.push_str(&format!("goto {};\n}}\n", mend));
                self.emit_scope_frees(depth);
            } else {
                self.emit_match_body(&arm.body, res_ctx, ret_ty)?;
                self.emit_scope_frees(depth);
                self.flush_nulls();
                self.pop_frame();
                // Catch-all trailing block falls through to `mend` naturally;
                // every other taken arm must skip the remaining arms.
                if !is_last_catch {
                    self.out.push_str(&format!("goto {};\n", mend));
                }
            }
            self.out.push_str("}\n");
            self.scopes.pop();
        }
        if res_ty.is_some() && !self.ends_with_catch_all(arms) {
            // Guards may all fail at runtime: abort (sema proved coverage modulo guards).
            self.out
                .push_str("{ fprintf(stderr, \"non-exhaustive match\\n\"); abort(); }\n");
        }
        self.out.push_str(&format!("{}:;\n", mend));
        Ok(res.unwrap_or_else(|| "0".to_string()))
    }

    /// Whether the emitted chain ends in an unguarded catch-all arm.
    fn ends_with_catch_all(&self, arms: &[MatchArm]) -> bool {
        match arms.last() {
            Some(a) => a.guard.is_none() && Self::pat_is_catch_all(&a.pat),
            None => false,
        }
    }

    /// Copy-valued arm: tail assigns into `target`.
    fn emit_arm_copy(&mut self, b: &Block, target: &str) -> Result<(), Ice> {
        self.scopes.push(vec![]);
        let depth = self.scopes.len() - 1;
        self.push_frame();
        for s in &b.stmts {
            self.emit_stmt(s, &Ty::Unit)?;
        }
        if let Some(t) = &b.tail {
            let saved = self.cur_move;
            self.cur_move = false;
            let v = self.emit_expr(t)?;
            self.cur_move = saved;
            self.out.push_str(&format!("{} = {};\n", target, v));
            self.flush_nulls();
        }
        self.pop_frame();
        self.emit_scope_frees(depth);
        self.scopes.pop();
        Ok(())
    }

    // ----- frees -----

    fn emit_scope_frees(&mut self, depth: usize) {
        if let Some(scope) = self.scopes.get(depth) {
            let vars = scope.clone();
            for (v, t) in vars.iter().rev() {
                self.emit_destroy(v, t);
            }
        }
    }

    /// Destroy an owned slot: `vstr_free` for Strings, drop fns for ADTs
    /// with owned content (Copy values need nothing).
    fn emit_destroy(&mut self, lval: &str, ty: &Ty) {
        if !self.moves(ty) {
            return;
        }
        match ty {
            Ty::Str => {
                self.out.push_str(&format!("vstr_free(&{});\n", lval));
            }
            Ty::Struct(n) | Ty::Enum(n) => {
                self.out
                    .push_str(&format!("v_drop_{}(&{});\n", dname(n), lval));
            }
            Ty::Generic { base, args } => {
                let iname = ctype_inst(base, args);
                let iname = iname.strip_prefix('V').unwrap_or(&iname);
                self.out
                    .push_str(&format!("v_drop_{}(&{});\n", iname, lval));
            }
            _ => {}
        }
    }

    fn emit_all_frees(&mut self) {
        for d in (0..self.scopes.len()).rev() {
            self.emit_scope_frees(d);
        }
    }

    /// Free owned locals in scopes at/after `depth` (for break/continue out of
    /// a loop body: intervening scopes' resources must run before leaving).
    fn emit_scope_frees_from(&mut self, depth: usize) {
        for d in (depth..self.scopes.len()).rev() {
            self.emit_scope_frees(d);
        }
    }
}

fn mangle(name: &str) -> String {
    format!("v_{}", name.replace('_', "__"))
}

/// C name for a function in a module: entry keeps `v_name`; others get
/// `v_<mod>___<name>` (dots in the module path become `_`). The triple
/// underscore cannot arise from `mangle` (which only ever doubles), so the
/// split is unambiguous in practice; user idents are `__`-escaped first.
fn cfun(module: &str, name: &str) -> String {
    if module.is_empty() {
        return mangle(name);
    }
    format!(
        "v_{}___{}",
        module.replace('_', "__").replace('.', "_"),
        name.replace('_', "__")
    )
}

/// Mirror of the parser's place conversion, for emitting nested place
/// expressions (address-of, deref chains) as C lvalues.
fn expr_place(e: &Expr) -> Option<Place> {
    match &e.kind {
        ExprKind::Var(name) => Some(Place::Var(name.clone(), e.span)),
        ExprKind::Field {
            base,
            field,
            field_span,
        } => {
            let inner = expr_place(base)?;
            Some(Place::Field {
                base: Box::new(inner),
                field: field.clone(),
                span: *field_span,
            })
        }
        ExprKind::Unary {
            op: UnOp::Deref,
            expr,
        } => {
            let inner = expr_place(expr)?;
            Some(Place::Deref {
                inner: Box::new(inner),
                span: e.span,
            })
        }
        _ => None,
    }
}

/// C lvalue for an assignment/borrow target place. Dereferences parenthesize
/// (`(*r).f`); field access on references uses `->` when the base is a
/// reference-typed variable slot (resolved by the caller passing `arrow`).
fn place_lvalue(place: &Place, gen: &Gen) -> String {
    match place {
        Place::Var(n, _) => mangle(n),
        Place::Field { base, field, .. } => {
            let b = place_lvalue(base, gen);
            // Arrow when the immediate base slot holds a reference.
            if gen.base_is_ref(base) {
                format!("{}->{}", b, cfield(field))
            } else {
                format!("{}.{}", b, cfield(field))
            }
        }
        Place::Deref { inner, .. } => format!("(*{})", place_lvalue(inner, gen)),
        // Index is always the outermost layer (parser nests outside-in)
        // and assignment lowering intercepts it above, so this is dead.
        // Nested index stores are rejected in sema.
        Place::Index { .. } => unreachable!("nested index store reached codegen"),
    }
}

/// Whether a place contains an index layer (defense in depth: nested
/// index stores are rejected in sema before lowering).
fn place_has_index(p: &Place) -> bool {
    match p {
        Place::Index { .. } => true,
        Place::Field { base, .. } => place_has_index(base),
        Place::Deref { inner, .. } => place_has_index(inner),
        Place::Var(_, _) => false,
    }
}

/// C type name for a Valvet struct/enum. Module qualifiers (`a.b::Point`)
/// sanitize to underscores (`Va_b__Point`); plain names are unaffected.
/// Drop/clone helper-name fragment for a struct/enum (`Point` -> `Point`,
/// `a.b::Point` -> `a_b__Point`). Identity for unqualified names, so
/// single-file output is unchanged.
fn dname(name: &str) -> String {
    name.replace([':', '.'], "_")
}

fn ctype_name(name: &str) -> String {
    format!("V{}", name.replace([':', '.'], "_"))
}

/// C field name for a Valvet struct field (`_0` tuple fields included).
fn cfield(name: &str) -> String {
    format!("v_{}", name.replace('_', "__"))
}

/// C union member name for enum variant index `i`.
fn cmember(i: usize) -> String {
    format!("v{}", i)
}

/// Helper-name fragment for an instantiation (`Vec_Int32` for `VVec_Int32`).
fn iname_of(ct: &str) -> String {
    ct.strip_prefix('V').unwrap_or(ct).to_string()
}

/// C type name for a builtin generic instantiation (`Option<Int32>` etc.).
fn ctype_inst(base: &str, args: &[Ty]) -> String {
    let inner: Vec<String> = args.iter().map(ctype_arg).collect();
    format!("V{}_{}", base, inner.join("_"))
}

/// C-safe name fragment for one instantiation argument.
fn ctype_arg(ty: &Ty) -> String {
    match ty {
        Ty::Int32 => "Int32".to_string(),
        Ty::Int64 => "Int64".to_string(),
        Ty::Float64 => "Float64".to_string(),
        Ty::Bool => "Bool".to_string(),
        Ty::Str => "String".to_string(),
        Ty::Unit => "Unit".to_string(),
        Ty::Named(n) => format!("unresolved_{}", n),
        // Qualified names sanitize like `ctype_name` (C idents).
        Ty::Struct(n) | Ty::Enum(n) => n.replace([':', '.'], "_"),
        Ty::Generic { base, args } => ctype_inst(base, args)
            .strip_prefix('V')
            .unwrap_or("X")
            .to_string(),
        // No nested references in 0.1.0; total fallback.
        Ty::Reference { .. } => "ref".to_string(),
    }
}

/// C expression for a checked `as` cast (D3 §1). int<->int wraps via plain C
/// conversion (deterministic on GCC/Clang); float->int saturates via helpers
/// (a raw C cast would be UB on overflow).
fn cast_expr(from: &Ty, to: &Ty, inner: &str) -> String {
    use Ty::*;
    if from == to {
        return format!("({})", inner);
    }
    match (from, to) {
        (Int32, Int64) => format!("((int64_t)({}))", inner),
        (Int64, Int32) => format!("((int32_t)({}))", inner),
        (Int32 | Int64, Float64) => format!("((double)({}))", inner),
        (Float64, Int32) => format!("(v_f2i32({}))", inner),
        (Float64, Int64) => format!("(v_f2i64({}))", inner),
        _ => format!("({})", inner),
    }
}

fn c_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c if (c as u32) < 0x20 || (c as u32) == 0x7f => {
                out.push_str(&format!("\\x{:02x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out
}

// ---------- C driver ----------

/// Locate a C compiler: $VALVET_CC, user-local vgcc, then PATH.
pub fn find_cc() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("VALVET_CC") {
        let pb = PathBuf::from(&p);
        if pb.exists() {
            return Some(pb);
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        let pb = Path::new(&home).join(".local/vgcc/usr/bin/gcc");
        if pb.exists() {
            return Some(pb);
        }
    }
    for cand in ["gcc", "cc"] {
        if let Ok(out) = Command::new("sh")
            .arg("-c")
            .arg(format!("command -v {}", cand))
            .output()
        {
            if out.status.success() {
                let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if !p.is_empty() {
                    return Some(PathBuf::from(p));
                }
            }
        }
    }
    None
}

/// Compile C source file to native executable. Returns stderr on failure.
pub fn cc_build(c_file: &Path, exe: &Path) -> Result<(), String> {
    let cc = find_cc().ok_or_else(|| "no C compiler found (set VALVET_CC)".to_string())?;
    let mut cmd = Command::new(&cc);
    cmd.arg("-std=c11")
        .arg("-O2")
        .arg("-fwrapv")
        .arg("-o")
        .arg(exe)
        .arg(c_file);
    // libisl lives in vgcc/usr/lib; the driver needs it for cc1.
    let mut ld_path = std::env::var("LD_LIBRARY_PATH").unwrap_or_default();
    if let Ok(home) = std::env::var("HOME") {
        let vlib = format!("{}/.local/vgcc/usr/lib", home);
        if Path::new(&vlib).exists() {
            if !ld_path.is_empty() {
                ld_path = format!("{}:{}", vlib, ld_path);
            } else {
                ld_path = vlib;
            }
        }
    }
    cmd.env("LD_LIBRARY_PATH", ld_path);
    let out = cmd
        .output()
        .map_err(|e| format!("failed to run C compiler: {}", e))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!(
            "C compiler failed:\n{}",
            String::from_utf8_lossy(&out.stderr)
        ))
    }
}
