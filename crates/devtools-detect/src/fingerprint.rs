//! 内存态摘要只用于去重，不是密码学身份或持久化标识。

use devtools_core::{ClipboardFingerprint, RawPayload};
use std::{
    collections::hash_map::RandomState,
    hash::{BuildHasher, Hash, Hasher},
    sync::Arc,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DedupDecision {
    Detect,
    Unchanged,
    SelfCopy,
}

#[derive(Debug, Default)]
pub struct ClipboardDeduplicator {
    hasher: RandomState,
    last: Option<ClipboardFingerprint>,
    self_copy: Option<u64>,
}

impl ClipboardDeduplicator {
    pub fn fingerprint(&self, sequence: u64, payload: &RawPayload) -> ClipboardFingerprint {
        let mut state = self.hasher.build_hasher();
        std::mem::discriminant(payload).hash(&mut state);
        let file_hint = match payload {
            RawPayload::Text(text) => {
                text.hash(&mut state);
                None
            }
            RawPayload::Image(image) => {
                image.width.hash(&mut state);
                image.height.hash(&mut state);
                image.bytes.hash(&mut state);
                None
            }
            RawPayload::Files(paths) => {
                paths.hash(&mut state);
                paths.first().cloned().map(Arc::new)
            }
        };
        ClipboardFingerprint {
            sequence,
            digest: state.finish(),
            file_hint,
        }
    }

    /// 在复制写入成功后登记；仅抑制当前连续内容，其他内容出现后即失效。
    pub fn mark_self_copy(&mut self, payload: &RawPayload) {
        self.self_copy = Some(self.fingerprint(0, payload).digest);
    }

    pub fn observe(&mut self, sequence: u64, payload: &RawPayload) -> DedupDecision {
        let fingerprint = self.fingerprint(sequence, payload);
        let decision = if self.self_copy == Some(fingerprint.digest) {
            DedupDecision::SelfCopy
        } else if self
            .last
            .as_ref()
            .is_some_and(|last| last.digest == fingerprint.digest)
        {
            DedupDecision::Unchanged
        } else {
            self.self_copy = None;
            DedupDecision::Detect
        };
        self.last = Some(fingerprint);
        decision
    }
}
