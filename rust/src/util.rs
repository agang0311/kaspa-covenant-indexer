//! JSON helpers reproducing the JavaScript semantics the JS build relies on (String(x), BigInt(x), Number(x),
//! truthiness, canonical key order). Amounts never pass through f64: integers stay exact (i128).
use anyhow::{Result, anyhow};
use serde_json::{Map, Value};

pub type Obj = Map<String, Value>;

pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

pub fn iso_now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

/// JS truthiness of a JSON value (`undefined` is represented by `None`/missing).
pub fn truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().map(|f| f != 0.0 && !f.is_nan()).unwrap_or(true),
        Some(Value::String(s)) => !s.is_empty(),
        Some(_) => true,
    }
}

/// `x == null` in JS (null or undefined).
pub fn is_nullish(v: Option<&Value>) -> bool {
    matches!(v, None | Some(Value::Null))
}

fn num_to_js(n: &serde_json::Number) -> String {
    if let Some(i) = n.as_i64() {
        return i.to_string();
    }
    if let Some(u) = n.as_u64() {
        return u.to_string();
    }
    let f = n.as_f64().unwrap_or(f64::NAN);
    if f.fract() == 0.0 && f.abs() < 1e21 {
        format!("{}", f as i128)
    } else {
        format!("{f}")
    }
}

/// `String(x)`.
pub fn js_string(v: Option<&Value>) -> String {
    match v {
        None => "undefined".into(),
        Some(Value::Null) => "null".into(),
        Some(Value::Bool(b)) => b.to_string(),
        Some(Value::Number(n)) => num_to_js(n),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(a)) => a
            .iter()
            .map(|x| {
                if matches!(x, Value::Null) {
                    String::new()
                } else {
                    js_string(Some(x))
                }
            })
            .collect::<Vec<_>>()
            .join(","),
        Some(Value::Object(_)) => "[object Object]".into(),
    }
}

/// `BigInt(x)` for the shapes that occur (integers, decimal strings, booleans); errors mirror the JS ones.
pub fn js_bigint(v: Option<&Value>) -> Result<i128> {
    match v {
        None => Err(anyhow!("Cannot convert undefined to a BigInt")),
        Some(Value::Null) => Err(anyhow!("Cannot convert null to a BigInt")),
        Some(Value::Bool(b)) => Ok(*b as i128),
        Some(Value::Number(n)) => {
            if let Some(i) = n.as_i64() {
                return Ok(i as i128);
            }
            if let Some(u) = n.as_u64() {
                return Ok(u as i128);
            }
            let f = n.as_f64().unwrap_or(f64::NAN);
            if f.is_finite() && f.fract() == 0.0 {
                Ok(f as i128)
            } else {
                Err(anyhow!(
                    "The number {} cannot be converted to a BigInt because it is not an integer",
                    num_to_js(n)
                ))
            }
        }
        Some(Value::String(s)) => {
            parse_bigint_str(s).ok_or_else(|| anyhow!("Cannot convert {s} to a BigInt"))
        }
        Some(_) => Err(anyhow!("Cannot convert {} to a BigInt", js_string(v))),
    }
}

fn parse_bigint_str(s: &str) -> Option<i128> {
    let t = s.trim();
    if t.is_empty() {
        return Some(0);
    }
    if let Some(h) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        return i128::from_str_radix(h, 16).ok();
    }
    let (neg, digits) = match t.strip_prefix('-') {
        Some(d) => (true, d),
        None => (false, t.strip_prefix('+').unwrap_or(t)),
    };
    if digits.is_empty() || !digits.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let n: i128 = digits.parse().ok()?;
    Some(if neg { -n } else { n })
}

/// `Number(x)` (NaN -> None).
pub fn js_number(v: Option<&Value>) -> Option<f64> {
    match v {
        None => None,
        Some(Value::Null) => Some(0.0),
        Some(Value::Bool(b)) => Some(*b as u8 as f64),
        Some(Value::Number(n)) => n.as_f64(),
        Some(Value::String(s)) => {
            let t = s.trim();
            if t.is_empty() {
                Some(0.0)
            } else {
                t.parse::<f64>().ok()
            }
        }
        _ => None,
    }
}

/// `Number(x)` as an exact integer index (used for outpoint indices). None when NaN / non-integer.
pub fn js_int(v: Option<&Value>) -> Option<i64> {
    let f = js_number(v)?;
    if f.fract() == 0.0 && f.abs() < 9.007_199_254_740_992e15 {
        Some(f as i64)
    } else {
        None
    }
}

/// JSON value of a JS number produced by `Number(...)` of an integer (keeps integers integral in the output).
pub fn num(n: i64) -> Value {
    Value::from(n)
}

/// `canonical(x)`: recursively sort object keys (bigints do not exist in JSON values).
pub fn canonical(v: &Value) -> Value {
    match v {
        Value::Array(a) => Value::Array(a.iter().map(canonical).collect()),
        Value::Object(o) => {
            let mut keys: Vec<&String> = o.keys().collect();
            keys.sort();
            let mut out = Map::new();
            for k in keys {
                out.insert(k.clone(), canonical(&o[k]));
            }
            Value::Object(out)
        }
        x => x.clone(),
    }
}

pub fn get<'a>(v: &'a Value, path: &[&str]) -> Option<&'a Value> {
    let mut cur = v;
    for k in path {
        cur = cur.get(*k)?;
    }
    Some(cur)
}

pub fn get_str<'a>(v: &'a Value, path: &[&str]) -> Option<&'a str> {
    get(v, path).and_then(|x| x.as_str())
}

pub fn obj_mut(v: &mut Value) -> &mut Obj {
    if !v.is_object() {
        *v = Value::Object(Map::new());
    }
    v.as_object_mut().unwrap()
}

/// `{...a, ...b}` (b's keys override a's, keeping a's key positions like JS spread).
pub fn spread(a: &Value, b: Value) -> Value {
    let mut out = a.as_object().cloned().unwrap_or_default();
    if let Value::Object(bo) = b {
        for (k, v) in bo {
            out.insert(k, v);
        }
    }
    Value::Object(out)
}

pub fn is_hash(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

pub fn opt_is_hash(v: Option<&Value>) -> bool {
    v.and_then(|x| x.as_str()).map(is_hash).unwrap_or(false)
}

/// `String(e?.message ?? e)`, also used for the regexes the JS build applies to error messages.
pub fn msg(e: &anyhow::Error) -> String {
    e.to_string()
}

pub fn slice_chars(s: &str, n: usize) -> String {
    // JS String.prototype.slice works on UTF-16 units; messages here are ASCII in practice.
    s.chars().take(n).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn js_semantics() {
        assert_eq!(js_string(Some(&json!(400000000))), "400000000");
        assert_eq!(js_string(Some(&json!(null))), "null");
        assert_eq!(
            js_bigint(Some(&json!("601431288116"))).unwrap(),
            601431288116
        );
        assert!(js_bigint(Some(&json!(null))).is_err());
        assert_eq!(js_bigint(Some(&json!(""))).unwrap(), 0);
        assert_eq!(
            canonical(&json!({"b":1,"a":{"d":2,"c":3}})).to_string(),
            r#"{"a":{"c":3,"d":2},"b":1}"#
        );
    }
}
