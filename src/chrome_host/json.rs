//! Small structured writer for chrome snapshots. Containers close when their builder returns.

use super::protocol::json_string;

pub fn capped(text: &str, max_chars: usize) -> &str {
    match text.char_indices().nth(max_chars) {
        Some((at, _)) => &text[..at],
        None => text,
    }
}

pub struct Json;

impl Json {
    pub fn object(build: impl FnOnce(&mut Object<'_>)) -> String {
        let mut out = String::new();
        write_object(&mut out, build);
        out
    }

    pub fn array(build: impl FnOnce(&mut Array<'_>)) -> String {
        let mut out = String::new();
        write_array(&mut out, build);
        out
    }
}

pub struct Object<'a> { out: &'a mut String, first: bool }
pub struct Array<'a> { out: &'a mut String, first: bool }

impl Object<'_> {
    fn key(&mut self, key: &str) {
        comma(self.out, &mut self.first);
        self.out.push_str(&json_string(key));
        self.out.push(':');
    }
    pub fn string(&mut self, key: &str, value: &str) { self.key(key); self.out.push_str(&json_string(value)); }
    pub fn int(&mut self, key: &str, value: i64) { self.key(key); self.out.push_str(&value.to_string()); }
    pub fn uint(&mut self, key: &str, value: u64) { self.key(key); self.out.push_str(&value.to_string()); }
    pub fn bool(&mut self, key: &str, value: bool) { self.key(key); self.out.push_str(if value { "true" } else { "false" }); }
    pub fn null(&mut self, key: &str) { self.key(key); self.out.push_str("null"); }
    pub fn object(&mut self, key: &str, build: impl FnOnce(&mut Object<'_>)) { self.key(key); write_object(self.out, build); }
    pub fn array(&mut self, key: &str, build: impl FnOnce(&mut Array<'_>)) { self.key(key); write_array(self.out, build); }
}

impl Array<'_> {
    fn next(&mut self) { comma(self.out, &mut self.first); }
    pub fn string(&mut self, value: &str) { self.next(); self.out.push_str(&json_string(value)); }
    pub fn int(&mut self, value: i64) { self.next(); self.out.push_str(&value.to_string()); }
    pub fn uint(&mut self, value: u64) { self.next(); self.out.push_str(&value.to_string()); }
    pub fn bool(&mut self, value: bool) { self.next(); self.out.push_str(if value { "true" } else { "false" }); }
    pub fn object(&mut self, build: impl FnOnce(&mut Object<'_>)) { self.next(); write_object(self.out, build); }
    pub fn array(&mut self, build: impl FnOnce(&mut Array<'_>)) { self.next(); write_array(self.out, build); }
}

fn comma(out: &mut String, first: &mut bool) {
    if *first { *first = false; } else { out.push(','); }
}
fn write_object(out: &mut String, build: impl FnOnce(&mut Object<'_>)) {
    out.push('{');
    build(&mut Object { out, first: true });
    out.push('}');
}
fn write_array(out: &mut String, build: impl FnOnce(&mut Array<'_>)) {
    out.push('[');
    build(&mut Array { out, first: true });
    out.push(']');
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nesting_and_empty_containers_close_correctly() {
        assert_eq!(Json::object(|o| { o.array("items", |a| {
            a.object(|o| { o.string("x", "é😀"); o.bool("on", true); });
            a.array(|a| { a.int(-2); a.uint(3); });
        }); o.object("empty", |_| {}); }),
        r#"{"items":[{"x":"é😀","on":true},[-2,3]],"empty":{}}"#);
        assert_eq!(Json::array(|_| {}), "[]");
        assert_eq!(Json::object(|_| {}), "{}");
    }

    #[test]
    fn escaping_and_character_cap() {
        assert_eq!(Json::array(|a| a.string("<\u{2028}\u{2029}\n\"\\\u{1}")),
            r#"["\u003c\u2028\u2029\n\"\\\u0001"]"#);
        assert_eq!(capped("é😀x", 2), "é😀");
        assert_eq!(capped("é😀x", 0), "");
        assert_eq!(capped("é😀x", 9), "é😀x");
    }
}
