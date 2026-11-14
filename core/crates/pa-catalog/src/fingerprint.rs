// SPDX-License-Identifier: MPL-2.0
//! Content fingerprint of a file: the basis of the `file:` media key
//! (`pa_model::MediaKey::File`, "hash of size + first and last 64 KiB").
//!
//! The value is the OSHash (OpenSubtitles hash): the file size plus the
//! little-endian 64-bit words of the first and the last 64 KiB, wrapping,
//! as 16 lowercase hex digits. It is written to both `files.oshash` and
//! `files.fingerprint`. LIB-004 owns hashing; it must keep this value, or
//! progress stored under `file:` keys loses its files.
//!
//! Files shorter than 64 KiB hash their whole content twice (start and end
//! chunks overlap), zero-padding the last partial word.

use pa_library::rusqlite::{Connection, params};
use pa_vfs::{ByteSource, Vfs, VfsPath};

use crate::Error;

/// Bytes hashed at each end of the file.
pub const CHUNK: u64 = 64 * 1024;

/// OSHash of `source`, 16 lowercase hex digits.
pub fn oshash(source: &dyn ByteSource) -> Result<String, Error> {
    let size = source.size();
    let chunk = size.min(CHUNK);
    let mut hash = size;
    let mut buf = vec![0u8; usize::try_from(chunk).unwrap_or(0)];
    for offset in [0, size - chunk] {
        source.read_exact_at(offset, &mut buf)?;
        hash = buf.chunks(8).fold(hash, |h, w| {
            let mut word = [0u8; 8];
            word[..w.len()].copy_from_slice(w);
            h.wrapping_add(u64::from_le_bytes(word))
        });
    }
    Ok(format!("{hash:016x}"))
}

/// OSHash of a local file.
pub fn oshash_file(path: &std::path::Path) -> Result<String, Error> {
    let dir = path
        .parent()
        .ok_or_else(|| Error::Invalid(format!("no parent: {}", path.display())))?;
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| Error::Invalid(format!("bad file name: {}", path.display())))?;
    let fs = pa_vfs::LocalFs::new(dir)?;
    let source = fs.open(&VfsPath::root().join(name)?)?;
    oshash(&*source)
}

/// Fingerprints the media files of `source_id` that have none yet (new or
/// changed since the last scan). Unreadable files are skipped and counted.
/// Returns `(hashed, failed)`.
pub(crate) fn fill_missing(
    conn: &mut Connection,
    source_id: i64,
    vfs: &dyn Vfs,
    on_progress: &mut dyn FnMut(usize, usize),
) -> Result<(usize, usize), Error> {
    let files: Vec<(i64, String)> = {
        let mut stmt = conn.prepare(
            "SELECT id, path FROM files
             WHERE source_id = ?1 AND fingerprint IS NULL AND missing_since_ms IS NULL
             ORDER BY id",
        )?;
        stmt.query_map([source_id], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<Result<_, _>>()?
    };
    let total = files.len();
    let (mut hashed, mut failed) = (0, 0);
    for batch in files.chunks(100) {
        let mut done = Vec::with_capacity(batch.len());
        for (id, path) in batch {
            let hash = VfsPath::parse(path)
                .map_err(Error::from)
                .and_then(|p| Ok(vfs.open(&p)?))
                .and_then(|s| oshash(&*s));
            match hash {
                Ok(h) => done.push((*id, h)),
                Err(_) => failed += 1,
            }
        }
        let tx = conn.transaction()?;
        {
            let mut update = tx.prepare_cached(
                "UPDATE files SET fingerprint = ?2, oshash = ?2, hashed_at_ms = ?3 WHERE id = ?1",
            )?;
            let now = crate::now_ms();
            for (id, h) in &done {
                update.execute(params![id, h, now])?;
            }
        }
        tx.commit()?;
        hashed += done.len();
        on_progress(hashed + failed, total);
    }
    Ok((hashed, failed))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct Mem(Vec<u8>);

    impl ByteSource for Mem {
        fn size(&self) -> u64 {
            self.0.len() as u64
        }
        fn read_at(&self, offset: u64, buf: &mut [u8]) -> pa_vfs::Result<usize> {
            let start = usize::try_from(offset).unwrap().min(self.0.len());
            let n = buf.len().min(self.0.len() - start);
            buf[..n].copy_from_slice(&self.0[start..start + n]);
            Ok(n)
        }
    }

    #[test]
    fn zero_file_hash_is_its_size() {
        // All words are zero: the hash is the size.
        let data = vec![0u8; 200_000];
        assert_eq!(oshash(&Mem(data)).unwrap(), format!("{:016x}", 200_000));
    }

    #[test]
    fn matches_reference_sum() {
        // 256 KiB of a byte pattern; reference: size + sum of LE words of
        // both 64 KiB ends.
        let data: Vec<u8> = (0..256 * 1024).map(|i| (i * 7 % 251) as u8).collect();
        let mut expect = data.len() as u64;
        for range in [0..65536, data.len() - 65536..data.len()] {
            for w in data[range].chunks(8) {
                expect = expect.wrapping_add(u64::from_le_bytes(w.try_into().unwrap()));
            }
        }
        assert_eq!(oshash(&Mem(data)).unwrap(), format!("{expect:016x}"));
    }

    #[test]
    fn short_and_empty_files() {
        assert_eq!(oshash(&Mem(Vec::new())).unwrap(), "0000000000000000");
        // 3 bytes: size 3 + word 0x030201 twice.
        let h = oshash(&Mem(vec![1, 2, 3])).unwrap();
        assert_eq!(h, format!("{:016x}", 3 + 2 * 0x0003_0201u64));
    }
}
