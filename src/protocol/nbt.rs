//! Full NBT in "network" format (unnamed root, 1.20.2+): reading, writing and skipping.

use super::buf::{Reader, Writer};
use anyhow::{bail, Result};

#[derive(Clone, Debug, PartialEq)]
pub enum Nbt {
    Byte(i8),
    Short(i16),
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    ByteArray(Vec<i8>),
    String(String),
    List(Vec<Nbt>),
    Compound(Vec<(String, Nbt)>),
    IntArray(Vec<i32>),
    LongArray(Vec<i64>),
}

impl Nbt {
    fn tag(&self) -> u8 {
        match self {
            Nbt::Byte(_) => 1,
            Nbt::Short(_) => 2,
            Nbt::Int(_) => 3,
            Nbt::Long(_) => 4,
            Nbt::Float(_) => 5,
            Nbt::Double(_) => 6,
            Nbt::ByteArray(_) => 7,
            Nbt::String(_) => 8,
            Nbt::List(_) => 9,
            Nbt::Compound(_) => 10,
            Nbt::IntArray(_) => 11,
            Nbt::LongArray(_) => 12,
        }
    }

    fn write_str(out: &mut Vec<u8>, s: &str) {
        out.u16(s.len() as u16);
        out.extend_from_slice(s.as_bytes());
    }

    fn write_payload(&self, out: &mut Vec<u8>) {
        match self {
            Nbt::Byte(v) => {
                out.i8(*v);
            }
            Nbt::Short(v) => {
                out.i16(*v);
            }
            Nbt::Int(v) => {
                out.i32(*v);
            }
            Nbt::Long(v) => {
                out.i64(*v);
            }
            Nbt::Float(v) => {
                out.f32(*v);
            }
            Nbt::Double(v) => {
                out.f64(*v);
            }
            Nbt::ByteArray(v) => {
                out.i32(v.len() as i32);
                for x in v {
                    out.i8(*x);
                }
            }
            Nbt::String(s) => Self::write_str(out, s),
            Nbt::List(items) => {
                out.u8(items.first().map_or(0, Nbt::tag));
                out.i32(items.len() as i32);
                for i in items {
                    i.write_payload(out);
                }
            }
            Nbt::Compound(fields) => {
                for (k, v) in fields {
                    out.u8(v.tag());
                    Self::write_str(out, k);
                    v.write_payload(out);
                }
                out.u8(0);
            }
            Nbt::IntArray(v) => {
                out.i32(v.len() as i32);
                for x in v {
                    out.i32(*x);
                }
            }
            Nbt::LongArray(v) => {
                out.i32(v.len() as i32);
                for x in v {
                    out.i64(*x);
                }
            }
        }
    }

    /// Writes as network NBT: type + payload, without root name.
    pub fn write_network(&self, out: &mut Vec<u8>) {
        out.u8(self.tag());
        self.write_payload(out);
    }

    pub fn to_network(&self) -> Vec<u8> {
        let mut v = Vec::new();
        self.write_network(&mut v);
        v
    }

    /// Reads network NBT. `None` if it is the empty tag (0).
    pub fn read_network(r: &mut Reader) -> Result<Option<Nbt>> {
        let tag = r.u8()?;
        if tag == 0 {
            return Ok(None);
        }
        Ok(Some(read_payload(r, tag, 0)?))
    }

    /// Writes in disk format: type + root name + payload (level.dat, Anvil chunks).
    pub fn write_named(&self, name: &str, out: &mut Vec<u8>) {
        out.u8(self.tag());
        Self::write_str(out, name);
        self.write_payload(out);
    }

    /// Reads the disk format (named root).
    pub fn read_named(r: &mut Reader) -> Result<(String, Nbt)> {
        let tag = r.u8()?;
        if tag == 0 {
            bail!("NBT vacío");
        }
        let name = read_str(r)?;
        Ok((name, read_payload(r, tag, 0)?))
    }

