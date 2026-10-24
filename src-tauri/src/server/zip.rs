//! Streaming ZIP writer: no temp file, constant memory, ZIP64 when needed.
//!
//! Every entry is written with a data descriptor (sizes after the data), so
//! the archive can be produced in a single pass while it is being sent.

use std::fmt;
use std::fs::File;
use std::io::{self, Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use bytes::Bytes;
use chrono::{Datelike, Timelike};
use flate2::write::DeflateEncoder;
use flate2::Compression;
use tokio::sync::mpsc;

use super::CHUNK;
use crate::share::ZipEntry;

const LOCAL_HEADER_SIG: u32 = 0x0403_4b50;
const DATA_DESCRIPTOR_SIG: u32 = 0x0807_4b50;
const CENTRAL_HEADER_SIG: u32 = 0x0201_4b50;
const EOCD_SIG: u32 = 0x0605_4b50;
const ZIP64_EOCD_SIG: u32 = 0x0606_4b50;
const ZIP64_LOCATOR_SIG: u32 = 0x0706_4b50;

const FLAG_DATA_DESCRIPTOR: u16 = 1 << 3;
const FLAG_UTF8: u16 = 1 << 11;
const METHOD_STORE: u16 = 0;
const METHOD_DEFLATE: u16 = 8;
const VERSION_DEFAULT: u16 = 20;
const VERSION_ZIP64: u16 = 45;
/// Entries from this size on get ZIP64 descriptors. The header is written
/// before the data, so the decision needs a wide margin below 4 GiB
/// (DEFLATE can grow incompressible data slightly).
const ZIP64_THRESHOLD: u64 = 1 << 31;

/// File types that are already compressed; DEFLATE would only cost CPU.
const STORE_EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "png", "gif", "webp", "heic", "heif", "avif", "mp4", "mov", "m4v", "mkv",
    "webm", "avi", "mp3", "m4a", "aac", "ogg", "opus", "flac", "zip", "7z", "rar", "gz", "bz2",
    "xz", "zst", "docx", "xlsx", "pptx", "pages", "numbers", "key", "epub", "apk", "ipa", "dmg",
    "pdf", "jar",
];

fn should_store(name: &str) -> bool {
    match name.rsplit_once('.') {
        Some((_, ext)) => STORE_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()),
        None => false,
    }
}

struct CentralRecord {
    name: Vec<u8>,
    method: u16,
    crc: u32,
    compressed: u64,
    uncompressed: u64,
    offset: u64,
    time: u16,
    date: u16,
    is_dir: bool,
    zip64_descriptor: bool,
}

/// Counts bytes written to the inner writer (archive offsets and sizes).
struct Counting<W> {
    inner: W,
    count: u64,
}

impl<W: Write> Write for Counting<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let n = self.inner.write(buf)?;
        self.count += n as u64;
        Ok(n)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

struct ZipStreamWriter<W: Write> {
    out: Counting<W>,
    records: Vec<CentralRecord>,
    /// Uncompressed source bytes consumed so far (for progress).
    progress: Arc<AtomicU64>,
}

fn dos_datetime(path: &Path) -> (u16, u16) {
    let modified = std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .map(chrono::DateTime::<chrono::Local>::from)
        .unwrap_or_else(chrono::Local::now);
    let year = modified.year().clamp(1980, 2107) as u16;
    let date = ((year - 1980) << 9) | ((modified.month() as u16) << 5) | modified.day() as u16;
    let time = ((modified.hour() as u16) << 11)
        | ((modified.minute() as u16) << 5)
        | (modified.second() as u16 / 2);
    (time, date)
}

impl<W: Write> ZipStreamWriter<W> {
    fn new(out: W, progress: Arc<AtomicU64>) -> Self {
        Self {
            out: Counting {
                inner: out,
                count: 0,
            },
            records: Vec::new(),
            progress,
        }
    }

