//! A JSON reader, for tests and for nothing else.
//!
//! The routes of `serve.rs` write JSON by hand, for the reason `observe/json.rs`
//! gives: the workspace has no JSON serializer and ADR-037 rejected taking a
//! dependency to print a few dozen numbers. A hand-written writer needs a reader
//! on the other side of it, though, or its tests degenerate into substring
//! matching — and substring matching cannot tell a document that parses from one
//! that merely contains the right characters, which is exactly the failure an
//! unescaped `substance.id` produces.
//!
//! So: the smallest reader that can say "this is JSON, and here is what it
//! says". Numbers are kept as the text they were written as, because the
//! assertions are about exact integers — a residual, a tick, a seed — and going
//! through `f64` would throw away the very thing being checked.
//!
//! Compiled under `cfg(test)` only. It is a test instrument, not a second
//! implementation of anything the binary does.

#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    /// The number exactly as it was written.
    Num(String),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

impl Json {
    /// Parse a whole document, or say where it stopped making sense.
    pub fn parse(text: &str) -> Result<Json, String> {
        let bytes = text.as_bytes();
        let mut at = 0usize;
        let value = parse_value(bytes, &mut at)?;
        skip_space(bytes, &mut at);
        if at != bytes.len() {
            return Err(format!("trailing bytes at {at}"));
        }
        Ok(value)
    }

