//! murmur3_x64_128 for `buvid_fp` — port of `util/auth/cookie.py`.
//! Verified against the Python implementation (SMHasher reference
//! constants, all arithmetic mod 2^64).

const C1: u64 = 0x87C3_7B91_1142_53D5;
const C2: u64 = 0x4CF5_AD43_2745_937F;
const C3: u64 = 0x52DC_E729;
const C4: u64 = 0x3849_5AB5;
const R1: u32 = 27;
const R2: u32 = 31;
const R3: u32 = 33;
const M: u64 = 5;

fn fmix64(mut k: u64) -> u64 {
    const F1: u64 = 0xFF51_AFD7_ED55_8CCD;
    const F2: u64 = 0xC4CE_B9FE_1A85_EC53;
    k ^= k >> 33;
    k = k.wrapping_mul(F1);
    k ^= k >> 33;
    k = k.wrapping_mul(F2);
    k ^= k >> 33;
    k
}

/// 128-bit digest, `h1` (low) first — matches Python's
/// `(h2 << 64) | h1` return.
pub fn murmur3_x64_128(source: &[u8], seed: u64) -> (u64, u64) {
    let mut h1 = seed;
    let mut h2 = seed;
    let mut processed: usize = 0;

    #[allow(clippy::chunks_exact_to_as_chunks)] // block-for-block mirror of the Python port
    let mut chunks = source.chunks_exact(16);
    for block in &mut chunks {
        let mut buf = [0u8; 16];
        buf.copy_from_slice(block);
        let k1 = u64::from_le_bytes(buf[..8].try_into().unwrap());
        let k2 = u64::from_le_bytes(buf[8..].try_into().unwrap());

        h1 ^= k1.wrapping_mul(C1).rotate_left(R2).wrapping_mul(C2);
        h1 = h1
            .rotate_left(R1)
            .wrapping_add(h2)
            .wrapping_mul(M)
            .wrapping_add(C3);
        h2 ^= k2.wrapping_mul(C2).rotate_left(R3).wrapping_mul(C1);
        h2 = h2
            .rotate_left(R2)
            .wrapping_add(h1)
            .wrapping_mul(M)
            .wrapping_add(C4);
        processed += 16;
    }

    let read = chunks.remainder();
    processed += read.len();

    if !read.is_empty() {
        let mut k1: u64 = 0;
        let mut k2: u64 = 0;

        // k2: bytes 8..len (guard per byte, len>=9 makes k2 live)
        for (i, &b) in read.iter().enumerate().skip(8) {
            k2 ^= (b as u64) << (8 * (i - 8));
        }
        if read.len() >= 9 {
            k2 = k2.wrapping_mul(C2).rotate_left(R3).wrapping_mul(C1);
            h2 ^= k2;
        }

        // k1: bytes 0..min(len, 8)
        for (i, &b) in read.iter().enumerate().take(8) {
            k1 ^= (b as u64) << (8 * i);
        }
        k1 = k1.wrapping_mul(C1).rotate_left(R2).wrapping_mul(C2);
        h1 ^= k1;
    }

    let p = processed as u64;
    h1 ^= p;
    h2 ^= p;
    h1 = h1.wrapping_add(h2);
    h2 = h2.wrapping_add(h1);
    h1 = fmix64(h1);
    h2 = fmix64(h2);
    h1 = h1.wrapping_add(h2);
    h2 = h2.wrapping_add(h1);
    (h1, h2)
}

/// `get_buvid_fp`'s hex: low 64 bits first, high second, no padding —
/// `hex(m & (MOD-1))[2:] + hex(m >> 64)[2:]`.
pub fn buvid_fp_hex(user_agent: &str, seed: u64) -> String {
    let (h1, h2) = murmur3_x64_128(user_agent.as_bytes(), seed);
    format!("{h1:x}{h2:x}")
}
