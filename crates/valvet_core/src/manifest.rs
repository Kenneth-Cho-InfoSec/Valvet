/// `valvet.toml` project manifest (Batch 1): `[package]` identity plus
/// `[dependencies]` path deps with semver bounds. TOML subset only
/// (tables, strings, inline tables); anything else names its line.
#[derive(Debug, Clone, Default)]
pub struct Manifest {
    pub name: String,
    pub version: String,
    pub deps: Vec<Dep>,
}

/// One dependency: path (`{ path, version }`) or git (`{ git, rev }` for
/// a pinned commit, `{ git, version }` for latest tag matching the bound).
#[derive(Debug, Clone)]
pub struct Dep {
    pub name: String,
    pub path: Option<String>,
    pub git: Option<String>,
    pub rev: Option<String>,
    pub req: String,
}

/// Resolved dependency record for `valvet.lock` (`dep` lines).
#[derive(Debug, Clone)]
pub struct DepLock {
    pub name: String,
    pub version: String,
    pub hash: String,
}

fn bad(line: usize, what: &str) -> String {
    format!("valvet.toml:{}: {}", line + 1, what)
}

fn unquote(s: &str) -> Option<String> {
    let s = s.trim();
    if s.len() >= 2 && s.starts_with('"') && s.ends_with('"') {
        Some(s[1..s.len() - 1].to_string())
    } else {
        None
    }
}

/// Parse one `key = value` line into (key, value).
fn kv(line: &str) -> Option<(String, String)> {
    let eq = line.find('=')?;
    let (k, v) = line.split_at(eq);
    let k = k.trim();
    if k.is_empty() || k.contains([' ', '\t', '[', ']']) {
        return None;
    }
    Some((k.to_string(), v[1..].trim().to_string()))
}

fn parse_dep_value(v: &str, line: usize) -> Result<DepTail, String> {
    let v = v.trim();
    if !v.starts_with('{') || !v.ends_with('}') {
        return Err(bad(line, "dependency value must be an inline table"));
    }
    let inner = &v[1..v.len() - 1];
    let mut path = None;
    let mut git = None;
    let mut rev = None;
    let mut version = None;
    for part in inner.split(',') {
        let (k, val) = kv(part).ok_or_else(|| bad(line, "bad inline-table entry"))?;
        let val = unquote(&val).ok_or_else(|| bad(line, "dependency values must be strings"))?;
        match k.as_str() {
            "path" => path = Some(val),
            "git" => git = Some(val),
            "rev" => rev = Some(val),
            "version" => version = Some(val),
            _ => return Err(bad(line, &format!("unknown dependency key `{}`", k))),
        }
    }
    match (&path, &git) {
        (Some(_), Some(_)) => Err(bad(line, "dependency takes `path` or `git`, not both")),
        (None, None) => Err(bad(line, "dependency needs `path` or `git`")),
        _ => Ok(DepTail {
            path,
            git,
            rev,
            req: version.unwrap_or_default(),
        }),
    }
}

struct DepTail {
    path: Option<String>,
    git: Option<String>,
    rev: Option<String>,
    req: String,
}

pub fn parse(text: &str) -> Result<Manifest, String> {
    let mut m = Manifest::default();
    let mut section = String::new();
    for (i, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            section = line[1..line.len() - 1].trim().to_string();
            match section.as_str() {
                "package" | "dependencies" | "profile" | "features" => {}
                _ => return Err(bad(i, &format!("unknown section `[{}]`", section))),
            }
            continue;
        }
        // Profiles and features parse but only record presence; selection
        // lands in later batches (Batch 6/7 read them back from here).
        if section == "profile" || section == "features" {
            if kv(line).is_none() {
                return Err(bad(i, "expected `key = value`"));
            }
            continue;
        }
        let (k, v) = kv(line).ok_or_else(|| bad(i, "expected `key = value`"))?;
        match section.as_str() {
            "package" => match k.as_str() {
                "name" | "version" => {
                    let val = unquote(&v).ok_or_else(|| bad(i, "expected a string"))?;
                    if k == "name" {
                        m.name = val;
                    } else {
                        m.version = val;
                    }
                }
                _ => return Err(bad(i, &format!("unknown package key `{}`", k))),
            },
            "dependencies" => {
                let tail = parse_dep_value(&v, i)?;
                m.deps.push(Dep {
                    name: k,
                    path: tail.path,
                    git: tail.git,
                    rev: tail.rev,
                    req: tail.req,
                });
            }
            _ => {
                return Err(bad(
                    i,
                    "keys must live under `[package]` or `[dependencies]`",
                ))
            }
        }
    }
    Ok(m)
}

