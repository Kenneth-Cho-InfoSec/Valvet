use std::collections::BTreeMap;

use crate::ast::UnitExpr;
use crate::source::Span;

/// A resolved dimension: exponents over SI base units plus a scale factor
/// (stored number × scale = base SI units). Dimension vectors decide
/// compatibility; scales drive conversions.
#[derive(Debug, Clone, PartialEq)]
pub struct Dim {
    /// Base-dimension exponents (`m`, `kg`, `s`, `K`, `mol`, `A`, `cd`).
    pub exps: BTreeMap<String, i32>,
    /// Scale to base SI (`km` → 1000 with `{m: 1}`).
    pub scale: f64,
    /// Original spelling, for diagnostics (`km/h`).
    pub repr: String,
}

impl Dim {
    pub fn dimensionless() -> Self {
        Dim {
            exps: BTreeMap::new(),
            scale: 1.0,
            repr: "1".to_string(),
        }
    }

    pub fn is_dimensionless(&self) -> bool {
        self.exps.is_empty()
    }

    /// Same physical dimension (scales may differ: `km/h` vs `m/s`).
    pub fn same_dim(&self, other: &Dim) -> bool {
        self.exps == other.exps
    }

    /// Factor converting a number in `self` to `other` (same dimension).
    pub fn to(&self, other: &Dim) -> f64 {
        self.scale / other.scale
    }

    pub fn mul(&self, other: &Dim) -> Dim {
        let mut exps = self.exps.clone();
        for (k, v) in &other.exps {
            let e = exps.get(k).copied().unwrap_or(0).saturating_add(*v);
            if e == 0 {
                exps.remove(k);
            } else {
                exps.insert(k.clone(), e);
            }
        }
        Dim {
            exps,
            scale: self.scale * other.scale,
            repr: format!("{}*{}", self.repr, other.repr),
        }
    }

    pub fn div(&self, other: &Dim) -> Dim {
        let mut exps = self.exps.clone();
        for (k, v) in &other.exps {
            let e = exps.get(k).copied().unwrap_or(0).saturating_sub(*v);
            if e == 0 {
                exps.remove(k);
            } else {
                exps.insert(k.clone(), e);
            }
        }
        Dim {
            exps,
            scale: self.scale / other.scale,
            repr: format!("{}/{}", self.repr, other.repr),
        }
    }

    pub fn pow(&self, n: i32) -> Dim {
        let mut exps = BTreeMap::new();
        for (k, v) in &self.exps {
            let e = v.saturating_mul(n);
            if e != 0 {
                exps.insert(k.clone(), e);
            }
        }
        Dim {
            exps,
            scale: self.scale.powi(n),
            repr: if n == 1 {
                self.repr.clone()
            } else {
                format!("{}^{}", self.repr, n)
            },
        }
    }

    /// Canonical display: `m/s`, `kg*m/s^2`, with a scale prefix when
    /// the spelling was lost (`1000*m`).
    pub fn display(&self) -> String {
        if self.exps.is_empty() {
            return "1".to_string();
        }
        let mut num = vec![];
        let mut den = vec![];
        for (k, v) in &self.exps {
            let part = if v.abs() == 1 {
                k.clone()
            } else {
                format!("{}^{}", k, v.abs())
            };
            if *v > 0 {
                num.push(part);
            } else {
                den.push(part);
            }
        }
        let mut s = num.join("*");
        if !den.is_empty() {
            s.push('/');
            s.push_str(den.join("*").as_str());
        }
        if (self.scale - 1.0).abs() > f64::EPSILON {
            format!("{}*{}", trim_float(self.scale), s)
        } else {
            s
        }
    }
}

fn trim_float(v: f64) -> String {
    format!("{}", v)
}

/// One builtin unit: scale to SI + base-dimension exponents.
type UnitDef = (f64, BTreeMap<String, i32>);

