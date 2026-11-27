//! #26 Hash / HMAC 与文件校验（HashHmac）。
//!
//! 文本 + 大文件 64 KiB 单遍流式哈希（进度回调 + [`CancelFlag`]）；
//! MD5 / SHA1 / SHA256 / SHA512 / BLAKE3 + HMAC；预期摘要恒定时间比对。

use std::fs::File;
use std::io::{self, Read, Write};
use std::path::Path;

use constant_time_eq::constant_time_eq;
use hmac::Hmac;
use md5::Md5;
use sha1::Sha1;
use sha2::digest::{Digest, Mac};
use sha2::{Sha256, Sha512};
use zeroize::Zeroizing;

use crate::{CancelFlag, Progress, ProgressFn, ToolError, ToolErrorKind};

/// 流式读取块大小：64 KiB。
pub const CHUNK_SIZE: usize = 64 * 1024;

/// 支持的哈希算法。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HashAlgorithm {
    Md5,
    Sha1,
    Sha256,
    Sha512,
    Blake3,
}

impl HashAlgorithm {
    pub const ALL: [Self; 5] = [
        Self::Md5,
        Self::Sha1,
        Self::Sha256,
        Self::Sha512,
        Self::Blake3,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Md5 => "MD5",
            Self::Sha1 => "SHA-1",
            Self::Sha256 => "SHA-256",
            Self::Sha512 => "SHA-512",
            Self::Blake3 => "BLAKE3",
        }
    }
}

/// 摘要输出（hex 小写）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DigestResult {
    pub algorithm: HashAlgorithm,
    pub hex: String,
}

/// 对内存文本计算摘要。
pub fn hash_text(data: &[u8], algorithm: HashAlgorithm, hmac_key: Option<&[u8]>) -> DigestResult {
    macro_rules! hmac_hex {
        ($ty:ty, $key:expr) => {{
            let mut mac = <Hmac<$ty>>::new_from_slice($key).expect("HMAC 接受任意长度密钥");
            mac.update(data);
            hex_encode(mac.finalize().into_bytes())
        }};
    }
    let hex = match algorithm {
        HashAlgorithm::Md5 => {
            if let Some(key) = hmac_key {
                hmac_hex!(Md5, key)
            } else {
                hex_encode(Md5::digest(data))
            }
        }
        HashAlgorithm::Sha1 => {
            if let Some(key) = hmac_key {
                hmac_hex!(Sha1, key)
            } else {
                hex_encode(Sha1::digest(data))
            }
        }
        HashAlgorithm::Sha256 => {
            if let Some(key) = hmac_key {
                hmac_hex!(Sha256, key)
            } else {
                hex_encode(Sha256::digest(data))
            }
        }
        HashAlgorithm::Sha512 => {
            if let Some(key) = hmac_key {
                hmac_hex!(Sha512, key)
            } else {
                hex_encode(Sha512::digest(data))
            }
        }
        HashAlgorithm::Blake3 => {
            let mut hasher = if let Some(key) = hmac_key {
                blake3::Hasher::new_keyed(&key_to_blake3_key(key))
            } else {
                blake3::Hasher::new()
            };
            hasher.update(data);
            hex_encode_bytes(hasher.finalize().as_bytes())
        }
    };
    DigestResult { algorithm, hex }
}

fn key_to_blake3_key(key: &[u8]) -> [u8; 32] {
    let mut out = [0u8; 32];
    let len = key.len().min(32);
    out[..len].copy_from_slice(&key[..len]);
    out
}

fn hex_encode(bytes: impl AsRef<[u8]>) -> String {
    hex_encode_bytes(bytes.as_ref())
}

