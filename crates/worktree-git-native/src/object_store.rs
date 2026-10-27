//! Cache-free, biturbo-style native Git object store (Stage-0 spike).
//!
//! This is a faithful port of the object-database half of `D:\Projects\GitFork\src\biturbo`
//! (`biturbo.dll`'s Rust reimplementation). It resolves Git objects with a
//! single hash lookup instead of spawning `git` for every read:
//!
//!   1. On first use per repository, every `.idx` is parsed once into an
//!      `OidMap` (`oid -> (pack, offset)`). The `.pack` payloads are
//!      memory-mapped and never copied wholesale; object bodies are sliced out
//!      of the mapping and inflated in place.
//!   2. An object read is: pack-index lookup (one hash hit) -> loose-object
//!      fallback (a single `CreateFileW`) -> `git cat-file` CLI last resort
//!      (thin/promisor/network-share object stores).
//!   3. A bounded in-memory `LocCache` (by pack offset) reuses inflated delta
//!      bases so a delta chain is never inflated twice.
//!   4. Pack-set churn from another process (`gc`/`repack`) is detected by a
//!      throttled (`SIG_CHECK_MS`) pack-signature probe; a missing object
//!      triggers one exact probe + a single rebuild.
//!
//! The whole point of the refactor this crate supports is to *remove* the
//! on-disk history cache and the stamp-based caches from GitComet: re-walking
//! the commit graph through this fast native store is cheap enough that a disk
//! cache is unnecessary. Nothing here writes to disk except via the git CLI
//! fallback.
//!
//! Correctness contract: [`read_object`] returns bytes identical to
//! `git cat-file <type> <oid>`. The parity tests enforce this against the real
//! git binary.

use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock};

// ---------------------------------------------------------------------------
// Object kinds
// ---------------------------------------------------------------------------

/// Object type, as reported by `git cat-file -t`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ObjKind {
    Commit,
    Tree,
    Blob,
    Tag,
}

impl ObjKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ObjKind::Commit => "commit",
            ObjKind::Tree => "tree",
            ObjKind::Blob => "blob",
            ObjKind::Tag => "tag",
        }
    }

    /// Pack object type nibble (1..=4) -> kind.
    fn from_nibble(nibble: u8) -> Option<ObjKind> {
        Some(match nibble {
            1 => ObjKind::Commit,
            2 => ObjKind::Tree,
            3 => ObjKind::Blob,
            4 => ObjKind::Tag,
            _ => return None,
        })
    }
}

/// A fully reconstructed object (type + body). The body is exactly what
/// `git cat-file <type> <oid>` prints to stdout.
pub struct Obj {
    pub kind: ObjKind,
    pub data: Vec<u8>,
}

pub type ObjRef = Arc<Obj>;

// ---------------------------------------------------------------------------
// OID hashing (biturbo's custom hasher; keeps the map small and fast)
// ---------------------------------------------------------------------------

#[derive(Default, Clone, Copy)]
struct OidHasher(u64);

impl OidHasher {
    const K: u64 = 0x517c_c1b7_2722_0a95;

    #[inline]
    fn mix(&mut self, v: u64) {
        self.0 = (self.0.rotate_left(5) ^ v).wrapping_mul(Self::K);
    }
}

impl Hasher for OidHasher {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        let mut rest = bytes;
        while rest.len() >= 8 {
            self.mix(u64::from_le_bytes(rest[..8].try_into().unwrap()));
            rest = &rest[8..];
        }
        if !rest.is_empty() {
            let mut tail = 0u64;
            for (i, &b) in rest.iter().enumerate() {
                tail |= (b as u64) << (8 * i);
            }
            self.mix(tail);
        }
    }

    fn write_u8(&mut self, v: u8) {
        self.mix(v as u64);
    }

    fn write_usize(&mut self, v: usize) {
        self.mix(v as u64);
    }
}

type OidMap = HashMap<[u8; 20], PackLoc, BuildHasherDefault<OidHasher>>;

