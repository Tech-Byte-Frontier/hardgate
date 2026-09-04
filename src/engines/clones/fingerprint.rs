use super::tokenizer::Token;

const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
const FNV_PRIME: u64 = 0x100000001b3;

pub(super) fn clone_fingerprint(tokens: &[Token]) -> String {
    let mut digest = digest_bytes(FNV_OFFSET_BASIS, b"hardgate-clone-fingerprint\0");
    for token in tokens {
        digest = digest_len_prefixed(digest, token.kind.as_bytes());
    }
    format!("{digest:016x}")
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
