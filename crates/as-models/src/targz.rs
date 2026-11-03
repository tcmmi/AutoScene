//! One regular-file member of a `.tar.gz`, copied out alone and hashed as
//! it is written (the writer hashes). The archive's own pinned hash is
//! checked before this runs, so what it decompresses is known; even so it
//! walks only the first [`MAX_ENTRIES`] entries and copies at most the
//! pinned size and a byte. Links, folders and other members are skipped,
//! and nothing is ever unpacked onto the disk by its own path.

use std::io::{self, Read, Write};

use flate2::read::GzDecoder;

/// The entries looked at for the member.
pub const MAX_ENTRIES: usize = 64;

/// What the archive held under the member's name.
#[derive(Debug, PartialEq, Eq)]
pub enum Taken {
    /// Copied: this many bytes, which is the pinned size only if they match.
    Copied(u64),
    /// No regular file of that exact path among the entries looked at.
    Missing,
    /// A regular file of that path, of another size than the pinned one.
    Size(u64),
}

/// Copy the regular file `member` (its exact path in the archive) of the
/// gzipped tar `archive` into `out`, if it is `size` bytes long.
pub fn take(archive: impl Read, member: &str, size: u64, out: &mut impl Write) -> io::Result<Taken> {
    let mut archive = tar::Archive::new(GzDecoder::new(archive));
    for entry in archive.entries()?.take(MAX_ENTRIES) {
        let entry = entry?;
        if !entry.header().entry_type().is_file() || *entry.path_bytes() != *member.as_bytes() {
            continue;
        }
        if entry.size() != size {
            return Ok(Taken::Size(entry.size()));
        }
        return io::copy(&mut entry.take(size + 1), out).map(Taken::Copied);
    }
    Ok(Taken::Missing)
}
