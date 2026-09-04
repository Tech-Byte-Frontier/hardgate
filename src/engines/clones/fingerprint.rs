use super::tokenizer::Token;
use std::path::Path;

const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
const FNV_PRIME: u64 = 0x100000001b3;

pub(super) fn clone_fingerprint(file_a: &Path, file_b: &Path, tokens: &[Token]) -> String {
    let mut files = [normalized_path(file_a), normalized_path(file_b)];
    files.sort_unstable();
    let mut digest = FNV_OFFSET_BASIS;
    digest = digest_bytes(digest, b"hardgate-clone-fingerprint\0");
    for file in files {
        digest = digest_len_prefixed(digest, file.as_bytes());
    }
    for token in tokens {
        digest = digest_len_prefixed(digest, token.kind.as_bytes());
    }
    format!("{digest:016x}")
}

fn normalized_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn digest_len_prefixed(mut digest: u64, bytes: &[u8]) -> u64 {
    digest = digest_bytes(digest, &(bytes.len() as u64).to_be_bytes());
    digest_bytes(digest, bytes)
}

fn digest_bytes(mut digest: u64, bytes: &[u8]) -> u64 {
    for byte in bytes {
        digest ^= *byte as u64;
        digest = digest.wrapping_mul(FNV_PRIME);
    }
    digest
}

pub(super) fn hash_token(kind: &str) -> u64 {
    digest_bytes(FNV_OFFSET_BASIS, kind.as_bytes())
}