fn base(name: &str, scale: f64, exps: &[(&str, i32)]) -> (String, UnitDef) {
    let mut m = BTreeMap::new();
    for (k, v) in exps {
        m.insert(k.to_string(), *v);
    }
    (name.to_string(), (scale, m))
}

/// Resolve a unit name: direct hit, else one SI prefix + remainder
/// (`ms` is millisecond: prefix binds tighter than juxtaposition).
fn resolve_name(name: &str) -> Option<(f64, BTreeMap<String, i32>)> {
    let table: Vec<(String, UnitDef)> = vec![
        base("m", 1.0, &[("m", 1)]),
        base("kg", 1.0, &[("kg", 1)]),
        base("s", 1.0, &[("s", 1)]),
        base("K", 1.0, &[("K", 1)]),
        base("mol", 1.0, &[("mol", 1)]),
        base("A", 1.0, &[("A", 1)]),
        base("cd", 1.0, &[("cd", 1)]),
        base("g", 1e-3, &[("kg", 1)]),
        base("Hz", 1.0, &[("s", -1)]),
        base("N", 1.0, &[("kg", 1), ("m", 1), ("s", -2)]),
        base("Pa", 1.0, &[("kg", 1), ("m", -1), ("s", -2)]),
        base("J", 1.0, &[("kg", 1), ("m", 2), ("s", -2)]),
        base("W", 1.0, &[("kg", 1), ("m", 2), ("s", -3)]),
        base("V", 1.0, &[("kg", 1), ("m", 2), ("s", -3), ("A", -1)]),
        base("C", 1.0, &[("s", 1), ("A", 1)]),
        base("eV", 1.602176634e-19, &[("kg", 1), ("m", 2), ("s", -2)]),
        base("min", 60.0, &[("s", 1)]),
        base("h", 3600.0, &[("s", 1)]),
        base("hr", 3600.0, &[("s", 1)]),
        base("day", 86400.0, &[("s", 1)]),
        base("inch", 0.0254, &[("m", 1)]),
        base("ft", 0.3048, &[("m", 1)]),
        base("mile", 1609.344, &[("m", 1)]),
        base("lb", 0.45359237, &[("kg", 1)]),
        base("L", 1e-3, &[("m", 3)]),
        base("rad", 1.0, &[]),
    ];
    for (n, v) in &table {
        if n == name {
            return Some(v.clone());
        }
    }
    // SI prefixes (case-sensitive): k M G m u n p.
    let prefixes: &[(&str, f64)] = &[
        ("k", 1e3),
        ("M", 1e6),
        ("G", 1e9),
        ("m", 1e-3),
        ("u", 1e-6),
        ("n", 1e-9),
        ("p", 1e-12),
    ];
    for (pre, ps) in prefixes {
        if let Some(rest) = name.strip_prefix(pre) {
            if rest.is_empty() {
                continue;
            }
            // The remainder must itself resolve (one level).
            for (n, (s, e)) in &table {
                if n == rest {
                    return Some((*ps * *s, e.clone()));
                }
            }
        }
    }
    None
}

/// Evaluate a parsed unit expression into a `Dim`.
/// Unknown names report the span for E0401.
pub fn eval(expr: &UnitExpr) -> Result<Dim, (Span, String)> {
    match expr {
        UnitExpr::Name(n, sp) => match resolve_name(n) {
            Some((scale, exps)) => Ok(Dim {
                exps,
                scale,
                repr: n.clone(),
            }),
            None => Err((*sp, format!("unknown unit `{}`", n))),
        },
        UnitExpr::Mul(a, b) => {
            let x = eval(a)?;
            let y = eval(b)?;
            Ok(x.mul(&y))
        }
        UnitExpr::Div(a, b) => {
            let x = eval(a)?;
            let y = eval(b)?;
            Ok(x.div(&y))
        }
        UnitExpr::Pow(a, n) => {
            let x = eval(a)?;
            Ok(x.pow(*n))
        }
    }
}
