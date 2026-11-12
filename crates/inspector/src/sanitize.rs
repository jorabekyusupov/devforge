use serde_json::Value;
use std::sync::LazyLock;
static SENSITIVE: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(
        r"(?i)password|passwd|secret|token|authorization|cookie|credential|bindings|headers|body|environment|api.?key|app.?key",
    )
    .unwrap()
});
static KEY_VALUE: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"(?i)(password|passwd|secret|token|authorization|api[_-]?key|app[_-]?key)\s*[:=]\s*[^\s,;]+" ).unwrap()
});
pub fn text(s: &str, token: &str) -> String {
    let value = s.replace(token, "[REDACTED]");
    let value = KEY_VALUE.replace_all(&value, "$1=[REDACTED]");
    value
        .chars()
        .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
        .take(2048)
        .collect()
}
pub fn value(v: Value, token: &str, depth: usize) -> Value {
    if depth >= 6 {
        return Value::String("[depth limit]".into());
    }
    match v {
        Value::String(s) => Value::String(text(&s, token)),
        Value::Array(a) => Value::Array(
            a.into_iter()
                .take(40)
                .map(|v| value(v, token, depth + 1))
                .collect(),
        ),
        Value::Object(o) => Value::Object(
            o.into_iter()
                .take(40)
                .map(|(k, v)| {
                    let v = if SENSITIVE.is_match(&k) {
                        Value::String("[REDACTED]".into())
                    } else {
                        value(v, token, depth + 1)
                    };
                    (text(&k, token), v)
                })
                .collect(),
        ),
        v => v,
    }
}
/// SQL literals and comments are discarded; bindings are never transported.
pub fn sql(s: &str) -> String {
    let chars: Vec<_> = s.chars().take(8192).collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '\'' {
            out.push('?');
            i += 1;
            while i < chars.len() {
                if chars[i] == '\\' {
                    i = (i + 2).min(chars.len());
                    continue;
                }
                if chars[i] == '\'' {
                    i += 1;
                    if i < chars.len() && chars[i] == '\'' {
                        i += 1;
                        continue;
                    }
                    break;
                }
                i += 1;
            }
            continue;
        }
        if c == '-' && chars.get(i + 1) == Some(&'-') {
            while i < chars.len() && chars[i] != '\n' {
                i += 1
            }
            out.push(' ');
            continue;
        }
        if c == '/' && chars.get(i + 1) == Some(&'*') {
            i += 2;
            while i + 1 < chars.len() && !(chars[i] == '*' && chars[i + 1] == '/') {
                i += 1
            }
            i = (i + 2).min(chars.len());
            out.push(' ');
            continue;
        }
        if c == '$' {
            let mut end = i + 1;
            while end < chars.len() && (chars[end].is_ascii_alphanumeric() || chars[end] == '_') {
                end += 1
            }
            if chars.get(end) == Some(&'$') {
                let tag = &chars[i..=end];
                let begin = end + 1;
                let mut close = begin;
                while close + tag.len() <= chars.len() && &chars[close..close + tag.len()] != tag {
                    close += 1
                }
                out.push('?');
                i = (close + tag.len()).min(chars.len());
                continue;
            }
        }
        if c.is_ascii_digit()
            && (i == 0 || !(chars[i - 1].is_alphanumeric() || chars[i - 1] == '_'))
        {
            out.push('?');
            i += 1;
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || "._+-".contains(chars[i]))
            {
                i += 1
            }
            continue;
        }
        out.push(c);
        i += 1;
    }
    out.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(2048)
        .collect()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn strips_sql_values_without_executing_them() {
        let s = sql(
            "select * from \"users\" where email='private@example.test' and id=42 /* token=abc */ and note=$t$private$t$",
        );
        assert_eq!(
            s,
            "select * from \"users\" where email=? and id=? and note=?"
        );
    }
    #[test]
    fn masks_nested_credentials_and_token() {
        let v = value(
            serde_json::json!({"password":"private","nested":{"api_key":"x","message":"token=abc"}}),
            "never-the-token",
            0,
        );
        assert!(!v.to_string().contains("private"));
        assert!(!v.to_string().contains("abc"));
    }
}
