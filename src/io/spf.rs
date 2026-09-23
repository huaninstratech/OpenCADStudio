// STEP Physical File (ISO-10303-21) parser.
//
// One tolerant parser serves both IFC models and CAD STEP exchanges: the
// exchange structure is the same `#12=ENTITY(args);` syntax, only the schema
// on top of it differs. Inspired by the reader architecture of ThatOpen's
// web-ifc (Apache-2.0) — tokenizer plus id-indexed entity table — re-expressed
// in Rust rather than translated.
//
// Scope notes:
//   - Header sections are scanned only for FILE_SCHEMA; no full header model.
//   - Complex entities `#1=(A(..)B(..))` keep every type name and concatenate
//     the argument groups, which is enough for the unit/context lookups used
//     here.
//   - Value-level keywords (`IFCLABEL('x')`, `IFCBOOLEAN(.T.)`) are kept as
//     `Val::Typed` wrappers so callers can unwrap them generically.

/// A value inside an entity's argument list.
#[derive(Debug, Clone, PartialEq)]
pub enum Val {
    /// `#42`
    Ref(u64),
    /// `'text'` with escapes decoded to UTF-8.
    Str(String),
    /// `.ENUMERATION.` — stored without the surrounding dots.
    Enum(String),
    /// Integral literal (no `.` or exponent in the source).
    Int(i64),
    /// Real literal.
    Num(f64),
    /// Nested `( … )` list.
    List(Vec<Val>),
    /// `KEYWORD(value)` — typed value wrapper.
    Typed(String, Box<Val>),
    /// `$`
    Unset,
    /// `*`
    Star,
}

impl Val {
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Val::Int(i) => Some(*i as f64),
            Val::Num(n) => Some(*n),
            Val::Typed(_, v) => v.as_f64(),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Val::Int(i) => Some(*i),
            Val::Num(n) if n.fract() == 0.0 => Some(*n as i64),
            Val::Typed(_, v) => v.as_i64(),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Val::Str(s) => Some(s),
            Val::Typed(_, v) => (**v).as_str(),
            _ => None,
        }
    }

    pub fn as_enum(&self) -> Option<&str> {
        match self {
            Val::Enum(e) => Some(e),
            Val::Typed(_, v) => (**v).as_enum(),
            _ => None,
        }
    }

    pub fn as_list(&self) -> Option<&[Val]> {
        match self {
            Val::List(items) => Some(items),
            Val::Typed(_, v) => (**v).as_list(),
            _ => None,
        }
    }

    pub fn as_ref(&self) -> Option<u64> {
        match self {
            Val::Ref(id) => Some(*id),
            Val::Typed(_, v) => (**v).as_ref(),
            _ => None,
        }
    }
}

/// One data-section record. `types` holds every type name of the record —
/// one for a simple entity, several for a complex `(A(..)B(..))` record.
/// Type names are `Arc`-interned: a multi-gigabyte file repeats a handful of
/// names millions of times, and shared strings keep the parse footprint
/// close to the argument data.
#[derive(Debug, Clone)]
pub struct Ent {
    pub id: u64,
    pub types: Vec<std::sync::Arc<str>>,
    pub args: Vec<Val>,
}

impl Ent {
    pub fn is(&self, ty: &str) -> bool {
        self.types.iter().any(|t| t.as_ref() == ty)
    }

    pub fn ty(&self) -> &str {
        self.types.first().map(|t| t.as_ref()).unwrap_or("")
    }

    pub fn get(&self, index: usize) -> Option<&Val> {
        self.args.get(index)
    }

    pub fn ref_id(&self, index: usize) -> Option<u64> {
        self.args.get(index).and_then(Val::as_ref)
    }

    pub fn num(&self, index: usize) -> Option<f64> {
        self.args.get(index).and_then(Val::as_f64)
    }

    pub fn int(&self, index: usize) -> Option<i64> {
        self.args.get(index).and_then(Val::as_i64)
    }

    pub fn str(&self, index: usize) -> Option<&str> {
        self.args.get(index).and_then(Val::as_str)
    }

    pub fn list(&self, index: usize) -> Option<&[Val]> {
        self.args.get(index).and_then(Val::as_list)
    }

    /// Resolve argument `index` as an entity reference.
    pub fn ent<'a>(&self, spf: &'a Spf, index: usize) -> Option<&'a Ent> {
        self.ref_id(index).and_then(|id| spf.get(id))
    }
}