// ---------------------------------------------------------------------------
// Pack files (mmap, with a heap fallback for un-mappable shares)
// ---------------------------------------------------------------------------

/// Backing store of one `.pack`: a read-only mapping (the normal case) or, if
/// mapping is refused, the whole file read into memory.
enum PackData {
    Mapped(memmap2::Mmap),
    Heap(Vec<u8>),
}

impl PackData {
    // SAFETY: the mapping is read-only and the file is not modified while the
    // store lives (a concurrent repack is detected by `is_stale`).
    unsafe fn open(path: &Path) -> Option<PackData> {
        let file = std::fs::File::open(path).ok()?;
        if file.metadata().ok()?.len() == 0 {
            return None;
        }
        match unsafe { memmap2::Mmap::map(&file) } {
            Ok(map) => Some(PackData::Mapped(map)),
            Err(_) => std::fs::read(path).ok().map(PackData::Heap),
        }
    }

    fn bytes(&self) -> &[u8] {
        match self {
            PackData::Mapped(m) => m,
            PackData::Heap(v) => v,
        }
    }
}

/// One open `.pack`, addressed by byte offset.
struct PackFile {
    data: PackData,
}

impl PackFile {
    fn bytes(&self) -> &[u8] {
        self.data.bytes()
    }
}

/// Where one object lives: pack slot + byte offset inside that pack.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct PackLoc {
    pack: u32,
    offset: u64,
}

/// LRU of resolved objects, indexed by [`PackLoc`] so repeated delta bases are
/// inflated once. Bounded by both entry count and resident bytes.
struct LocCache {
    map: HashMap<PackLoc, (ObjRef, u64)>,
    order: std::collections::VecDeque<(PackLoc, u64)>,
    tick: u64,
    bytes: usize,
}

impl LocCache {
    const MAX_ENTRIES: usize = 4096;
    const MAX_BYTES: usize = 32 << 20;

    fn new() -> LocCache {
        LocCache {
            map: HashMap::new(),
            order: std::collections::VecDeque::new(),
            tick: 0,
            bytes: 0,
        }
    }

    fn get(&mut self, loc: PackLoc) -> Option<ObjRef> {
        self.tick += 1;
        let tick = self.tick;
        let hit = self.map.get_mut(&loc)?;
        hit.1 = tick;
        let arc = hit.0.clone();
        self.order.push_back((loc, tick));
        self.compact_if_needed();
        Some(arc)
    }

    fn put(&mut self, loc: PackLoc, obj: &ObjRef) {
        if self.map.contains_key(&loc) {
            return;
        }
        self.tick += 1;
        self.bytes += obj.data.len();
        self.map.insert(loc, (obj.clone(), self.tick));
        self.order.push_back((loc, self.tick));
        while self.map.len() > Self::MAX_ENTRIES
            || (self.bytes > Self::MAX_BYTES && self.map.len() > 1)
        {
            let (key, tick) = match self.order.pop_front() {
                Some(v) => v,
                None => break,
            };
            if let Some(entry) = self.map.get(&key) {
                if entry.1 == tick {
                    self.bytes -= entry.0.data.len();
                    self.map.remove(&key);
                }
            }
        }
        self.compact_if_needed();
    }

    /// `order` keeps one entry per hit and is pruned lazily; rewrite it from the
    /// live set once the garbage outweighs the useful entries.
    fn compact_if_needed(&mut self) {
        if self.order.len() <= self.map.len() * 8 + 512 {
            return;
        }
        let mut live: Vec<(PackLoc, u64)> = self.map.iter().map(|(k, v)| (*k, v.1)).collect();
        live.sort_by_key(|e| e.1);
        self.order = live.into();
    }
}

// ---------------------------------------------------------------------------
// Object store (one per git_dir)
// ---------------------------------------------------------------------------