    /// Adds one entry. `file` must be the opened file for non-directories;
    /// at most `entry.size` bytes of it are archived, so a file that grows
    /// meanwhile cannot outgrow the header written up front.
    fn add(&mut self, entry: &ZipEntry, file: Option<File>) -> io::Result<()> {
        let name = entry.name.as_bytes().to_vec();
        let (time, date) = dos_datetime(&entry.path);
        let offset = self.out.count;
        let method = if entry.is_dir || should_store(&entry.name) {
            METHOD_STORE
        } else {
            METHOD_DEFLATE
        };
        let zip64 = entry.size >= ZIP64_THRESHOLD;

        // Local file header with zero CRC/sizes; the real values follow in
        // the data descriptor.
        let mut h = Vec::with_capacity(30 + name.len() + 20);
        put_u32(&mut h, LOCAL_HEADER_SIG);
        put_u16(
            &mut h,
            if zip64 {
                VERSION_ZIP64
            } else {
                VERSION_DEFAULT
            },
        );
        put_u16(&mut h, FLAG_DATA_DESCRIPTOR | FLAG_UTF8);
        put_u16(&mut h, method);
        put_u16(&mut h, time);
        put_u16(&mut h, date);
        put_u32(&mut h, 0);
        put_u32(&mut h, if zip64 { u32::MAX } else { 0 });
        put_u32(&mut h, if zip64 { u32::MAX } else { 0 });
        put_u16(&mut h, name.len() as u16);
        put_u16(&mut h, if zip64 { 20 } else { 0 });
        h.extend_from_slice(&name);
        if zip64 {
            put_u16(&mut h, 0x0001);
            put_u16(&mut h, 16);
            put_u64(&mut h, 0);
            put_u64(&mut h, 0);
        }
        self.out.write_all(&h)?;

        let data_start = self.out.count;
        let (crc, uncompressed) = match file {
            Some(file) => self.write_payload(file.take(entry.size), method)?,
            None => (0, 0),
        };
        let compressed = self.out.count - data_start;

        let mut d = Vec::with_capacity(24);
        put_u32(&mut d, DATA_DESCRIPTOR_SIG);
        put_u32(&mut d, crc);
        if zip64 {
            put_u64(&mut d, compressed);
            put_u64(&mut d, uncompressed);
        } else {
            put_u32(&mut d, compressed as u32);
            put_u32(&mut d, uncompressed as u32);
        }
        self.out.write_all(&d)?;

        self.records.push(CentralRecord {
            name,
            method,
            crc,
            compressed,
            uncompressed,
            offset,
            time,
            date,
            is_dir: entry.is_dir,
            zip64_descriptor: zip64,
        });
        Ok(())
    }

    /// Copies the file data (deflated or stored) and returns its CRC-32 and
    /// uncompressed size.
    fn write_payload(&mut self, source: impl Read, method: u16) -> io::Result<(u32, u64)> {
        if method == METHOD_DEFLATE {
            let mut encoder = DeflateEncoder::new(&mut self.out, Compression::fast());
            let result = copy_counted(source, &mut encoder, &self.progress)?;
            encoder.finish()?;
            Ok(result)
        } else {
            copy_counted(source, &mut self.out, &self.progress)
        }
    }

    /// Writes the central directory and returns the inner writer.
    fn finish(mut self) -> io::Result<W> {
        let cd_start = self.out.count;
        for r in &self.records {
            let needs64 = r.zip64_descriptor
                || r.compressed >= u32::MAX as u64
                || r.uncompressed >= u32::MAX as u64
                || r.offset >= u32::MAX as u64;
            let mut extra = Vec::new();
            if needs64 {
                put_u16(&mut extra, 0x0001);
                put_u16(&mut extra, 24);
                put_u64(&mut extra, r.uncompressed);
                put_u64(&mut extra, r.compressed);
                put_u64(&mut extra, r.offset);
            }
            let version = if needs64 {
                VERSION_ZIP64
            } else {
                VERSION_DEFAULT
            };
            let mut c = Vec::with_capacity(46 + r.name.len() + extra.len());
            put_u32(&mut c, CENTRAL_HEADER_SIG);
            put_u16(&mut c, version);
            put_u16(&mut c, version);
            put_u16(&mut c, FLAG_DATA_DESCRIPTOR | FLAG_UTF8);
            put_u16(&mut c, r.method);
            put_u16(&mut c, r.time);
            put_u16(&mut c, r.date);
            put_u32(&mut c, r.crc);
            put_u32(
                &mut c,
                if needs64 {
                    u32::MAX
                } else {
                    r.compressed as u32
                },
            );
            put_u32(
                &mut c,
                if needs64 {
                    u32::MAX
                } else {
                    r.uncompressed as u32
                },
            );
            put_u16(&mut c, r.name.len() as u16);
            put_u16(&mut c, extra.len() as u16);
            put_u16(&mut c, 0); // comment length
            put_u16(&mut c, 0); // disk number
            put_u16(&mut c, 0); // internal attributes
            put_u32(&mut c, if r.is_dir { 0x10 } else { 0 });
            put_u32(&mut c, if needs64 { u32::MAX } else { r.offset as u32 });
            c.extend_from_slice(&r.name);
            c.extend_from_slice(&extra);
            self.out.write_all(&c)?;
        }
        let cd_size = self.out.count - cd_start;
        let count = self.records.len() as u64;
        let need_zip64_end =
            count >= 0xFFFF || cd_size >= u32::MAX as u64 || cd_start >= u32::MAX as u64;

        let mut e = Vec::new();
        if need_zip64_end {
            let zip64_eocd_offset = self.out.count;
            put_u32(&mut e, ZIP64_EOCD_SIG);
            put_u64(&mut e, 44);
            put_u16(&mut e, VERSION_ZIP64);
            put_u16(&mut e, VERSION_ZIP64);
            put_u32(&mut e, 0);
            put_u32(&mut e, 0);
            put_u64(&mut e, count);
            put_u64(&mut e, count);
            put_u64(&mut e, cd_size);
            put_u64(&mut e, cd_start);
            put_u32(&mut e, ZIP64_LOCATOR_SIG);
            put_u32(&mut e, 0);
            put_u64(&mut e, zip64_eocd_offset);
            put_u32(&mut e, 1);
        }
        put_u32(&mut e, EOCD_SIG);
        put_u16(&mut e, 0);
        put_u16(&mut e, 0);
        let short_count = if need_zip64_end { 0xFFFF } else { count as u16 };
        put_u16(&mut e, short_count);
        put_u16(&mut e, short_count);
        put_u32(
            &mut e,
            if need_zip64_end {
                u32::MAX
            } else {
                cd_size as u32
            },
        );
        put_u32(
            &mut e,
            if need_zip64_end {
                u32::MAX
            } else {
                cd_start as u32
            },
        );
        put_u16(&mut e, 0);
        self.out.write_all(&e)?;
        self.out.flush()?;
        Ok(self.out.inner)
    }
}