/// A parsed exchange file: schema name plus the id-indexed entity table.
#[derive(Debug, Default)]
pub struct Spf {
    schema: String,
    ents: Vec<Ent>,
    index: std::collections::HashMap<u64, usize>,
}

impl Spf {
    pub fn schema(&self) -> &str {
        &self.schema
    }

    pub fn get(&self, id: u64) -> Option<&Ent> {
        self.index.get(&id).copied().and_then(|i| self.ents.get(i))
    }

    pub fn len(&self) -> usize {
        self.ents.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ents.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Ent> {
        self.ents.iter()
    }

    pub fn of_type<'a>(&'a self, ty: &str) -> impl Iterator<Item = &'a Ent> + 'a {
        let ty = ty.to_string();
        self.ents
            .iter()
            .filter(move |e| e.types.iter().any(|t| t.as_ref() == ty.as_str()))
    }

    pub fn first_of_type(&self, ty: &str) -> Option<&Ent> {
        self.of_type(ty).next()
    }

    /// Parse an ISO-10303-21 exchange structure. Tolerant of the extensions
    /// real files carry (extra whitespace, comments, multiple DATA sections);
    /// fails only when the file is not an exchange structure at all.
    pub fn parse(bytes: &[u8]) -> Result<Spf, String> {
        Self::parse_with_progress(bytes, None)
    }

    /// Same parse with an optional byte-offset progress callback. The
    /// callback fires at most once per ~1 MiB of input so huge files feed a
    /// live progress bar without burning time on the reporting itself.
    pub fn parse_with_progress(
        bytes: &[u8],
        progress: Option<&dyn Fn(usize, usize)>,
    ) -> Result<Spf, String> {
        let mut parser = Parser {
            b: bytes,
            pos: 0,
            type_cache: std::collections::HashMap::new(),
            reported: 0,
            progress,
        };
        parser.parse_file()
    }
}

struct Parser<'a> {
    b: &'a [u8],
    pos: usize,
    /// Interning cache for entity type names (see `Ent::types`).
    type_cache: std::collections::HashMap<String, std::sync::Arc<str>>,
    /// Byte offset last reported through `progress`.
    reported: usize,
    progress: Option<&'a dyn Fn(usize, usize)>,
}

impl<'a> Parser<'a> {
    fn intern_range(&mut self, start: usize, end: usize) -> std::sync::Arc<str> {
        let b: &'a [u8] = self.b;
        let bytes = &b[start..end.min(b.len())];
        match std::str::from_utf8(bytes) {
            Ok(text) => {
                if let Some(shared) = self.type_cache.get(text) {
                    return shared.clone();
                }
                let shared: std::sync::Arc<str> = std::sync::Arc::from(text);
                self.type_cache.insert(text.to_string(), shared.clone());
                shared
            }
            Err(_) => std::sync::Arc::from(String::from_utf8_lossy(bytes).as_ref()),
        }
    }
    fn peek(&self) -> Option<u8> {
        self.b.get(self.pos).copied()
    }

    fn bump(&mut self) -> Option<u8> {
        let ch = self.peek()?;
        self.pos += 1;
        Some(ch)
    }

