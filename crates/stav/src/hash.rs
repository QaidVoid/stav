//! The BLAKE3 hash function, used to identify the contents of source files.
//!
//! This is the portable form of the algorithm for whole inputs in memory:
//! one chunk after another, merged into a binary tree of parent nodes, with
//! the default 32 byte output. Source files are at most a few mebibytes, so
//! the vectorized and multithreaded forms are not needed.

const IV: [u32; 8] = [
    0x6A09_E667,
    0xBB67_AE85,
    0x3C6E_F372,
    0xA54F_F53A,
    0x510E_527F,
    0x9B05_688C,
    0x1F83_D9AB,
    0x5BE0_CD19,
];

const PERMUTATION: [usize; 16] = [2, 6, 3, 10, 7, 0, 4, 13, 1, 11, 12, 5, 9, 14, 15, 8];

const CHUNK_START: u32 = 1;
const CHUNK_END: u32 = 2;
const PARENT: u32 = 4;
const ROOT: u32 = 8;

const BLOCK_LEN: usize = 64;
const CHUNK_LEN: usize = 1024;

fn g(state: &mut [u32; 16], a: usize, b: usize, c: usize, d: usize, x: u32, y: u32) {
    state[a] = state[a].wrapping_add(state[b]).wrapping_add(x);
    state[d] = (state[d] ^ state[a]).rotate_right(16);
    state[c] = state[c].wrapping_add(state[d]);
    state[b] = (state[b] ^ state[c]).rotate_right(12);
    state[a] = state[a].wrapping_add(state[b]).wrapping_add(y);
    state[d] = (state[d] ^ state[a]).rotate_right(8);
    state[c] = state[c].wrapping_add(state[d]);
    state[b] = (state[b] ^ state[c]).rotate_right(7);
}

fn round(state: &mut [u32; 16], m: &[u32; 16]) {
    g(state, 0, 4, 8, 12, m[0], m[1]);
    g(state, 1, 5, 9, 13, m[2], m[3]);
    g(state, 2, 6, 10, 14, m[4], m[5]);
    g(state, 3, 7, 11, 15, m[6], m[7]);
    g(state, 0, 5, 10, 15, m[8], m[9]);
    g(state, 1, 6, 11, 12, m[10], m[11]);
    g(state, 2, 7, 8, 13, m[12], m[13]);
    g(state, 3, 4, 9, 14, m[14], m[15]);
}

/// Compresses one block and returns the first eight words of the result,
/// the chaining value.
fn compress(cv: &[u32; 8], block: &[u32; 16], counter: u64, len: usize, flags: u32) -> [u32; 8] {
    #[allow(
        clippy::cast_possible_truncation,
        reason = "the counter is split into its low and high words, and a block is at most 64 bytes"
    )]
    let mut state = [
        cv[0],
        cv[1],
        cv[2],
        cv[3],
        cv[4],
        cv[5],
        cv[6],
        cv[7],
        IV[0],
        IV[1],
        IV[2],
        IV[3],
        counter as u32,
        (counter >> 32) as u32,
        len as u32,
        flags,
    ];
    let mut m = *block;
    for i in 0..7 {
        round(&mut state, &m);
        if i < 6 {
            m = PERMUTATION.map(|p| m[p]);
        }
    }
    std::array::from_fn(|i| state[i] ^ state[i + 8])
}

fn words(bytes: &[u8]) -> [u32; 16] {
    let mut block = [0u8; BLOCK_LEN];
    block[..bytes.len()].copy_from_slice(bytes);
    std::array::from_fn(|i| {
        u32::from_le_bytes([
            block[4 * i],
            block[4 * i + 1],
            block[4 * i + 2],
            block[4 * i + 3],
        ])
    })
}

/// The last compression of a chunk or parent node, kept back until it is
/// known whether it is the root of the tree.
struct Output {
    cv: [u32; 8],
    block: [u32; 16],
    counter: u64,
    len: usize,
    flags: u32,
}

impl Output {
    fn chaining_value(&self) -> [u32; 8] {
        compress(&self.cv, &self.block, self.counter, self.len, self.flags)
    }

    fn root(&self) -> [u8; 32] {
        let out = compress(&self.cv, &self.block, 0, self.len, self.flags | ROOT);
        let mut bytes = [0u8; 32];
        for (i, word) in out.iter().enumerate() {
            bytes[4 * i..4 * i + 4].copy_from_slice(&word.to_le_bytes());
        }
        bytes
    }
}

/// Returns the output of the chunk with index `counter`, at most 1024
/// bytes long.
fn chunk(input: &[u8], counter: u64) -> Output {
    let mut cv = IV;
    let mut blocks = input.chunks(BLOCK_LEN).peekable();
    let mut start = CHUNK_START;
    while let Some(block) = blocks.next() {
        if blocks.peek().is_none() {
            return Output {
                cv,
                block: words(block),
                counter,
                len: block.len(),
                flags: start | CHUNK_END,
            };
        }
        cv = compress(&cv, &words(block), counter, BLOCK_LEN, start);
        start = 0;
    }
    Output {
        cv,
        block: [0; 16],
        counter,
        len: 0,
        flags: start | CHUNK_END,
    }
}

fn parent(left: &[u32; 8], right: &[u32; 8]) -> Output {
    let mut block = [0u32; 16];
    block[..8].copy_from_slice(left);
    block[8..].copy_from_slice(right);
    Output {
        cv: IV,
        block,
        counter: 0,
        len: BLOCK_LEN,
        flags: PARENT,
    }
}

/// Returns the 32 byte BLAKE3 hash of `input`.
pub(crate) fn blake3(input: &[u8]) -> [u8; 32] {
    let mut stack: Vec<[u32; 8]> = Vec::new();
    let mut chunks = input.chunks(CHUNK_LEN).peekable();
    let mut counter = 0u64;
    let mut last = chunk(&[], 0);
    while let Some(bytes) = chunks.next() {
        let output = chunk(bytes, counter);
        if chunks.peek().is_none() {
            last = output;
            break;
        }
        let mut cv = output.chaining_value();
        counter += 1;
        let mut total = counter;
        while total & 1 == 0 {
            let Some(left) = stack.pop() else { break };
            cv = parent(&left, &cv).chaining_value();
            total >>= 1;
        }
        stack.push(cv);
    }
    while let Some(left) = stack.pop() {
        last = parent(&left, &last.chaining_value());
    }
    last.root()
}

#[cfg(test)]
#[path = "hash_tests.rs"]
mod tests;