/// `(pack dir, .idx count, newest .idx mtime)` — cheap staleness probe used
/// only when an object is missing from the index (gc/repack from another
/// process), so a long-lived instance picks up newly created packs.
type PackSignature = Vec<(PathBuf, usize, u128)>;

pub(crate) struct ObjectStore {
    /// `objects` directories (main + alternates), used for the loose fallback.
    dirs: Vec<PathBuf>,
    /// `objects/pack` directories, one per entry of `dirs`.
    pack_dirs: Vec<PathBuf>,
    packs: Vec<Arc<PackFile>>,
    map: OidMap,
    cache: Mutex<LocCache>,
    /// Last time the pack signature was probed (see [`ObjectStore::sig_check_due`]).
    last_sig_check: AtomicU64,
    signature: PackSignature,
}

impl ObjectStore {
    /// Throttle on the pack-signature probe: probing costs one `read_dir` +
    /// `metadata` per `.idx`; throttle keeps a burst of reads free while still
    /// noticing a `gc`/`repack` within [`Self::SIG_CHECK_MS`].
    const SIG_CHECK_MS: u64 = 200;

    fn open(git_dir: &Path) -> ObjectStore {
        let dirs = compute_object_dirs(git_dir);
        let pack_dirs: Vec<PathBuf> = dirs.iter().map(|d| d.join("pack")).collect();
        let mut packs: Vec<Arc<PackFile>> = Vec::new();

        // Sorted so duplicate OIDs across packs always resolve the same way
        // (git orders packs by name too; the body is identical either way).
        let mut idx_paths: Vec<PathBuf> = Vec::new();
        for pack_dir in &pack_dirs {
            if let Ok(entries) = std::fs::read_dir(pack_dir) {
                idx_paths.extend(
                    entries
                        .flatten()
                        .map(|e| e.path())
                        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("idx")),
                );
            }
        }
        idx_paths.sort();

        // Pre-size the table from the per-index object counts: rehashing
        // hundreds of thousands of entries as the map grows would cost more
        // than parsing the indexes.
        let total: usize = idx_paths.iter().map(|p| index_object_count(p)).sum();
        let mut map: OidMap = HashMap::with_capacity_and_hasher(total, BuildHasherDefault::default());

        for idx_path in idx_paths {
            // Map the payload first: an index without a usable pack is ignored
            // so those objects fall through to the loose store (and ultimately
            // to the git CLI) exactly as before.
            let data = match unsafe { PackData::open(&idx_path.with_extension("pack")) } {
                Some(d) => d,
                None => continue,
            };
            if data.bytes().len() < 12 {
                continue; // truncated / not a pack ("PACK" + version + count)
            }
            let idx = match std::fs::read(&idx_path) {
                Ok(v) => v,
                Err(_) => continue,
            };
            let slot = packs.len() as u32;
            let mut found = 0usize;
            for_each_index_entry(&idx, |oid, offset| {
                found += 1;
                map.entry(*oid).or_insert(PackLoc { pack: slot, offset });
            });
            if found > 0 {
                packs.push(Arc::new(PackFile { data }));
            }
        }

        ObjectStore {
            signature: pack_signature(&pack_dirs),
            last_sig_check: AtomicU64::new(0),
            dirs,
            pack_dirs,
            packs,
            map,
            cache: Mutex::new(LocCache::new()),
        }
    }

    /// True when this call is allowed to probe the pack signature.
    fn sig_check_due(&self) -> bool {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let last = self.last_sig_check.load(Ordering::Relaxed);
        if now.saturating_sub(last) < Self::SIG_CHECK_MS {
            return false;
        }
        self.last_sig_check.store(now, Ordering::Relaxed);
        true
    }

    fn cache_get(&self, loc: PackLoc) -> Option<ObjRef> {
        self.cache.lock().ok()?.get(loc)
    }

    fn cache_put(&self, loc: PackLoc, obj: &ObjRef) {
        if let Ok(mut cache) = self.cache.lock() {
            cache.put(loc, obj);
        }
    }

    /// True when the set of packs changed since the index was built.
    fn is_stale(&self) -> bool {
        pack_signature(&self.pack_dirs) != self.signature
    }

    pub(crate) fn dirs(&self) -> &[PathBuf] {
        &self.dirs
    }
}