    fn skip_ws(&mut self) {
        while let Some(&ch) = self.b.get(self.pos) {
            if ch == b'/' && self.b.get(self.pos + 1) == Some(&b'*') {
                self.pos += 2;
                while self.pos < self.b.len() {
                    if self.b[self.pos] == b'*' && self.b.get(self.pos + 1) == Some(&b'/') {
                        self.pos += 2;
                        break;
                    }
                    self.pos += 1;
                }
            } else if ch.is_ascii_whitespace() {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    fn eat(&mut self, token: &[u8]) -> bool {
        self.skip_ws();
        if self.b[self.pos..].starts_with(token) {
            self.pos += token.len();
            true
        } else {
            false
        }
    }

    fn expect(&mut self, token: &[u8]) -> Result<(), String> {
        if self.eat(token) {
            Ok(())
        } else {
            Err(format!(
                "expected {:?} at byte {} (found {:?})",
                String::from_utf8_lossy(token),
                self.pos,
                String::from_utf8_lossy(&self.b[self.pos..self.pos.min(self.b.len()) + 24]),
            ))
        }
    }

    /// Read an upper-case keyword such as `DATA` or `IFCWALL`.
    fn keyword(&mut self) -> Option<String> {
        self.skip_ws();
        let start = self.pos;
        while self
            .b
            .get(self.pos)
            .is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'_')
        {
            self.pos += 1;
        }
        if self.pos == start {
            return None;
        }
        Some(String::from_utf8_lossy(&self.b[start..self.pos]).into_owned())
    }

    fn parse_file(&mut self) -> Result<Spf, String> {
        let mut spf = Spf::default();
        self.expect(b"ISO-10303-21")?;
        self.expect(b";")?;
        let mut sections = 0u32;
        loop {
            self.skip_ws();
            let Some(ch) = self.peek() else {
                return Err("unexpected end of file".into());
            };
            if ch == b'E' {
                let save = self.pos;
                if self.eat(b"END-ISO-10303-21") {
                    self.expect(b";")?;
                    return Ok(spf);
                }
                self.pos = save;
            }
            match self.keyword().as_deref() {
                Some("HEADER") => {
                    self.parse_header(&mut spf)?;
                    sections += 1;
                }
                Some("DATA") => {
                    // DATA; or DATA('anchor-ish params') — the params are not used.
                    self.skip_ws();
                    if self.peek() == Some(b'(') {
                        self.skip_parenthesized()?;
                    }
                    self.expect(b";")?;
                    self.parse_data(&mut spf)?;
                    sections += 1;
                }
                Some(other) => {
                    // Unknown top-level token: skip through the next ';' so one
                    // odd record cannot abort the whole import.
                    let _ = other;
                    self.skip_to_semicolon();
                }
                None => {
                    self.bump();
                    sections += 1; // guard against pathological input
                }
            }
            if sections > 64 {
                return Err("exchange structure has too many sections".into());
            }
        }
    }

    /// Header: record FILE_SCHEMA's schema name, ignore the rest.
    fn parse_header(&mut self, spf: &mut Spf) -> Result<(), String> {
        loop {
            self.skip_ws();
            if self.eat(b"ENDSEC") {
                self.expect(b";")?;
                return Ok(());
            }
            let Some(name) = self.keyword() else {
                self.bump();
                continue;
            };
            if name == "FILE_SCHEMA" {
                if let Ok(values) = self.parse_args() {
                    spf.schema = values
                        .first()
                        .and_then(Val::as_list)
                        .and_then(|l| l.first())
                        .and_then(Val::as_str)
                        .unwrap_or_default()
                        .to_string();
                }
            } else {
                self.skip_to_semicolon();
            }
        }
    }

    fn parse_data(&mut self, spf: &mut Spf) -> Result<(), String> {
        loop {
            self.skip_ws();
            match self.peek() {
                None => return Err("unterminated DATA section".into()),
                Some(b'E') => {
                    if self.b[self.pos..].starts_with(b"ENDSEC") {
                        self.expect(b"ENDSEC")?;
                        self.expect(b";")?;
                        return Ok(());
                    }
                    self.skip_to_semicolon();
                }
                Some(b'#') => {
                    let ent = self.parse_entity()?;
                    if let Some(ent) = ent {
                        spf.index.entry(ent.id).or_insert(spf.ents.len());
                        spf.ents.push(ent);
                    }
                    if let Some(report) = self.progress {
                        if self.pos - self.reported >= (1 << 20) {
                            self.reported = self.pos;
                            report(self.pos, self.b.len());
                        }
                    }
                }
                _ => {
                    self.skip_to_semicolon();
                }
            }
        }
    }

    /// `#12 = NAME(args);` or `#12 = (NAME(args)NAME(args));`
    fn parse_entity(&mut self) -> Result<Option<Ent>, String> {
        self.expect(b"#")?;
        let id = self.parse_int()? as u64;
        self.expect(b"=")?;
        self.skip_ws();
        let mut types = Vec::new();
        let mut args = Vec::new();
        if self.peek() == Some(b'(') {
            // Complex entity: a run of TYPE(args) groups.
            loop {
                self.skip_ws();
                if self.peek() == Some(b')') {
                    self.bump();
                    break;
                }
                self.skip_ws();
                let start = self.pos;
                if self.keyword().is_some() {
                    types.push(self.intern_range(start, self.pos));
                    if self.peek() == Some(b'(') {
                        args.extend(self.parse_args()?);
                    }
                } else {
                    self.bump();
                }
            }
        } else {
            self.skip_ws();
            let start = self.pos;
            let name = self
                .keyword()
                .ok_or_else(|| format!("entity #{id}: missing type name"))?;
            let _ = name;
            types.push(self.intern_range(start, self.pos));
            self.skip_ws();
            if self.peek() == Some(b'(') {
                args = self.parse_args()?;
            }
        }
        self.expect(b";")?;
        Ok(Some(Ent { id, types, args }))
    }

    /// Consume `( … )` with arbitrary nesting and return the top-level values.
    fn parse_args(&mut self) -> Result<Vec<Val>, String> {
        self.skip_ws();
        if self.peek() != Some(b'(') {
            return Err(format!("expected '(' at byte {}", self.pos));
        }
        self.bump();
        let mut values = Vec::new();
        loop {
            self.skip_ws();
            match self.peek() {
                None => return Err("unterminated argument list".into()),
                Some(b')') => {
                    self.bump();
                    return Ok(values);
                }
                Some(b',') => {
                    self.bump();
                }
                _ => values.push(self.parse_value()?),
            }
        }
    }

    fn parse_value(&mut self) -> Result<Val, String> {
        self.skip_ws();
        match self.peek() {
            None => Err("unexpected end inside a value".into()),
            Some(b'#') => {
                self.bump();
                Ok(Val::Ref(self.parse_int()? as u64))
            }
            Some(b'$') => {
                self.bump();
                Ok(Val::Unset)
            }
            Some(b'*') => {
                self.bump();
                Ok(Val::Star)
            }
            Some(b'(') => Ok(Val::List(self.parse_args()?)),
            Some(b'\'') => Ok(Val::Str(self.parse_string()?)),
            Some(b'.') => {
                self.bump();
                let start = self.pos;
                while self.peek().is_some_and(|c| c != b'.') {
                    self.bump();
                }
                let text = String::from_utf8_lossy(&self.b[start..self.pos]).into_owned();
                if self.peek() == Some(b'.') {
                    self.bump();
                }
                Ok(Val::Enum(text))
            }
            Some(c) if c == b'"' => {
                // Binary literal "0A1F" — consumed, not interpreted.
                self.bump();
                while self.peek().is_some_and(|c| c != b'"') {
                    self.bump();
                }
                self.bump();
                Ok(Val::Unset)
            }
            Some(c) if c.is_ascii_digit() || c == b'-' || c == b'+' => self.parse_number(),
            Some(c) if c.is_ascii_alphabetic() || c == b'_' => {
                let name = self.keyword().unwrap_or_default();
                self.skip_ws();
                if self.peek() == Some(b'(') {
                    // Typed value: exactly one payload value inside.
                    let inner = self.parse_args()?;
                    let payload = inner
                        .into_iter()
                        .next()
                        .ok_or_else(|| format!("typed value {name} has no payload"))?;
                    Ok(Val::Typed(name, Box::new(payload)))
                } else {
                    // Bare keyword inside a value (rare); keep as an enum-like.
                    Ok(Val::Enum(name))
                }
            }
            Some(c) => Err(format!("unexpected character {:?} in a value at byte {}", c as char, self.pos)),
        }
    }

    fn parse_number(&mut self) -> Result<Val, String> {
        self.skip_ws();
        let start = self.pos;
        if matches!(self.peek(), Some(b'-') | Some(b'+')) {
            self.bump();
        }
        let mut is_real = false;
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() {
                self.bump();
            } else if c == b'.' {
                is_real = true;
                self.bump();
            } else if c == b'e' || c == b'E' {
                is_real = true;
                self.bump();
                if matches!(self.peek(), Some(b'-') | Some(b'+')) {
                    self.bump();
                }
            } else {
                break;
            }
        }
        let text = std::str::from_utf8(&self.b[start..self.pos]).unwrap_or("");
        if is_real {
            text.parse::<f64>()
                .map(Val::Num)
                .map_err(|e| format!("bad real {text:?}: {e}"))
        } else {
            text.parse::<i64>()
                .map(Val::Int)
                .map_err(|e| format!("bad integer {text:?}: {e}"))
        }
    }

    fn parse_int(&mut self) -> Result<i64, String> {
        match self.parse_number()? {
            Val::Int(i) => Ok(i),
            other => Err(format!("expected an integer, found {other:?}")),
        }
    }

    /// `'…'` with STEP escapes decoded. See ISO 10303-21 §7.3 / web-ifc's
    /// string handling: `''`→`'`, `\\`→`\`, `\X2\hhhh…\X0\`→UTF-16BE,
    /// `\X\hh`→single-byte code point, `\S\c`→symbol, `\P\`→layout directive.
    fn parse_string(&mut self) -> Result<String, String> {
        self.expect(b"'")?;
        let mut out = String::new();
        loop {
            let Some(c) = self.bump() else {
                return Err("unterminated string".into());
            };
            if c != b'\'' && c != b'\\' {
                out.push(c as char);
                continue;
            }
            let Some(next) = self.peek() else {
                return Err("unterminated string".into());
            };
            match (c, next) {
                (b'\'', b'\'') => {
                    self.bump();
                    out.push('\'');
                }
                (b'\'', _) => break, // closing quote
                (b'\\', b'\\') => {
                    self.bump();
                    out.push('\\');
                }
                (b'\\', b'X') | (b'\\', b'x') => {
                    self.bump();
                    let kind = self.bump().unwrap_or(b'0');
                    self.expect(b"\\")?;
                    if kind == b'2' || kind == b'4' {
                        // \X2\hhhh…\X0\ — hex-encoded UTF-16BE payload.
                        let mut payload = Vec::new();
                        while let Some(ch) = self.peek() {
                            if ch == b'\\' {
                                break;
                            }
                            payload.push(ch);
                            self.bump();
                        }
                        if payload.len() % 2 != 0 {
                            payload.pop();
                        }
                        let mut bytes = Vec::with_capacity(payload.len() / 2);
                        let digit = |c: u8| (c as char).to_digit(16).map(|d| d as u8);
                        for pair in payload.chunks_exact(2) {
                            match (digit(pair[0]), digit(pair[1])) {
                                (Some(h), Some(l)) => bytes.push(h * 16 + l),
                                _ => break,
                            }
                        }
                        out.push_str(&decode_utf16be(&bytes));
                        self.expect(b"\\X0\\")?;
                    } else {
                        // \X\hh — two hex digits, single-byte code point.
                        let hi = self.bump();
                        let lo = self.bump();
                        let digit = |c: u8| (c as char).to_digit(16).map(|d| d as u8);
                        if let (Some(hv), Some(lv)) = (digit(hi.unwrap_or(b'0')), digit(lo.unwrap_or(b'0'))) {
                            let byte_val = hv * 16 + lv;
                            if (0x20..0x7f).contains(&byte_val) {
                                out.push(byte_val as char);
                            } else {
                                out.push(char::REPLACEMENT_CHARACTER);
                            }
                        }
                    }
                }
                (b'\\', b'S') | (b'\\', b's') => {
                    self.bump();
                    self.bump(); // the special character itself
                    self.expect(b"\\")?;
                }
                (b'\\', b'P') | (b'\\', b'p') => {
                    self.bump(); // layout directive takes no payload
                    self.expect(b"\\")?;
                }
                (b'\\', b'F') | (b'\\', b'f') => {
                    // \F\ font directive — skip its argument run.
                    self.bump();
                    while self.peek().is_some_and(|ch| ch != b'\\') {
                        self.bump();
                    }
                    self.bump();
                }
                _ => out.push(c as char),
            }
        }
        Ok(out)
    }

    fn skip_to_semicolon(&mut self) {
        while let Some(c) = self.bump() {
            if c == b'\'' {
                // A string can contain ';'.
                while let Some(c) = self.bump() {
                    if c == b'\'' {
                        if self.peek() == Some(b'\'') {
                            self.bump();
                        } else {
                            break;
                        }
                    }
                }
            } else if c == b';' {
                return;
            }
        }
    }

    fn skip_parenthesized(&mut self) -> Result<(), String> {
        let mut depth = 0usize;
        loop {
            let Some(c) = self.bump() else {
                return Err("unbalanced parentheses".into());
            };
            if c == b'(' {
                depth += 1;
            } else if c == b')' {
                depth -= 1;
                if depth == 0 {
                    return Ok(());
                }
            } else if c == b'\'' {
                while let Some(c) = self.bump() {
                    if c == b'\'' {
                        if self.peek() == Some(b'\'') {
                            self.bump();
                        } else {
                            break;
                        }
                    }
                }
            }
        }
    }
}

fn decode_utf16be(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|p| u16::from_be_bytes([p[0], p[1]]))
        .collect();
    String::from_utf16_lossy(&units)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_minimal_ifc_exchange() {
        let src = b"ISO-10303-21;\nHEADER;\nFILE_DESCRIPTION(('x'),'2;1');\nFILE_NAME('a.ifc','2024',('a'),('b'),'c','d','');\nFILE_SCHEMA(('IFC4'));\nENDSEC;\nDATA;\n#1=IFCPROJECT('guid',$,'P',$,$,$,(#2),$,$);\n#2=IFCGEOMETRICREPRESENTATIONCONTEXT($,'Model',3,1.E-05,#3,$);\n#3=IFCAXIS2PLACEMENT3D(#4,$,$);\n#4=IFCCARTESIANPOINT((0.,0.,0.));\nENDSEC;\nEND-ISO-10303-21;\n";
        let spf = Spf::parse(src).expect("parse");
        assert_eq!(spf.schema(), "IFC4");
        let project = spf.first_of_type("IFCPROJECT").expect("project");
        assert_eq!(project.str(2), Some("P"));
        // RepresentationContexts is a list at slot 6.
        let ctx_id = project
            .list(6)
            .and_then(|l| l.first())
            .and_then(Val::as_ref)
            .expect("context ref");
        let ctx = spf.get(ctx_id).expect("contexts");
        assert!(ctx.is("IFCGEOMETRICREPRESENTATIONCONTEXT"));
        assert_eq!(ctx.int(2), Some(3));
        let origin = ctx.ent(&spf, 4).and_then(|p| p.ent(&spf, 0)).expect("origin");
        assert_eq!(origin.ty(), "IFCCARTESIANPOINT");
        assert_eq!(origin.list(0).unwrap().len(), 3);
    }

