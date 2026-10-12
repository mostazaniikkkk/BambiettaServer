//! Minecraft protocol primitives: reading and writing basic types.

use anyhow::{bail, ensure, Result};

/// Read cursor over a packet body.
pub struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    pub fn remaining(&self) -> usize {
        self.buf.len() - self.pos
    }

    pub fn bytes(&mut self, n: usize) -> Result<&'a [u8]> {
        ensure!(self.remaining() >= n, "paquete truncado");
        let s = &self.buf[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }

    pub fn position_in_buffer(&self) -> usize {
        self.pos
    }

    /// Bytes read from `start` up to the current position.
    pub fn slice_from(&self, start: usize) -> &'a [u8] {
        &self.buf[start..self.pos]
    }

    pub fn rest(&mut self) -> &'a [u8] {
        let s = &self.buf[self.pos..];
        self.pos = self.buf.len();
        s
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N]> {
        Ok(self.bytes(N)?.try_into().unwrap())
    }

    pub fn u8(&mut self) -> Result<u8> {
        Ok(self.array::<1>()?[0])
    }
    pub fn i8(&mut self) -> Result<i8> {
        Ok(self.u8()? as i8)
    }
    pub fn bool(&mut self) -> Result<bool> {
        Ok(self.u8()? != 0)
    }
    pub fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_be_bytes(self.array()?))
    }
    pub fn i16(&mut self) -> Result<i16> {
        Ok(i16::from_be_bytes(self.array()?))
    }
    pub fn i32(&mut self) -> Result<i32> {
        Ok(i32::from_be_bytes(self.array()?))
    }
    pub fn i64(&mut self) -> Result<i64> {
        Ok(i64::from_be_bytes(self.array()?))
    }
    pub fn f32(&mut self) -> Result<f32> {
        Ok(f32::from_be_bytes(self.array()?))
    }
    pub fn f64(&mut self) -> Result<f64> {
        Ok(f64::from_be_bytes(self.array()?))
    }
    pub fn uuid(&mut self) -> Result<u128> {
        Ok(u128::from_be_bytes(self.array()?))
    }

    pub fn varint(&mut self) -> Result<i32> {
        let mut value: u32 = 0;
        for i in 0..5 {
            let b = self.u8()?;
            value |= ((b & 0x7f) as u32) << (7 * i);
            if b & 0x80 == 0 {
                return Ok(value as i32);
            }
        }
        bail!("varint demasiado largo")
    }

    pub fn string(&mut self, max_chars: usize) -> Result<String> {
        let len = self.varint()?;
        ensure!(len >= 0 && len as usize <= max_chars * 4, "string demasiado largo");
        let s = std::str::from_utf8(self.bytes(len as usize)?)?;
        ensure!(s.chars().count() <= max_chars, "string demasiado largo");
        Ok(s.to_owned())
    }

    pub fn position(&mut self) -> Result<BlockPos> {
        Ok(BlockPos::from_packed(self.i64()?))
    }
}

/// Decodes a VarInt from the start of `buf`. Returns `None` if bytes are missing.
pub fn peek_varint(buf: &[u8]) -> Result<Option<(i32, usize)>> {
    let mut value: u32 = 0;
    for i in 0..5 {
        let Some(&b) = buf.get(i) else { return Ok(None) };
        value |= ((b & 0x7f) as u32) << (7 * i);
        if b & 0x80 == 0 {
            return Ok(Some((value as i32, i + 1)));
        }
    }
    bail!("varint demasiado largo")
}

/// Writing protocol types into a `Vec<u8>`.
pub trait Writer {
    fn put(&mut self, b: &[u8]);

