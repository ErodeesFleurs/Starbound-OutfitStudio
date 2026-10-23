use crate::{Result, fail};
use serde_json::{Map, Value};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

const EXACT_MAX: u64 = (1 << 53) - 1;

struct Cursor<'a> {
    bytes: &'a [u8],
    position: usize,
}
impl<'a> Cursor<'a> {
    fn take(&mut self, length: usize) -> Result<&'a [u8]> {
        if length > self.bytes.len() - self.position {
            return fail(format!(
                "Truncated SBAsset6 index at byte {} (need {length})",
                self.position
            ));
        }
        let start = self.position;
        self.position += length;
        Ok(&self.bytes[start..self.position])
    }
    fn byte(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn vlq(&mut self) -> Result<u64> {
        let mut value = 0u64;
        for i in 0..10 {
            let byte = self.byte()?;
            if i == 0 && byte == 0x80 {
                return fail("Noncanonical SBAsset6 VLQ");
            }
            if value > (u64::MAX >> 7) {
                return fail("SBAsset6 VLQ exceeds uint64");
            }
            value = (value << 7) | u64::from(byte & 127);
            if byte & 128 == 0 {
                return Ok(value);
            }
        }
        fail("Unterminated SBAsset6 VLQ")
    }
    fn count(&mut self) -> Result<usize> {
        let count = self.vlq()?;
        if count > (self.bytes.len() - self.position) as u64 {
            return fail("SBAsset6 count exceeds remaining index bytes");
        }
        Ok(count as usize)
    }
    fn string(&mut self) -> Result<String> {
        let length = self.count()?;
        Ok(std::str::from_utf8(self.take(length)?)?.to_owned())
    }
    fn object(&mut self, depth: usize) -> Result<Map<String, Value>> {
        if depth > 64 {
            return fail("SBAsset6 metadata nesting exceeds 64");
        }
        let count = self.count()?;
        let mut result = Map::new();
        for _ in 0..count {
            let key = self.string()?;
            if result.contains_key(&key) {
                return fail(format!("Duplicate SBAsset6 metadata key: {key}"));
            }
            result.insert(key, self.json(depth + 1)?);
        }
        Ok(result)
    }
    fn json(&mut self, depth: usize) -> Result<Value> {
        if depth > 64 {
            return fail("SBAsset6 metadata nesting exceeds 64");
        }
        Ok(match self.byte()? {
            0 | 1 => Value::Null,
            2 => {
                let number = f64::from_be_bytes(self.take(8)?.try_into()?);
                serde_json::Number::from_f64(number)
                    .ok_or("Nonfinite SBAsset6 JSON number")?
                    .into()
            }
            3 => match self.byte()? {
                0 => false.into(),
                1 => true.into(),
                _ => return fail("Invalid SBAsset6 JSON boolean"),
            },
            4 => {
                let encoded = self.vlq()?;
                let magnitude = (encoded >> 1) as i64;
                Value::from(if encoded & 1 != 0 {
                    -magnitude - 1
                } else {
                    magnitude
                })
            }
            5 => self.string()?.into(),
            6 => {
                let count = self.count()?;
                let mut values = Vec::with_capacity(count);
                for _ in 0..count {
                    values.push(self.json(depth + 1)?);
                }
                values.into()
            }
            7 => self.object(depth + 1)?.into(),
            _ => return fail("Unknown SBAsset6 JSON type tag"),
        })
    }
    fn uint64(&mut self) -> Result<u64> {
        let value = u64::from_be_bytes(self.take(8)?.try_into()?);
        if value > EXACT_MAX {
            return fail("SBAsset6 offset/size exceeds exact filesystem addressing");
        }
        Ok(value)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Entry {
    pub offset: u64,
    pub size: u64,
}

/// SBAsset6 packages are never modified; only header/index and requested assets are read.
pub struct PackedAssets {
    pub path: PathBuf,
    pub metadata: Map<String, Value>,
    pub entries: BTreeMap<String, Entry>,
    pub file_bytes: u64,
    pub index_bytes: u64,
    file: RefCell<File>,
}
impl PackedAssets {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let file = File::open(path.as_ref())?;
        let metadata = file.metadata()?;
        let file_bytes = metadata.len();
        if !metadata.is_file() || !(22..=EXACT_MAX).contains(&file_bytes) {
            return fail("Invalid SBAsset6 file size/type");
        }
        let mut result = Self {
            path: path.as_ref().to_owned(),
            metadata: Map::new(),
            entries: BTreeMap::new(),
            file_bytes,
            index_bytes: 0,
            file: RefCell::new(file),
        };
        let header = result.read_at(0, 16)?;
        if &header[..8] != b"SBAsset6" {
            return fail("Expected SBAsset6 header");
        }
        let index_start = u64::from_be_bytes(header[8..].try_into()?);
        if index_start < 16 || index_start > file_bytes - 6 {
            return fail("SBAsset6 index offset out of bounds");
        }
        result.index_bytes = file_bytes - index_start;
        if result.index_bytes > 128 * 1024 * 1024 {
            return fail("SBAsset6 index exceeds 128 MiB safety limit");
        }
        let bytes = result.read_at(index_start, result.index_bytes as usize)?;
        let mut cursor = Cursor {
            bytes: &bytes,
            position: 0,
        };
        if cursor.take(5)? != b"INDEX" {
            return fail("Expected SBAsset6 INDEX marker");
        }
        result.metadata = cursor.object(0)?;
        let count = cursor.count()?;
        for _ in 0..count {
            let name = cursor.string()?;
            let entry = Entry {
                offset: cursor.uint64()?,
                size: cursor.uint64()?,
            };
            if !name.starts_with('/')
                || name.contains(['\0', '\\'])
                || name.split('/').any(|p| p == "." || p == "..")
            {
                return fail(format!("Invalid SBAsset6 asset path: {name}"));
            }
            let folded = name.to_lowercase();
            if result.entries.contains_key(&folded) {
                return fail(format!("Duplicate SBAsset6 asset path: {name}"));
            }
            if entry.offset < 16
                || entry.offset > index_start
                || entry.size > index_start - entry.offset
            {
                return fail(format!("SBAsset6 entry out of bounds: {name}"));
            }
            result.entries.insert(folded, entry);
        }
        if cursor.position != bytes.len() {
            return fail("Trailing bytes in SBAsset6 index");
        }
        let mut intervals: Vec<_> = result.entries.iter().filter(|(_, e)| e.size != 0).collect();
        intervals.sort_unstable_by_key(|(_, entry)| entry.offset);
        for pair in intervals.windows(2) {
            if pair[1].1.offset < pair[0].1.offset + pair[0].1.size {
                return fail(format!(
                    "Overlapping SBAsset6 entries: {} and {}",
                    pair[0].0, pair[1].0
                ));
            }
        }
        Ok(result)
    }
    fn read_at(&self, offset: u64, length: usize) -> Result<Vec<u8>> {
        let mut result = vec![0; length];
        let mut file = self.file.borrow_mut();
        file.seek(SeekFrom::Start(offset))?;
        file.read_exact(&mut result)?;
        Ok(result)
    }
    pub fn read(&self, path: &str) -> Result<Vec<u8>> {
        let entry = self
            .entries
            .get(&path.to_lowercase())
            .ok_or_else(|| format!("Missing packed asset: {path}"))?;
        if entry.size > 256 * 1024 * 1024 {
            return fail(format!(
                "Requested asset exceeds 256 MiB safety limit: {path}"
            ));
        }
        self.read_at(entry.offset, entry.size as usize)
    }
    pub fn json(&self, path: &str) -> Result<Value> {
        Ok(json5::from_str(std::str::from_utf8(&self.read(path)?)?)?)
    }
}

#[cfg(test)]
mod tests;