/// Copies `source` into `dest` in 64 KiB blocks; returns CRC-32 and length
/// of the source data and adds it to `progress`.
fn copy_counted(
    mut source: impl Read,
    dest: &mut impl Write,
    progress: &AtomicU64,
) -> io::Result<(u32, u64)> {
    let mut crc = crc32fast::Hasher::new();
    let mut total = 0u64;
    let mut buf = vec![0u8; CHUNK];
    loop {
        let n = match source.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        crc.update(&buf[..n]);
        total += n as u64;
        dest.write_all(&buf[..n])?;
        progress.fetch_add(n as u64, Ordering::Relaxed);
    }
    Ok((crc.finalize(), total))
}

fn put_u16(v: &mut Vec<u8>, x: u16) {
    v.extend_from_slice(&x.to_le_bytes());
}
fn put_u32(v: &mut Vec<u8>, x: u32) {
    v.extend_from_slice(&x.to_le_bytes());
}
fn put_u64(v: &mut Vec<u8>, x: u64) {
    v.extend_from_slice(&x.to_le_bytes());
}

/// Marks the error a [`ChannelWriter`] fails with once the HTTP response is
/// gone, so it can be told apart from read errors.
#[derive(Debug)]
struct ClientGone;

impl fmt::Display for ClientGone {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("client disconnected")
    }
}

impl std::error::Error for ClientGone {}

/// True if the archive stopped because nobody receives it any more (as
/// opposed to a file that could not be read).
pub(super) fn is_client_gone(e: &io::Error) -> bool {
    e.get_ref().is_some_and(|inner| inner.is::<ClientGone>())
}

/// `Write` adapter that forwards 64 KiB chunks into a Tokio channel. Fails
/// with [`ClientGone`] once the receiving response was dropped -- also when
/// a stalled connection was closed by the idle timeout, so a phone that
/// stops reading never pins the blocking thread.
pub(super) struct ChannelWriter {
    tx: mpsc::Sender<Bytes>,
    buf: Vec<u8>,
}

impl ChannelWriter {
    pub fn new(tx: mpsc::Sender<Bytes>) -> Self {
        Self {
            tx,
            buf: Vec::with_capacity(CHUNK),
        }
    }

    fn send_buf(&mut self) -> io::Result<()> {
        if self.buf.is_empty() {
            return Ok(());
        }
        let chunk = Bytes::from(std::mem::replace(&mut self.buf, Vec::with_capacity(CHUNK)));
        self.tx
            .blocking_send(chunk)
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, ClientGone))
    }
}

impl Write for ChannelWriter {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        let room = CHUNK - self.buf.len();
        let n = room.min(data.len());
        self.buf.extend_from_slice(&data[..n]);
        if self.buf.len() >= CHUNK {
            self.send_buf()?;
        }
        Ok(n)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.send_buf()
    }
}