    #[test]
    fn parses_complex_entities() {
        let src = b"ISO-10303-21;\nDATA;\n#5=(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.));\nENDSEC;\nEND-ISO-10303-21;\n";
        let spf = Spf::parse(src).expect("parse");
        let unit = spf.first_of_type("SI_UNIT").expect("complex ent");
        assert!(unit.is("LENGTH_UNIT") && unit.is("NAMED_UNIT"));
        // Concatenated complex args: NAMED_UNIT(*) then SI_UNIT(prefix, name).
        assert_eq!(unit.get(1).and_then(Val::as_enum), Some("MILLI"));
        assert_eq!(unit.get(2).and_then(Val::as_enum), Some("METRE"));
    }

    #[test]
    fn decodes_unicode_and_escapes_in_strings() {
        let src = "ISO-10303-21;\nDATA;\n#1=IFCTEXT('C\\X2\\01AF\\X0\\ qu''an');\nENDSEC;\nEND-ISO-10303-21;\n".as_bytes();
        let spf = Spf::parse(src).expect("parse");
        let text = spf.first_of_type("IFCTEXT").expect("text");
        // \X2\01AF\X0\ = U+01AF (Ư); '' inside the literal is a quote.
        let value = text.str(0).expect("string value");
        assert!(value.contains('\u{01AF}'), "got {value:?}");
        assert!(value.contains('\''), "got {value:?}");
    }