// ---------------------------------------------------------------------------
// Registry: one lazily-built store per git_dir
// ---------------------------------------------------------------------------

fn store_registry(
) -> &'static RwLock<HashMap<PathBuf, Arc<ObjectStore>>> {
    static STORES: OnceLock<RwLock<HashMap<PathBuf, Arc<ObjectStore>>>> = OnceLock::new();
    STORES.get_or_init(|| RwLock::new(HashMap::new()))
}

/// One `ObjectStore` per `git_dir`, built lazily on the first object read.
fn store_for(git_dir: &Path) -> Arc<ObjectStore> {
    let registry = store_registry();
    if let Ok(guard) = registry.read() {
        if let Some(store) = guard.get(git_dir) {
            return store.clone();
        }
    }
    let store = Arc::new(ObjectStore::open(git_dir));
    if let Ok(mut guard) = registry.write() {
        return guard.entry(git_dir.to_path_buf()).or_insert(store).clone();
    }
    store
}

/// Rebuild (and re-publish) the store after the pack set changed on disk.
fn store_reload(git_dir: &Path) -> Arc<ObjectStore> {
    let store = Arc::new(ObjectStore::open(git_dir));
    if let Ok(mut guard) = store_registry().write() {
        guard.insert(git_dir.to_path_buf(), store.clone());
    }
    store
}

/// The store to use for one object read, re-opened first when the pack set
/// changed since it was built (a `gc`/`repack` from another process).
fn store_current(git_dir: &Path) -> Arc<ObjectStore> {
    let store = store_for(git_dir);
    if store.sig_check_due() && store.is_stale() {
        return store_reload(git_dir);
    }
    store
}

// ---------------------------------------------------------------------------
// Object directories (commondir + alternates, depth <= 5)
// ---------------------------------------------------------------------------

/// Object directories visible from `git_dir`: its own `objects` dir (resolving
/// `commondir` for linked worktrees) plus any `objects/info/alternates` roots,
/// recursively (depth <= 5, matching the shipped engine).
fn compute_object_dirs(git_dir: &Path) -> Vec<PathBuf> {
    fn push_dir(dir: PathBuf, depth: usize, out: &mut Vec<PathBuf>) {
        if depth > 5 || out.iter().any(|d| d == &dir) {
            return;
        }
        out.push(dir.clone());
        if let Ok(text) = std::fs::read_to_string(dir.join("info").join("alternates")) {
            for line in text.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                let p = PathBuf::from(line);
                let alt = if p.is_absolute() {
                    p.join("objects")
                } else {
                    dir.join(p).join("objects")
                };
                push_dir(alt, depth + 1, out);
            }
        }
    }

    let base = match std::fs::read_to_string(git_dir.join("commondir")) {
        Ok(c) => {
            let p = PathBuf::from(c.trim());
            if p.is_absolute() {
                p
            } else {
                git_dir.join(p)
            }
        }
        Err(_) => git_dir.to_path_buf(),
    };
    let mut out: Vec<PathBuf> = Vec::new();
    push_dir(base.join("objects"), 0, &mut out);
    out
}

// ---------------------------------------------------------------------------
// Index parsing (v1 / v2), loose read, delta, inflate
// ---------------------------------------------------------------------------

/// Inflate a whole zlib stream into a fresh buffer.
fn zlib_inflate(compressed: &[u8]) -> Option<Vec<u8>> {
    use std::io::Read;
    let mut dec = flate2::read::ZlibDecoder::new(compressed);
    let mut out = Vec::new();
    dec.read_to_end(&mut out).ok()?;
    Some(out)
}

