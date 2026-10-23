use super::*;
use std::{
    fs,
    sync::atomic::{AtomicU64, Ordering},
};

static SERIAL: AtomicU64 = AtomicU64::new(0);
struct TempPak(PathBuf);
impl TempPak {
    fn raw(bytes: &[u8]) -> Self {
        let path = std::env::temp_dir().join(format!(
            "outfit-packed-{}-{}.pak",
            std::process::id(),
            SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        let mut options = std::fs::OpenOptions::new();
        use std::io::Write;
        options
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap()
            .write_all(bytes)
            .unwrap();
        Self(path)
    }
    fn new(index: &[u8]) -> Self {
        Self::offset(index, 19)
    }
    fn offset(index: &[u8], offset: u64) -> Self {
        let mut bytes = b"SBAsset6".to_vec();
        bytes.extend(offset.to_be_bytes());
        bytes.extend([0x11, 0x22, 0x33]);
        bytes.extend(b"INDEX");
        bytes.extend(index);
        Self::raw(&bytes)
    }
    fn open(&self) -> Result<PackedAssets> {
        PackedAssets::open(&self.0)
    }
}
impl Drop for TempPak {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}
fn vlq(mut value: u64) -> Vec<u8> {
    let mut bytes = vec![(value & 127) as u8];
    value >>= 7;
    while value > 0 {
        bytes.push((value & 127) as u8 | 128);
        value >>= 7;
    }
    bytes.reverse();
    bytes
}
fn string(text: &str) -> Vec<u8> {
    [vlq(text.len() as u64), text.as_bytes().to_vec()].concat()
}
fn entry(path: &str, offset: u64, size: u64) -> Vec<u8> {
    [
        string(path),
        offset.to_be_bytes().to_vec(),
        size.to_be_bytes().to_vec(),
    ]
    .concat()
}
fn index(metadata: Vec<u8>, entries: Vec<Vec<u8>>) -> Vec<u8> {
    [metadata, vlq(entries.len() as u64), entries.concat()].concat()
}
fn basic(metadata: Vec<u8>) -> Vec<u8> {
    index(metadata, vec![entry("/test.bin", 16, 3)])
}
fn keyed(value: Vec<u8>) -> Vec<u8> {
    [vec![1], string("key"), value].concat()
}

#[test]
fn reads_untagged_metadata_and_casefolded_explicit_assets() {
    let file = TempPak::new(&index(
        [vec![1], string("name"), vec![5], string("base")].concat(),
        vec![entry("/Folder/Sprite.PNG", 16, 3)],
    ));
    let pak = file.open().unwrap();
    assert_eq!(pak.metadata["name"], "base");
    assert_eq!(pak.entries.len(), 1);
    assert_eq!(pak.read("/folder/SPRITE.png").unwrap(), [0x11, 0x22, 0x33]);
    assert!(
        pak.read("/missing")
            .unwrap_err()
            .to_string()
            .contains("Missing packed asset")
    );
}
#[test]
fn all_json_tags_preserve_big_endian_numbers_unicode_and_int64_limits() {
    let unicode = "é".repeat(70);
    let values = vec![
        ("null", vec![1]),
        ("legacyNull", vec![0]),
        ("float", [vec![2], 1.25f64.to_be_bytes().to_vec()].concat()),
        ("true", vec![3, 1]),
        ("false", vec![3, 0]),
        ("minusOne", vec![4, 1]),
        ("minus65", vec![4, 0x81, 1]),
        ("plus64", vec![4, 0x81, 0]),
        ("minInt64", [vec![4], vlq(u64::MAX)].concat()),
        ("maxInt64", [vec![4], vlq(u64::MAX - 1)].concat()),
        ("unicode", [vec![5], string(&unicode)].concat()),
        ("array", vec![6, 3, 1, 3, 1, 4, 2]),
        (
            "object",
            [vec![7, 1], string("child"), vec![5], string("yes")].concat(),
        ),
    ];
    let mut metadata = vlq(values.len() as u64);
    for (key, value) in values {
        metadata.extend(string(key));
        metadata.extend(value);
    }
    let file = TempPak::new(&basic(metadata));
    let pak = file.open().unwrap();
    assert_eq!(
        Value::Object(pak.metadata),
        serde_json::json!({"null":null,"legacyNull":null,"float":1.25,"true":true,"false":false,"minusOne":-1,"minus65":-65,"plus64":64,"minInt64":i64::MIN,"maxInt64":i64::MAX,"unicode":unicode,"array":[null,true,1],"object":{"child":"yes"}})
    );
}
#[test]
fn multibyte_counts_and_zero_length_entries_do_not_imply_overlaps() {
    let entries = (0..130)
        .map(|i| entry(&format!("/empty-{i}"), 16, 0))
        .collect();
    let file = TempPak::new(&index(vec![0], entries));
    let pak = file.open().unwrap();
    assert_eq!(pak.entries.len(), 130);
    assert!(pak.read("/empty-129").unwrap().is_empty());
}
#[test]
fn corrupt_addressing_paths_duplicates_overlaps_and_truncation_are_rejected() {
    let mut cases = vec![
        index(vec![0], vec![entry("/bad", 15, 1)]),
        index(vec![0], vec![entry("/bad", 18, 2)]),
        index(vec![0], vec![entry("/bad", 20, 0)]),
        index(vec![0], vec![entry("/bad", 1 << 53, 0)]),
        index(vec![0], vec![entry("/same", 16, 3), entry("/same", 16, 3)]),
        index(vec![0], vec![entry("/Same", 16, 3), entry("/same", 16, 3)]),
        index(vec![0], vec![entry("/one", 16, 2), entry("/two", 17, 2)]),
    ];
    for path in [
        "../escape",
        "/has/../escape",
        "/has/./escape",
        "/zero\0",
        "/bad\\path",
    ] {
        cases.push(index(vec![0], vec![entry(path, 16, 3)]));
    }
    let mut trailing = basic(vec![0]);
    trailing.push(0);
    cases.push(trailing);
    let mut truncated = basic(vec![0]);
    truncated.pop();
    cases.push(truncated);
    for bytes in cases {
        assert!(TempPak::new(&bytes).open().is_err());
    }
    for offset in [0, 1 << 63] {
        assert!(TempPak::offset(&basic(vec![0]), offset).open().is_err());
    }
}
#[test]
fn malformed_metadata_and_noncanonical_or_overflowing_vlq_are_rejected() {
    let mut deep = vec![];
    for _ in 0..66 {
        deep.extend([6, 1]);
    }
    deep.push(1);
    let metadata = vec![
        keyed(vec![8]),
        keyed(vec![3, 2]),
        keyed([vec![4], vec![0x81; 10]].concat()),
        keyed([vec![4, 0x82], vec![0x80; 8], vec![0]].concat()),
        keyed(vec![4, 0x80, 0]),
        keyed(vec![5, 2, 0xff, 0xff]),
        keyed(vec![5, 0xff, 0x7f]),
        [vec![2], string("key"), vec![1], string("key"), vec![1]].concat(),
        keyed([vec![2], f64::INFINITY.to_be_bytes().to_vec()].concat()),
        keyed(deep),
    ];
    for bytes in metadata {
        assert!(TempPak::new(&basic(bytes)).open().is_err());
    }
}
#[test]
fn header_and_index_magic_and_physical_header_length_are_required() {
    for bytes in [
        [
            b"SBAsset7".to_vec(),
            16u64.to_be_bytes().to_vec(),
            b"INDEX\0\0".to_vec(),
        ]
        .concat(),
        [
            b"SBAsset6".to_vec(),
            16u64.to_be_bytes().to_vec(),
            b"WRONG\0\0".to_vec(),
        ]
        .concat(),
        b"SBAsset6".to_vec(),
    ] {
        assert!(TempPak::raw(&bytes).open().is_err());
    }
}