/// Split `1.2.3` into parts (missing parts are 0; suffixes rejected).
pub fn ver_parts(v: &str) -> Option<[u64; 3]> {
    let mut out = [0u64; 3];
    for (i, p) in v.split('.').enumerate() {
        if i >= 3 || p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        out[i] = p.parse().ok()?;
    }
    Some(out)
}

/// Cache directory for one git URL (`~/.cache/valvet/git/<slug>`).
/// Shared by the resolver and `valvet vendor` so both agree.
pub fn git_cache_dir(url: &str) -> Option<std::path::PathBuf> {
    let home = std::env::var("HOME").ok()?;
    let mut slug: String = url
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    slug.truncate(48);
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    url.hash(&mut h);
    slug.push_str(&format!("-{:x}", h.finish()));
    Some(
        std::path::PathBuf::from(home)
            .join(".cache/valvet/git")
            .join(slug),
    )
}

/// Cargo-style max-compatible check: exact `1.2.3` or caret `^1.2.3`
/// (same major, at least the bound). Unknown shapes never match.
pub fn matches_req(version: &str, req: &str) -> bool {
    let req = req.trim();
    let bound = req.strip_prefix('^').unwrap_or(req);
    let (v, b) = match (ver_parts(version), ver_parts(bound)) {
        (Some(v), Some(b)) => (v, b),
        _ => return false,
    };
    if req.starts_with('^') {
        v[0] == b[0] && v >= b
    } else {
        v == b
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_package_and_deps() {
        let m = parse("[package]\nname = \"app\"\nversion = \"0.1.0\"\n\n[dependencies]\nmath2 = { path = \"libs/math2\", version = \"^1.0\" }\n").unwrap();
        assert_eq!(m.name, "app");
        assert_eq!(m.version, "0.1.0");
        assert_eq!(m.deps.len(), 1);
        assert_eq!(m.deps[0].name, "math2");
        assert_eq!(m.deps[0].path.as_deref(), Some("libs/math2"));
        assert_eq!(m.deps[0].req, "^1.0");
    }

    #[test]
    fn rejects_junk() {
        assert!(parse("[package]\noops = 1\n").is_err());
        assert!(parse("[nope]\n").is_err());
        assert!(parse("[dependencies]\na = \"1.0\"\n").is_err());
        let g = parse("[dependencies]\na = { git = \"https://x/y\", rev = \"abc\" }\n").unwrap();
        assert_eq!(g.deps[0].git.as_deref(), Some("https://x/y"));
        assert_eq!(g.deps[0].rev.as_deref(), Some("abc"));
        assert!(parse("[dependencies]\na = { path = \"p\", git = \"g\" }\n").is_err());
        assert!(parse("stray\n").is_err());
    }

    #[test]
    fn semver_bounds() {
        assert!(matches_req("1.2.0", "^1.0"));
        assert!(matches_req("1.0.0", "^1.0"));
        assert!(!matches_req("2.0.0", "^1.0"));
        assert!(!matches_req("0.9.9", "^1.0"));
        assert!(matches_req("1.2.3", "1.2.3"));
        assert!(!matches_req("1.2.4", "1.2.3"));
        assert!(!matches_req("xx", "^1.0"));
    }
}