/// Writes a whole archive. Blocking: meant to run inside `spawn_blocking`.
/// Files that vanished or cannot be read are left out instead of breaking
/// the whole archive.
pub(super) fn write_archive<W: Write>(
    entries: &[ZipEntry],
    out: W,
    progress: Arc<AtomicU64>,
) -> io::Result<W> {
    let mut zip = ZipStreamWriter::new(out, progress);
    let mut skipped = 0usize;
    for entry in entries {
        let file = if entry.is_dir {
            None
        } else {
            match File::open(&entry.path) {
                Ok(f) => Some(f),
                Err(_) => {
                    skipped += 1;
                    continue;
                }
            }
        };
        zip.add(entry, file)?;
    }
    if skipped > 0 {
        log::warn!("{skipped} nicht lesbare Dateien im ZIP übersprungen");
    }
    zip.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn entry(path: &Path, name: &str, is_dir: bool) -> ZipEntry {
        ZipEntry {
            path: path.to_path_buf(),
            name: name.to_string(),
            size: if is_dir {
                0
            } else {
                std::fs::metadata(path).unwrap().len()
            },
            is_dir,
        }
    }

    #[test]
    fn archive_roundtrip_with_umlauts_and_emoji() {
        let dir = tempfile::tempdir().unwrap();
        let text = dir.path().join("Grüße 👋.txt");
        let photo = dir.path().join("foto.jpg");
        let text_data = "Hallo Welt! ".repeat(5000);
        std::fs::write(&text, &text_data).unwrap();
        std::fs::write(&photo, [1u8, 2, 3, 4, 5]).unwrap();

        let entries = vec![
            entry(dir.path(), "Ordner/", true),
            entry(&text, "Ordner/Grüße 👋.txt", false),
            entry(&photo, "Ordner/foto.jpg", false),
        ];
        let progress = Arc::new(AtomicU64::new(0));
        let bytes = write_archive(&entries, Vec::new(), progress.clone()).unwrap();
        assert_eq!(progress.load(Ordering::Relaxed), text_data.len() as u64 + 5);

        let mut archive = ::zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        assert_eq!(archive.len(), 3);
        let mut s = String::new();
        archive
            .by_name("Ordner/Grüße 👋.txt")
            .unwrap()
            .read_to_string(&mut s)
            .unwrap();
        assert_eq!(s, text_data);
        let f = archive.by_name("Ordner/foto.jpg").unwrap();
        assert_eq!(f.compression(), ::zip::CompressionMethod::Stored);
        drop(f);
        assert!(archive.by_name("Ordner/").unwrap().is_dir());
    }

    #[test]
    fn store_list() {
        assert!(should_store("IMG_0001.JPG"));
        assert!(should_store("film.mp4"));
        assert!(!should_store("notes.txt"));
        assert!(!should_store("README"));
    }

    #[test]
    fn many_entries_use_zip64_end_record() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("x.txt");
        std::fs::write(&f, b"x").unwrap();
        let entries: Vec<_> = (0..0x1_0005)
            .map(|i| ZipEntry {
                path: f.clone(),
                name: format!("f{i}.txt"),
                size: 1,
                is_dir: false,
            })
            .collect();
        let bytes = write_archive(&entries, Vec::new(), Arc::new(AtomicU64::new(0))).unwrap();
        let archive = ::zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        assert_eq!(archive.len(), 0x1_0005);
    }

    #[test]
    fn large_entries_use_zip64_descriptors_and_unreadable_files_are_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let small = dir.path().join("klein.bin");
        std::fs::write(&small, b"0123456789").unwrap();
        let entries = vec![
            // Listed as huge (e.g. it shrank since the scan): ZIP64 headers.
            ZipEntry {
                path: small.clone(),
                name: "gross.bin".into(),
                size: ZIP64_THRESHOLD,
                is_dir: false,
            },
            ZipEntry {
                path: dir.path().join("weg.txt"),
                name: "weg.txt".into(),
                size: 3,
                is_dir: false,
            },
        ];
        let bytes = write_archive(&entries, Vec::new(), Arc::new(AtomicU64::new(0))).unwrap();
        let mut archive = ::zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        assert_eq!(archive.len(), 1);
        let mut content = Vec::new();
        archive
            .by_name("gross.bin")
            .unwrap()
            .read_to_end(&mut content)
            .unwrap();
        assert_eq!(content, b"0123456789");
    }

    #[test]
    fn a_gone_client_is_told_apart_from_read_errors() {
        let (tx, rx) = mpsc::channel(1);
        drop(rx);
        let mut writer = ChannelWriter::new(tx);
        writer.write_all(b"data").unwrap();
        let err = writer.flush().unwrap_err();
        assert!(is_client_gone(&err));
        let read_error = io::Error::new(io::ErrorKind::BrokenPipe, "pipe closed");
        assert!(!is_client_gone(&read_error));
    }
}