/// Inflate the zlib stream that starts at `at` in a pack image. `hint` is the
/// unpacked object size from the pack header; it only presizes the output.
fn inflate_at(pack: &PackFile, at: usize, hint: usize) -> Option<Vec<u8>> {
    use std::io::Read;
    let mut out = Vec::with_capacity(hint.min(64 << 20));
    // `&[u8]` is already a `BufRead`, so the decoder consumes the mapped bytes
    // in place — no intermediate copy of the pack window.
    let mut dec = flate2::bufread::ZlibDecoder::new(pack.bytes().get(at..)?);
    dec.read_to_end(&mut out).ok()?;
    Some(out)
}

/// Apply a git delta (INSERT literal / COPY-from-base) to `base`, producing the
/// reconstructed object body.
fn apply_delta(base: &[u8], delta: &[u8]) -> Option<Vec<u8>> {
    let mut i = 0usize;
    // header: base size varint + result size varint (skip both)
    skip_pack_varint(delta, &mut i)?;
    skip_pack_varint(delta, &mut i)?;
    let mut out: Vec<u8> = Vec::new();
    while i < delta.len() {
        let op = delta[i];
        i += 1;
        if op & 0x80 != 0 {
            // COPY: offset bits 0x01..0x08 (<=4 bytes LE), size bits 0x10..0x40
            let mut cp_off = 0u64;
            let mut cp_size = 0u64;
            for (bit, sh) in [(0x01u8, 0u32), (0x02, 8), (0x04, 16), (0x08, 24)] {
                if op & bit != 0 {
                    cp_off |= (*delta.get(i)? as u64) << sh;
                    i += 1;
                }
            }
            for (bit, sh) in [(0x10u8, 0u32), (0x20, 8), (0x40, 16)] {
                if op & bit != 0 {
                    cp_size |= (*delta.get(i)? as u64) << sh;
                    i += 1;
                }
            }
            if cp_size == 0 {
                cp_size = 0x10000;
            }
            let s = cp_off as usize;
            let e = s + cp_size as usize;
            if e > base.len() {
                return None;
            }
            out.extend_from_slice(&base[s..e]);
        } else if op != 0 {
            // INSERT: `op` literal bytes follow
            let n = op as usize;
            if i + n > delta.len() {
                return None;
            }
            out.extend_from_slice(&delta[i..i + n]);
            i += n;
        } else {
            return None; // opcode 0 is reserved
        }
    }
    Some(out)
}

fn skip_pack_varint(buf: &[u8], i: &mut usize) -> Option<()> {
    let mut c = *buf.get(*i)?;
    *i += 1;
    while c & 0x80 != 0 {
        c = *buf.get(*i)?;
        *i += 1;
    }
    Some(())
}

/// Walk every entry of a pack index, yielding `(oid, pack offset)`.
///
/// v2 (modern, only format git writes today): `magic \xfftOc`, `version=2`,
/// `fanout[256] u32`, `N×20` OIDs, `N×4` CRC, `N×4` offsets whose MSB selects
/// an entry in the trailing `M×8` large-offset table. v1 (no magic): fanout
/// first, then interleaved `4-byte offset + 20-byte OID` records.
///
/// Returns the number of entries accepted; 0 means "unsupported or malformed",
/// which makes the caller fall back to the git CLI for that pack.
fn for_each_index_entry(idx: &[u8], mut f: impl FnMut(&[u8; 20], u64)) -> usize {
    let be32 = |at: usize| -> Option<u32> {
        Some(u32::from_be_bytes(idx.get(at..at + 4)?.try_into().ok()?))
    };
    let be64 = |at: usize| -> Option<u64> {
        Some(u64::from_be_bytes(idx.get(at..at + 8)?.try_into().ok()?))
    };

    if idx.len() >= 8 && &idx[0..4] == b"\xfftOc" {
        if be32(4) != Some(2) {
            return 0;
        }
        let fan = 8usize;
        let n = match be32(fan + 255 * 4) {
            Some(n) => n as usize,
            None => return 0,
        };
        let oid_table = fan + 256 * 4; // 1032
        let crc_table = oid_table + n * 20;
        let off_table = crc_table + n * 4;
        let large_table = off_table + n * 4;
        if idx.len() < large_table {
            return 0;
        }
        for i in 0..n {
            let oid = match idx.get(oid_table + i * 20..oid_table + i * 20 + 20) {
                Some(s) => s,
                None => return 0,
            };
            let mut key = [0u8; 20];
            key.copy_from_slice(oid);
            let o32 = match be32(off_table + i * 4) {
                Some(v) => v,
                None => return 0,
            };
            let offset = if o32 & 0x8000_0000 != 0 {
                match be64(large_table + (o32 & 0x7fff_ffff) as usize * 8) {
                    Some(v) => v,
                    None => return 0,
                }
            } else {
                o32 as u64
            };
            f(&key, offset);
        }
        n
    } else {
        // v1: fanout[256] u32, then N × (4-byte offset, 20-byte OID).
        let n = match be32(255 * 4) {
            Some(n) => n as usize,
            None => return 0,
        };
        let table = 256 * 4;
        if idx.len() < table + n * 24 {
            return 0;
        }
        for i in 0..n {
            let at = table + i * 24;
            let o32 = be32(at).unwrap_or(0);
            let mut key = [0u8; 20];
            key.copy_from_slice(&idx[at + 4..at + 24]);
            f(&key, o32 as u64);
        }
        n
    }
}