    /// The value under a key of an object, or `None` — including when this is
    /// not an object at all.
    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Obj(entries) => entries
                .iter()
                .find(|(name, _)| name == key)
                .map(|(_, value)| value),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::Str(value) => Some(value),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Json::Num(text) => text.parse().ok(),
            _ => None,
        }
    }

    pub fn as_i128(&self) -> Option<i128> {
        match self {
            Json::Num(text) => text.parse().ok(),
            _ => None,
        }
    }

    pub fn as_u32(&self) -> Option<u32> {
        match self {
            Json::Num(text) => text.parse().ok(),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[Json]> {
        match self {
            Json::Arr(values) => Some(values),
            _ => None,
        }
    }
}

fn skip_space(bytes: &[u8], at: &mut usize) {
    while *at < bytes.len() && bytes[*at].is_ascii_whitespace() {
        *at += 1;
    }
}

fn parse_value(bytes: &[u8], at: &mut usize) -> Result<Json, String> {
    skip_space(bytes, at);
    match bytes.get(*at) {
        None => Err("the document ended early".into()),
        Some(b'{') => parse_object(bytes, at),
        Some(b'[') => parse_array(bytes, at),
        Some(b'"') => parse_string(bytes, at).map(Json::Str),
        Some(b't') => literal(bytes, at, "true", Json::Bool(true)),
        Some(b'f') => literal(bytes, at, "false", Json::Bool(false)),
        Some(b'n') => literal(bytes, at, "null", Json::Null),
        Some(_) => parse_number(bytes, at),
    }
}

fn literal(bytes: &[u8], at: &mut usize, text: &str, value: Json) -> Result<Json, String> {
    if bytes[*at..].starts_with(text.as_bytes()) {
        *at += text.len();
        Ok(value)
    } else {
        Err(format!("expected `{text}` at {at}"))
    }
}

fn parse_object(bytes: &[u8], at: &mut usize) -> Result<Json, String> {
    *at += 1;
    let mut entries = Vec::new();
    skip_space(bytes, at);
    if bytes.get(*at) == Some(&b'}') {
        *at += 1;
        return Ok(Json::Obj(entries));
    }
    loop {
        skip_space(bytes, at);
        let key = parse_string(bytes, at)?;
        skip_space(bytes, at);
        if bytes.get(*at) != Some(&b':') {
            return Err(format!("expected `:` at {at}"));
        }
        *at += 1;
        entries.push((key, parse_value(bytes, at)?));
        skip_space(bytes, at);
        match bytes.get(*at) {
            Some(b',') => *at += 1,
            Some(b'}') => {
                *at += 1;
                return Ok(Json::Obj(entries));
            }
            _ => return Err(format!("expected `,` or `}}` at {at}")),
        }
    }
}

fn parse_array(bytes: &[u8], at: &mut usize) -> Result<Json, String> {
    *at += 1;
    let mut values = Vec::new();
    skip_space(bytes, at);
    if bytes.get(*at) == Some(&b']') {
        *at += 1;
        return Ok(Json::Arr(values));
    }
    loop {
        values.push(parse_value(bytes, at)?);
        skip_space(bytes, at);
        match bytes.get(*at) {
            Some(b',') => *at += 1,
            Some(b']') => {
                *at += 1;
                return Ok(Json::Arr(values));
            }
            _ => return Err(format!("expected `,` or `]` at {at}")),
        }
    }
}

fn parse_string(bytes: &[u8], at: &mut usize) -> Result<String, String> {
    if bytes.get(*at) != Some(&b'"') {
        return Err(format!("expected a string at {at}"));
    }
    *at += 1;
    let mut out = String::new();
    loop {
        let byte = *bytes.get(*at).ok_or("the string never ended")?;
        *at += 1;
        match byte {
            b'"' => return Ok(out),
            b'\\' => {
                let escape = *bytes.get(*at).ok_or("the escape never ended")?;
                *at += 1;
                match escape {
                    b'"' => out.push('"'),
                    b'\\' => out.push('\\'),
                    b'/' => out.push('/'),
                    b'b' => out.push('\u{8}'),
                    b'f' => out.push('\u{c}'),
                    b'n' => out.push('\n'),
                    b'r' => out.push('\r'),
                    b't' => out.push('\t'),
                    b'u' => {
                        let hex = std::str::from_utf8(&bytes[*at..*at + 4])
                            .map_err(|_| "a \\u escape that is not hex".to_string())?;
                        let code = u32::from_str_radix(hex, 16)
                            .map_err(|_| format!("a \\u escape that is not hex: {hex}"))?;
                        *at += 4;
                        out.push(char::from_u32(code).ok_or("a \\u escape outside Unicode")?);
                    }
                    other => return Err(format!("unknown escape `\\{}`", other as char)),
                }
            }
            // A raw control byte inside a string is exactly what an unescaped
            // `substance.id` produces, and it is not JSON.
            0x00..=0x1f => return Err(format!("a raw control byte {byte:#04x} inside a string")),
            _ => {
                let start = *at - 1;
                let mut end = *at;
                while end < bytes.len() && bytes[end] & 0xc0 == 0x80 {
                    end += 1;
                }
                out.push_str(
                    std::str::from_utf8(&bytes[start..end]).map_err(|err| err.to_string())?,
                );
                *at = end;
            }
        }
    }
}

fn parse_number(bytes: &[u8], at: &mut usize) -> Result<Json, String> {
    let start = *at;
    while *at < bytes.len() && matches!(bytes[*at], b'0'..=b'9' | b'-' | b'+' | b'.' | b'e' | b'E')
    {
        *at += 1;
    }
    if start == *at {
        return Err(format!("expected a value at {start}"));
    }
    let text = std::str::from_utf8(&bytes[start..*at]).map_err(|err| err.to_string())?;
    text.parse::<f64>()
        .map_err(|_| format!("`{text}` is not a number"))?;
    Ok(Json::Num(text.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_object_reads_back_key_by_key() {
        let value = Json::parse(r#"{"a":1,"b":[true,null,"x"],"c":{"d":-2e-3}}"#).expect("JSON");
        assert_eq!(value.get("a").and_then(Json::as_i128), Some(1));
        assert_eq!(
            value.get("b").and_then(Json::as_array).map(<[Json]>::len),
            Some(3)
        );
        assert_eq!(
            value
                .get("c")
                .and_then(|c| c.get("d"))
                .and_then(Json::as_f64),
            Some(-2e-3)
        );
    }

    #[test]
    fn an_unescaped_control_byte_is_not_json() {
        // The property the escaping tests rest on: if this parser accepted a raw
        // newline inside a string, `a_substance_id_that_would_break_the_json_is_escaped`
        // would be green against a writer that escapes nothing.
        assert!(Json::parse("{\"id\":\"a\nb\"}").is_err());
        assert!(Json::parse("{\"id\":\"a\\nb\"}").is_ok());
        assert_eq!(
            Json::parse("{\"id\":\"a\\nb\"}")
                .expect("JSON")
                .get("id")
                .and_then(Json::as_str),
            Some("a\nb")
        );
    }

    #[test]
    fn a_big_integer_survives_the_reader() {
        // The reader keeps the text, so an assertion about a seed past 2^53 is
        // an assertion about the seed and not about a double.
        let value = Json::parse(r#"{"seed":"12345678901234567890"}"#).expect("JSON");
        assert_eq!(
            value.get("seed").and_then(Json::as_str),
            Some("12345678901234567890")
        );
    }
}