    #[test]
    fn typed_values_keep_their_payload() {
        let src = b"ISO-10303-21;\nDATA;\n#1=IFCPROPERTYSINGLEVALUE('Fire rating',IFCLABEL('REI 60'),$,$);\n#2=IFCPROPERTYSINGLEVALUE('Depth',IFCLENGTHMEASURE(240.),$,$);\nENDSEC;\nEND-ISO-10303-21;\n";
        let spf = Spf::parse(src).expect("parse");
        let a = spf.get(1).unwrap();
        let label = a.get(1).expect("nominal");
        assert_eq!(label.as_str(), Some("REI 60"));
        let b = spf.get(2).unwrap();
        assert_eq!(b.get(1).and_then(Val::as_f64), Some(240.0));
    }

    #[test]
    fn tolerates_comments_and_multiple_data_sections() {
        let src = b"ISO-10303-21;\nHEADER;\nFILE_SCHEMA(('AP214IS'));\nENDSEC;\n/* comment ; with stuff */\nDATA;\n#1=DUMMY('a;b');\nENDSEC;\nDATA;\n#2=DUMMY(2);\nENDSEC;\nEND-ISO-10303-21;\n";
        let spf = Spf::parse(src).expect("parse");
        assert_eq!(spf.schema(), "AP214IS");
        assert_eq!(spf.len(), 2);
        assert_eq!(spf.get(1).unwrap().str(0), Some("a;b"));
    }
}