/// Object count of a pack index, read from the tail of its fanout table without
/// loading the whole index. For v2 the fanout starts after the 8-byte header
/// (magic + version) and its last u32 holds the total object count; for v1 the
/// fanout is at the very start. (Reading the file trailer would instead return
/// trailer bytes and a garbage capacity.)
fn index_object_count(idx_path: &Path) -> usize {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = match std::fs::File::open(idx_path) {
        Ok(f) => f,
        Err(_) => return 0,
    };
    let mut magic = [0u8; 4];
    if file.read_exact(&mut magic).is_err() {
        return 0;
    }
    let fanout_base = if magic == *b"\xfftOc" { 8usize } else { 0usize };
    let last_entry = fanout_base + 255 * 4; // last u32 of the 256-entry fanout
    if file.seek(SeekFrom::Start(last_entry as u64)).is_err() {
        return 0;
    }
    let mut buf = [0u8; 4];
    if file.read_exact(&mut buf).is_err() {
        return 0;
    }
    let count = u32::from_be_bytes(buf) as usize;
    // Sanity cap: an index cannot plausibly hold this many objects, and it
    // keeps `with_capacity` from ballooning on a malformed header.
    count.min(50_000_000)
}

/// `(pack dir, .idx count, newest .idx mtime)` for every pack dir.
fn pack_signature(pack_dirs: &[PathBuf]) -> PackSignature {
    let mut out = Vec::with_capacity(pack_dirs.len());
    for dir in pack_dirs {
        let mut count = 0usize;
        let mut newest = 0u128;
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                if entry.path().extension().and_then(|e| e.to_str()) != Some("idx") {
                    continue;
                }
                count += 1;
                if let Ok(meta) = entry.metadata() {
                    if let Ok(time) = meta.modified() {
                        if let Ok(delta) = time.duration_since(std::time::UNIX_EPOCH) {
                            newest = newest.max(delta.as_nanos());
                        }
                    }
                }
            }
        }
        out.push((dir.clone(), count, newest));
    }
    out
}