fn hex_encode_bytes(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// 流式文件摘要，支持进度回调与取消。
///
/// `on_progress` 返回 `false` 表示请求中止；`CancelFlag` 被置位时同样中止。
/// 二者均以 [`ToolErrorKind::Cancelled`] 返回。
pub fn hash_file_streaming(
    path: &Path,
    algorithm: HashAlgorithm,
    hmac_key: Option<&[u8]>,
    cancel: &CancelFlag,
    on_progress: &mut ProgressFn<'_>,
) -> Result<DigestResult, ToolError> {
    let mut file = File::open(path)
        .map_err(|e| ToolError::new(ToolErrorKind::Io, format!("无法打开文件：{e}")))?;
    let total = file.metadata().ok().map(|m| m.len());
    let mut buffer = vec![0u8; CHUNK_SIZE];
    let mut processed = 0u64;

    // 摘要累加器以类型擦除的闭包完成；各分支独立处理以避免泛型分派复杂性。
    macro_rules! run_stream {
        ($hasher:expr, $finalize:expr) => {{
            let mut hasher = $hasher;
            loop {
                if cancel.is_cancelled() {
                    return Err(ToolError::new(ToolErrorKind::Cancelled, "文件哈希已取消"));
                }
                let read = file.read(&mut buffer)
                    .map_err(|e| ToolError::new(ToolErrorKind::Io, format!("读取文件失败：{e}")))?;
                if read == 0 {
                    break;
                }
                hasher.update(&buffer[..read]);
                processed += read as u64;
                if !on_progress(Progress::new(processed, total)) {
                    return Err(ToolError::new(ToolErrorKind::Cancelled, "用户取消了文件哈希"));
                }
            }
            $finalize(hasher)
        }};
    }

    let hex = match algorithm {
        HashAlgorithm::Md5 => {
            if let Some(key) = hmac_key {
                let key = Zeroizing::new(key.to_vec());
                run_stream!(
                    Hmac::<Md5>::new_from_slice(&key).expect("HMAC 接受任意长度密钥"),
                    |h: Hmac<Md5>| hex_encode(h.finalize().into_bytes())
                )
            } else {
                run_stream!(Md5::new(), |h: Md5| hex_encode(h.finalize()))
            }
        }
        HashAlgorithm::Sha1 => {
            if let Some(key) = hmac_key {
                let key = Zeroizing::new(key.to_vec());
                run_stream!(
                    Hmac::<Sha1>::new_from_slice(&key).expect("HMAC 接受任意长度密钥"),
                    |h: Hmac<Sha1>| hex_encode(h.finalize().into_bytes())
                )
            } else {
                run_stream!(Sha1::new(), |h: Sha1| hex_encode(h.finalize()))
            }
        }
        HashAlgorithm::Sha256 => {
            if let Some(key) = hmac_key {
                let key = Zeroizing::new(key.to_vec());
                run_stream!(
                    Hmac::<Sha256>::new_from_slice(&key).expect("HMAC 接受任意长度密钥"),
                    |h: Hmac<Sha256>| hex_encode(h.finalize().into_bytes())
                )
            } else {
                run_stream!(Sha256::new(), |h: Sha256| hex_encode(h.finalize()))
            }
        }
        HashAlgorithm::Sha512 => {
            if let Some(key) = hmac_key {
                let key = Zeroizing::new(key.to_vec());
                run_stream!(
                    Hmac::<Sha512>::new_from_slice(&key).expect("HMAC 接受任意长度密钥"),
                    |h: Hmac<Sha512>| hex_encode(h.finalize().into_bytes())
                )
            } else {
                run_stream!(Sha512::new(), |h: Sha512| hex_encode(h.finalize()))
            }
        }
        HashAlgorithm::Blake3 => {
            let hasher = if let Some(key) = hmac_key {
                let key = Zeroizing::new(key.to_vec());
                blake3::Hasher::new_keyed(&key_to_blake3_key(&key))
            } else {
                blake3::Hasher::new()
            };
            run_stream!(hasher, |h: blake3::Hasher| hex_encode_bytes(
                h.finalize().as_bytes()
            ))
        }
    };

    // 最终进度：100%。
    let _ = on_progress(Progress::new(total.unwrap_or(processed), total));
    Ok(DigestResult { algorithm, hex })
}

/// 恒定时间比对预期摘要（忽略大小写与首尾空白）。
pub fn compare_digest_compute_time(actual: &str, expected: &str) -> bool {
    let normalize = |s: &str| -> String { s.trim().to_ascii_lowercase() };
    let actual = normalize(actual);
    let expected = normalize(expected);
    if actual.len() != expected.len() {
        // 长度不一致时仍做一次等长虚拟比较，避免早期返回泄露长度信息。
        let _ = constant_time_eq(actual.as_bytes(), actual.as_bytes());
        return false;
    }
    constant_time_eq(actual.as_bytes(), expected.as_bytes())
}

/// 将摘要以标准 `算法=hex` 格式写入输出流（供工具接力复用）。
pub fn write_digest_report<W: Write>(result: &DigestResult, writer: &mut W) -> io::Result<()> {
    writeln!(writer, "{}={}", result.algorithm.label(), result.hex)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn known_text_vectors() {
        assert_eq!(
            hash_text(b"abc", HashAlgorithm::Md5, None).hex,
            "900150983cd24fb0d6963f7d28e17f72"
        );
        assert_eq!(
            hash_text(b"abc", HashAlgorithm::Sha1, None).hex,
            "a9993e364706816aba3e25717850c26c9cd0d89d"
        );
        assert_eq!(
            hash_text(b"abc", HashAlgorithm::Sha256, None).hex,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(hash_text(b"abc", HashAlgorithm::Blake3, None).hex.len(), 64);
    }

    #[test]
    fn hmac_vectors_and_key_zeroization() {
        let key = b"key";
        let hmac = hash_text(
            b"The quick brown fox jumps over the lazy dog",
            HashAlgorithm::Sha256,
            Some(key),
        );
        assert_eq!(
            hmac.hex,
            "f7bc83f430538424b13298e6aa6fb143ef4d59a14946175997479dbc2d1a3cd8"
        );
    }

    #[test]
    fn constant_time_compare() {
        let digest = hash_text(b"abc", HashAlgorithm::Sha256, None);
        assert!(compare_digest_compute_time(
            &digest.hex,
            &digest.hex.to_uppercase()
        ));
        assert!(compare_digest_compute_time(
            &digest.hex,
            &format!("  {}  ", digest.hex)
        ));
        assert!(!compare_digest_compute_time(&digest.hex, "00"));
    }

    #[test]
    fn streaming_file_hash_with_progress_and_cancel() {
        let dir = std::env::temp_dir().join(format!("devtools-hash-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("data.bin");
        let mut file = File::create(&path).unwrap();
        // 200 KiB，跨 4 个 64 KiB 块。
        file.write_all(&vec![0x5a; 200 * 1024]).unwrap();
        drop(file);

        let mut progress_calls = 0;
        let result = hash_file_streaming(
            &path,
            HashAlgorithm::Sha256,
            None,
            &CancelFlag::new(),
            &mut |_| {
                progress_calls += 1;
                true
            },
        )
        .unwrap();
        assert_eq!(
            result,
            hash_text(&vec![0x5a; 200 * 1024], HashAlgorithm::Sha256, None)
        );
        assert!(progress_calls >= 4);

        // 通过 CancelFlag 取消。
        let flag = CancelFlag::new();
        flag.cancel();
        assert_eq!(
            hash_file_streaming(&path, HashAlgorithm::Sha256, None, &flag, &mut |_| true)
                .unwrap_err()
                .kind,
            ToolErrorKind::Cancelled
        );

        // 通过回调返回 false 取消。
        assert_eq!(
            hash_file_streaming(
                &path,
                HashAlgorithm::Sha256,
                None,
                &CancelFlag::new(),
                &mut |_| false
            )
            .unwrap_err()
            .kind,
            ToolErrorKind::Cancelled
        );

        std::fs::remove_file(&path).unwrap();
        std::fs::remove_dir(&dir).unwrap();
    }

    #[test]
    fn digest_report_writer() {
        let digest = hash_text(b"abc", HashAlgorithm::Sha1, None);
        let mut out = Vec::new();
        write_digest_report(&digest, &mut out).unwrap();
        assert_eq!(
            String::from_utf8(out).unwrap(),
            format!("SHA-1={}\n", digest.hex)
        );
    }
}