    fn u8(&mut self, v: u8) -> &mut Self {
        self.put(&[v]);
        self
    }
    fn i8(&mut self, v: i8) -> &mut Self {
        self.u8(v as u8)
    }
    fn bool(&mut self, v: bool) -> &mut Self {
        self.u8(v as u8)
    }
    fn u16(&mut self, v: u16) -> &mut Self {
        self.put(&v.to_be_bytes());
        self
    }
    fn i16(&mut self, v: i16) -> &mut Self {
        self.put(&v.to_be_bytes());
        self
    }
    fn i32(&mut self, v: i32) -> &mut Self {
        self.put(&v.to_be_bytes());
        self
    }
    fn i64(&mut self, v: i64) -> &mut Self {
        self.put(&v.to_be_bytes());
        self
    }
    fn f32(&mut self, v: f32) -> &mut Self {
        self.put(&v.to_be_bytes());
        self
    }
    fn f64(&mut self, v: f64) -> &mut Self {
        self.put(&v.to_be_bytes());
        self
    }
    fn uuid(&mut self, v: u128) -> &mut Self {
        self.put(&v.to_be_bytes());
        self
    }
    fn varint(&mut self, v: i32) -> &mut Self {
        let mut v = v as u32;
        loop {
            if v & !0x7f == 0 {
                self.put(&[v as u8]);
                return self;
            }
            self.put(&[(v as u8 & 0x7f) | 0x80]);
            v >>= 7;
        }
    }
    fn varlong(&mut self, v: i64) -> &mut Self {
        let mut v = v as u64;
        loop {
            if v & !0x7f == 0 {
                self.put(&[v as u8]);
                return self;
            }
            self.put(&[(v as u8 & 0x7f) | 0x80]);
            v >>= 7;
        }
    }
    fn string(&mut self, s: &str) -> &mut Self {
        self.varint(s.len() as i32);
        self.put(s.as_bytes());
        self
    }
    fn position(&mut self, p: BlockPos) -> &mut Self {
        self.i64(p.packed())
    }
    /// Angle in "steps" of 1/256 of a turn.
    fn angle(&mut self, degrees: f32) -> &mut Self {
        self.u8((degrees.rem_euclid(360.0) / 360.0 * 256.0) as i32 as u8)
    }
}

impl Writer for Vec<u8> {
    fn put(&mut self, b: &[u8]) {
        self.extend_from_slice(b);
    }
}

pub fn varint_len(v: i32) -> usize {
    let mut v = v as u32;
    let mut n = 1;
    while v & !0x7f != 0 {
        v >>= 7;
        n += 1;
    }
    n
}

/// Builds a packet body (id + data) ready to enqueue.
pub fn packet(id: i32) -> Vec<u8> {
    let mut v = Vec::with_capacity(32);
    v.varint(id);
    v
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BlockPos {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl BlockPos {
    pub const fn new(x: i32, y: i32, z: i32) -> Self {
        Self { x, y, z }
    }

    pub fn from_packed(v: i64) -> Self {
        Self {
            x: (v >> 38) as i32,
            y: (v << 52 >> 52) as i32,
            z: (v << 26 >> 38) as i32,
        }
    }

    pub fn packed(self) -> i64 {
        ((self.x as i64 & 0x3ff_ffff) << 38) | ((self.z as i64 & 0x3ff_ffff) << 12) | (self.y as i64 & 0xfff)
    }

    /// Neighbor in direction `face` (0=down, 1=up, 2=north, 3=south, 4=west, 5=east).
    pub fn offset(self, face: i32) -> Self {
        let (dx, dy, dz) = match face {
            0 => (0, -1, 0),
            1 => (0, 1, 0),
            2 => (0, 0, -1),
            3 => (0, 0, 1),
            4 => (-1, 0, 0),
            5 => (1, 0, 0),
            _ => (0, 0, 0),
        };
        Self::new(self.x + dx, self.y + dy, self.z + dz)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn varint_roundtrip() {
        for v in [0, 1, 127, 128, 255, 25565, 2097151, i32::MAX, -1, i32::MIN] {
            let mut b = Vec::new();
            b.varint(v);
            assert_eq!(b.len(), varint_len(v));
            assert_eq!(Reader::new(&b).varint().unwrap(), v);
        }
    }

    #[test]
    fn position_roundtrip() {
        for p in [BlockPos::new(0, 0, 0), BlockPos::new(-1, -64, 33554431), BlockPos::new(18357644, 831, -20882616)] {
            assert_eq!(BlockPos::from_packed(p.packed()), p);
        }
    }
}