/// Read a loose git object natively: `<objects>/<aa>/<38hex>` -> zlib-inflate ->
/// split the `"<type> <size>\0<data>"` header. Returns the object on a hit,
/// `None` on a miss (packed object / not found / bad header) so the caller can
/// fall back to the git CLI.
fn read_loose_object(store: &ObjectStore, raw: &[u8; 20]) -> Option<Obj> {
    let hex = raw20_to_hex(raw);
    for objects in store.dirs() {
        let obj_path = objects.join(&hex[..2]).join(&hex[2..]);
        let compressed = match std::fs::read(&obj_path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let mut body = zlib_inflate(&compressed)?;
        let sp = body.iter().position(|&b| b == b' ')?;
        let nul = body.iter().position(|&b| b == 0)?;
        if nul < sp {
            return None;
        }
        let kind = match &body[..sp] {
            b"commit" => ObjKind::Commit,
            b"tree" => ObjKind::Tree,
            b"blob" => ObjKind::Blob,
            b"tag" => ObjKind::Tag,
            _ => return None,
        };
        body.drain(..=nul);
        return Some(Obj { kind, data: body });
    }
    None
}

/// Resolve the object at `loc`, following OFS_DELTA (same pack, relative
/// offset) and REF_DELTA (20-byte base OID, any pack or loose) chains.
const MAX_DELTA_DEPTH: usize = 1024;

fn resolve_pack_object(store: &ObjectStore, loc: PackLoc, depth: usize) -> Option<ObjRef> {
    if depth > MAX_DELTA_DEPTH {
        return None;
    }
    if let Some(hit) = store.cache_get(loc) {
        return Some(hit);
    }
    let pack = store.packs.get(loc.pack as usize)?;
    let image = pack.bytes();
    let start = usize::try_from(loc.offset).ok()?;
    // Object header: type+size varint, then the delta base reference.
    let header = image.get(start..)?;
    let first = *header.first()?;

    let type_nibble = (first & 0x70) >> 4;
    let mut p = 1usize;
    // size varint: low 4 bits first, then 7-bit groups. Only used to size the
    // inflate buffers — the decoder reports the authoritative length.
    let mut size = (first & 0x0f) as u64;
    let mut shift = 4u32;
    let mut c = first;
    while c & 0x80 != 0 {
        c = *header.get(p)?;
        p += 1;
        size |= ((c & 0x7f) as u64) << shift;
        shift += 7;
    }
    let hint = size.min(usize::MAX as u64) as usize;

    let obj = match type_nibble {
        1..=4 => {
            let kind = ObjKind::from_nibble(type_nibble)?;
            Obj {
                kind,
                data: inflate_at(pack, start + p, hint)?,
            }
        }
        6 => {
            // OFS_DELTA: distance varint encoded as base-128 with +1 bias.
            let mut cc = *header.get(p)?;
            p += 1;
            let mut rel = (cc & 0x7f) as u64;
            while cc & 0x80 != 0 {
                cc = *header.get(p)?;
                p += 1;
                rel = ((rel + 1) << 7) | (cc & 0x7f) as u64;
            }
            let base_offset = loc.offset.checked_sub(rel)?;
            let base = resolve_pack_object(
                store,
                PackLoc {
                    pack: loc.pack,
                    offset: base_offset,
                },
                depth + 1,
            )?;
            let delta = inflate_at(pack, start + p, hint)?;
            Obj {
                kind: base.kind,
                data: apply_delta(&base.data, &delta)?,
            }
        }
        7 => {
            // REF_DELTA: 20-byte base OID, resolved through the global map (or
            // a loose object) so thin-ish packs that reference other packs work.
            let mut base_oid = [0u8; 20];
            base_oid.copy_from_slice(header.get(p..p + 20)?);
            p += 20;
            let base = read_object_raw(store, &base_oid, depth + 1)?;
            let delta = inflate_at(pack, start + p, hint)?;
            Obj {
                kind: base.kind,
                data: apply_delta(&base.data, &delta)?,
            }
        }
        _ => return None,
    };

    let obj = Arc::new(obj);
    store.cache_put(loc, &obj);
    Some(obj)
}

/// Look an object up in the pack index, then in the loose store.
fn read_object_raw(store: &ObjectStore, oid: &[u8; 20], depth: usize) -> Option<ObjRef> {
    if let Some(&loc) = store.map.get(oid) {
        if let Some(obj) = resolve_pack_object(store, loc, depth) {
            return Some(obj);
        }
    }
    // Fewer than 1% of objects are loose in a gc'd repository, so this miss is
    // cheap: it is a single `CreateFileW`, and only after the index has failed.
    read_loose_object(store, oid).map(Arc::new)
}

/// Object read against an already-resolved store: pack index first (one hash
/// lookup), loose second, plus one rebuild when the index went stale. Returns
/// the shared object, so a caller that only inspects it never copies the body
/// out of the location cache.
fn read_object_ref(store: &ObjectStore, git_dir: &Path, hex: &str) -> Option<ObjRef> {
    let oid = hex_to_raw(hex)?;
    if let Some(obj) = read_object_raw(store, &oid, 0) {
        return Some(obj);
    }
    // A `gc`/`repack` running in another process may have produced a pack after
    // we built the index; rebuild once and retry before giving up.
    if store.is_stale() {
        let fresh = store_reload(git_dir);
        if let Some(obj) = read_object_raw(&fresh, &oid, 0) {
            return Some(obj);
        }
    }
    None
}

/// Unified native object read: pack index first (one hash lookup), loose
/// second. `git_dir` is the repo's object root (`.git`).
/// Returns the detected object kind and its body (identical to
/// `git cat-file <type> <hex>`). The kind-agnostic variant is used by the
/// `worktree-git-gix` adapter, which knows the OID but not necessarily the type.
pub fn read_object(git_dir: &Path, hex: &str) -> Option<(ObjKind, Vec<u8>)> {
    read_object_ref(&store_current(git_dir), git_dir, hex).map(unwrap_obj)
}

/// Take the body out of a cached object, copying only when it is still shared.
fn unwrap_obj(obj: ObjRef) -> (ObjKind, Vec<u8>) {
    match Arc::try_unwrap(obj) {
        Ok(owned) => (owned.kind, owned.data),
        Err(shared) => (shared.kind, shared.data.clone()),
    }
}

/// Run a git plumbing command against `git_dir`, returning raw stdout bytes.
/// Only used as the last-resort object fallback (exotic object stores).
fn git_capture_bytes(git_dir: &Path, args: &[&str]) -> Option<Vec<u8>> {
    let gd = git_dir.to_str()?;
    let mut cmd = std::process::Command::new("git");
    cmd.arg("--git-dir").arg(gd);
    for a in args {
        cmd.arg(a);
    }
    let out = cmd.output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(out.stdout)
}

/// Fetch a git object's raw content, natively from the pack index or the loose
/// store, with a `git` CLI fallback for exotic object stores (thin packs whose
/// base is missing, promisor/partial clones). The returned bytes are the object
/// body, identical to `git cat-file <type> <hex>` stdout.
pub fn fetch_object_bytes(git_dir: &Path, hex: &str, obj_type: &str) -> Option<Vec<u8>> {
    if let Some((ty, data)) = read_object(git_dir, hex) {
        if ty.as_str() == obj_type {
            return Some(data);
        }
    }
    git_capture_bytes(git_dir, &["cat-file", obj_type, hex])
}

// ---------------------------------------------------------------------------
// Hex helpers
// ---------------------------------------------------------------------------

/// Raw 20-byte OID -> canonical 40-char lowercase hex.
fn raw20_to_hex(raw: &[u8]) -> String {
    let mut s = String::with_capacity(40);
    for &b in &raw[..20] {
        s.push(char::from_digit((b >> 4) as u32, 16).unwrap());
        s.push(char::from_digit((b & 0xf) as u32, 16).unwrap());
    }
    s
}

/// Parse a 40-char hex OID into its canonical 20 bytes.
fn hex_to_raw(hex: &str) -> Option<[u8; 20]> {
    let bytes = hex.as_bytes();
    if bytes.len() != 40 {
        return None;
    }
    let mut out = [0u8; 20];
    for i in 0..20 {
        out[i] = (hexval(bytes[2 * i])? << 4) | hexval(bytes[2 * i + 1])?;
    }
    Some(out)
}

fn hexval(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}