    // ---------- convenience constructors ----------

    pub fn compound() -> Nbt {
        Nbt::Compound(Vec::new())
    }

    /// Adds a key (for building compounds by chaining).
    pub fn with(mut self, key: &str, value: Nbt) -> Nbt {
        self.set(key, value);
        self
    }

    pub fn str(s: &str) -> Nbt {
        Nbt::String(s.to_string())
    }

    pub fn get_str(&self, key: &str) -> Option<&str> {
        self.get(key).and_then(Nbt::as_str)
    }

    pub fn get_i64(&self, key: &str) -> Option<i64> {
        self.get(key).and_then(Nbt::as_i64)
    }

    pub fn from_network(bytes: &[u8]) -> Option<Nbt> {
        Nbt::read_network(&mut Reader::new(bytes)).ok().flatten()
    }

    // ---------- convenient compound access ----------

    pub fn get(&self, key: &str) -> Option<&Nbt> {
        match self {
            Nbt::Compound(f) => f.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn get_mut(&mut self, key: &str) -> Option<&mut Nbt> {
        match self {
            Nbt::Compound(f) => f.iter_mut().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    /// Inserts or replaces a key in a compound.
    pub fn set(&mut self, key: &str, value: Nbt) {
        if let Nbt::Compound(f) = self {
            match f.iter_mut().find(|(k, _)| k == key) {
                Some((_, v)) => *v = value,
                None => f.push((key.to_string(), value)),
            }
        }
    }

    pub fn remove(&mut self, key: &str) {
        if let Nbt::Compound(f) = self {
            f.retain(|(k, _)| k != key);
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match *self {
            Nbt::Byte(v) => Some(v as i64),
            Nbt::Short(v) => Some(v as i64),
            Nbt::Int(v) => Some(v as i64),
            Nbt::Long(v) => Some(v),
            Nbt::Float(v) => Some(v as i64),
            Nbt::Double(v) => Some(v as i64),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Nbt::String(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_list(&self) -> &[Nbt] {
        match self {
            Nbt::List(l) => l,
            _ => &[],
        }
    }

    pub fn is_empty_compound(&self) -> bool {
        matches!(self, Nbt::Compound(f) if f.is_empty())
    }
}

fn len(r: &mut Reader) -> Result<usize> {
    let n = r.i32()?;
    if !(0..=(1 << 22)).contains(&n) {
        bail!("longitud NBT inválida");
    }
    Ok(n as usize)
}

fn read_str(r: &mut Reader) -> Result<String> {
    let n = r.u16()? as usize;
    Ok(String::from_utf8_lossy(r.bytes(n)?).into_owned())
}

fn read_payload(r: &mut Reader, tag: u8, depth: u32) -> Result<Nbt> {
    if depth > 512 {
        bail!("NBT demasiado profundo");
    }
    Ok(match tag {
        1 => Nbt::Byte(r.i8()?),
        2 => Nbt::Short(r.i16()?),
        3 => Nbt::Int(r.i32()?),
        4 => Nbt::Long(r.i64()?),
        5 => Nbt::Float(r.f32()?),
        6 => Nbt::Double(r.f64()?),
        7 => {
            let n = len(r)?;
            Nbt::ByteArray(r.bytes(n)?.iter().map(|&b| b as i8).collect())
        }
        8 => Nbt::String(read_str(r)?),
        9 => {
            let inner = r.u8()?;
            let n = len(r)?;
            let mut v = Vec::with_capacity(n.min(1024));
            for _ in 0..n {
                v.push(read_payload(r, inner, depth + 1)?);
            }
            Nbt::List(v)
        }
        10 => {
            let mut f = Vec::new();
            loop {
                let t = r.u8()?;
                if t == 0 {
                    break;
                }
                let k = read_str(r)?;
                f.push((k, read_payload(r, t, depth + 1)?));
            }
            Nbt::Compound(f)
        }
        11 => {
            let n = len(r)?;
            Nbt::IntArray((0..n).map(|_| r.i32()).collect::<Result<_>>()?)
        }
        12 => {
            let n = len(r)?;
            Nbt::LongArray((0..n).map(|_| r.i64()).collect::<Result<_>>()?)
        }
        t => bail!("tag NBT desconocido {t}"),
    })
}

/// Reads text NBT (SNBT, as used by commands): `{Potion:"minecraft:healing",Count:3b}`.
pub fn parse_snbt(text: &str) -> Result<Nbt> {
    struct P<'a> {
        s: &'a [u8],
        i: usize,
    }
    impl P<'_> {
        fn ws(&mut self) {
            while self.i < self.s.len() && self.s[self.i].is_ascii_whitespace() {
                self.i += 1;
            }
        }
        fn peek(&mut self) -> Option<u8> {
            self.ws();
            self.s.get(self.i).copied()
        }
        fn eat(&mut self, c: u8) -> Result<()> {
            if self.peek() != Some(c) {
                bail!("se esperaba '{}' en la posición {}", c as char, self.i);
            }
            self.i += 1;
            Ok(())
        }
        fn quoted(&mut self) -> Result<String> {
            let q = self.s[self.i];
            self.i += 1;
            let mut out = Vec::new();
            while self.i < self.s.len() {
                let c = self.s[self.i];
                self.i += 1;
                if c == b'\\' && self.i < self.s.len() {
                    out.push(self.s[self.i]);
                    self.i += 1;
                } else if c == q {
                    return Ok(String::from_utf8_lossy(&out).into_owned());
                } else {
                    out.push(c);
                }
            }
            bail!("texto sin cerrar")
        }
        fn bare(&mut self) -> String {
            self.word(b"_-.+:")
        }
        fn word(&mut self, extra: &[u8]) -> String {
            self.ws();
            let start = self.i;
            while self.i < self.s.len() && (self.s[self.i].is_ascii_alphanumeric() || extra.contains(&self.s[self.i])) {
                self.i += 1;
            }
            String::from_utf8_lossy(&self.s[start..self.i]).into_owned()
        }
        fn key(&mut self) -> Result<String> {
            match self.peek() {
                Some(b'"') | Some(b'\'') => self.quoted(),
                // Unquoted keys do not contain ':' (it separates key and value).
                _ => Ok(self.word(b"_-.+")),
            }
        }
        fn value(&mut self, depth: u32) -> Result<Nbt> {
            if depth > 64 {
                bail!("SNBT demasiado profundo");
            }
            match self.peek() {
                Some(b'{') => {
                    self.i += 1;
                    let mut f = Vec::new();
                    if self.peek() == Some(b'}') {
                        self.i += 1;
                        return Ok(Nbt::Compound(f));
                    }
                    loop {
                        let k = self.key()?;
                        self.eat(b':')?;
                        f.push((k, self.value(depth + 1)?));
                        match self.peek() {
                            Some(b',') => self.i += 1,
                            Some(b'}') => {
                                self.i += 1;
                                return Ok(Nbt::Compound(f));
                            }
                            _ => bail!("se esperaba ',' o '}}'"),
                        }
                    }
                }
                Some(b'[') => {
                    self.i += 1;
                    // Typed arrays: [B;1b,2b] [I;1,2] [L;1l].
                    let save = self.i;
                    let prefix = self.bare();
                    let typed = matches!(prefix.as_str(), "B" | "I" | "L") && self.peek() == Some(b';');
                    if typed {
                        self.i += 1;
                    } else {
                        self.i = save;
                    }
                    let mut items = Vec::new();
                    if self.peek() == Some(b']') {
                        self.i += 1;
                    } else {
                        loop {
                            items.push(self.value(depth + 1)?);
                            match self.peek() {
                                Some(b',') => self.i += 1,
                                Some(b']') => {
                                    self.i += 1;
                                    break;
                                }
                                _ => bail!("se esperaba ',' o ']'"),
                            }
                        }
                    }
                    Ok(match prefix.as_str() {
                        "B" if typed => Nbt::ByteArray(items.iter().filter_map(Nbt::as_i64).map(|v| v as i8).collect()),
                        "I" if typed => Nbt::IntArray(items.iter().filter_map(Nbt::as_i64).map(|v| v as i32).collect()),
                        "L" if typed => Nbt::LongArray(items.iter().filter_map(Nbt::as_i64).collect()),
                        _ => Nbt::List(items),
                    })
                }
                Some(b'"') | Some(b'\'') => Ok(Nbt::String(self.quoted()?)),
                Some(_) => {
                    let w = self.bare();
                    if w.is_empty() {
                        bail!("valor vacío en la posición {}", self.i);
                    }
                    let (num, suffix) = w.split_at(w.len() - 1);
                    let parsed = match suffix.to_ascii_lowercase().as_str() {
                        "b" => num.parse::<i8>().ok().map(Nbt::Byte),
                        "s" => num.parse::<i16>().ok().map(Nbt::Short),
                        "l" => num.parse::<i64>().ok().map(Nbt::Long),
                        "f" => num.parse::<f32>().ok().map(Nbt::Float),
                        "d" => num.parse::<f64>().ok().map(Nbt::Double),
                        _ => None,
                    };
                    Ok(match (parsed, w.as_str()) {
                        (Some(v), _) => v,
                        (None, "true") => Nbt::Byte(1),
                        (None, "false") => Nbt::Byte(0),
                        _ => match w.parse::<i32>() {
                            Ok(v) => Nbt::Int(v),
                            Err(_) => match w.parse::<f64>() {
                                Ok(v) if w.contains('.') => Nbt::Double(v),
                                _ => Nbt::String(w),
                            },
                        },
                    })
                }
                None => bail!("SNBT incompleto"),
            }
        }
    }
    let mut p = P { s: text.as_bytes(), i: 0 };
    let v = p.value(0)?;
    if p.peek().is_some() {
        bail!("sobra texto tras el SNBT");
    }
    Ok(v)
}

/// Skips a network NBT (optional: tag 0 = empty) without keeping it.
pub fn skip_network(r: &mut Reader) -> Result<()> {
    Nbt::read_network(r).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let n = Nbt::Compound(vec![
            ("Damage".into(), Nbt::Int(12)),
            ("Enchantments".into(), Nbt::List(vec![Nbt::Compound(vec![("id".into(), Nbt::String("minecraft:sharpness".into())), ("lvl".into(), Nbt::Short(3))])])),
            ("f".into(), Nbt::Float(1.5)),
            ("a".into(), Nbt::IntArray(vec![1, 2])),
        ]);
        let bytes = n.to_network();
        assert_eq!(Nbt::from_network(&bytes), Some(n));
    }

    #[test]
    fn snbt() {
        let n = parse_snbt(r#"{Potion:"minecraft:healing",id:minecraft:stone,Damage:3,x:1.5f,list:[1b,2b],arr:[I;1,2],name:'{"text":"a"}',nested:{ok:true}}"#).unwrap();
        assert_eq!(n.get_str("Potion"), Some("minecraft:healing"));
        assert_eq!(n.get_str("id"), Some("minecraft:stone"));
        assert_eq!(n.get("Damage"), Some(&Nbt::Int(3)));
        assert_eq!(n.get("x"), Some(&Nbt::Float(1.5)));
        assert_eq!(n.get("list"), Some(&Nbt::List(vec![Nbt::Byte(1), Nbt::Byte(2)])));
        assert_eq!(n.get("arr"), Some(&Nbt::IntArray(vec![1, 2])));
        assert_eq!(n.get_str("name"), Some(r#"{"text":"a"}"#));
        assert_eq!(n.get("nested").unwrap().get("ok"), Some(&Nbt::Byte(1)));
        assert!(parse_snbt("{a:").is_err());
    }
}
