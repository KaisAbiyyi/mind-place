use crate::{Result, StateError};
use sha2::{Digest, Sha256};

/// Canonical referents must come from source evidence, not a disposable
/// index rowid. External keys identify logical sources; raw hashes identify
/// immutable objects/revisions, not mutable conversations.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceKey<'a> {
    External(&'a str),
    RawSha256(&'a str),
}

impl<'a> SourceKey<'a> {
    pub fn parts(self) -> (&'static str, &'a str) {
        match self {
            Self::External(key) => ("external", key),
            Self::RawSha256(hash) => ("raw_sha256", hash),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourceDescriptor<'a> {
    pub provider: &'a str,
    /// Stable account/import namespace. Callers MUST distinguish sources from
    /// different provider accounts or import origins; never use an index ID.
    pub source_namespace: &'a str,
    pub source_kind: &'a str,
    pub key: SourceKey<'a>,
}

pub fn canonical_source_id(source: &SourceDescriptor<'_>) -> Result<String> {
    validate_source(source)?;
    let (key_type, key) = source.key.parts();
    let mut hash = Sha256::new();
    hash.update(b"mind-place:source-identity:v1\0");
    for part in [
        source.provider,
        source.source_namespace,
        source.source_kind,
        key_type,
        key,
    ] {
        // Length framing prevents ambiguous tuples from sharing a hash input.
        hash.update((part.len() as u64).to_be_bytes());
        hash.update(part.as_bytes());
    }
    Ok(format!("src:v1:{:x}", hash.finalize()))
}

pub(crate) fn validate_source(source: &SourceDescriptor<'_>) -> Result<()> {
    for token in [source.provider, source.source_kind] {
        if token.is_empty()
            || token.len() > 64
            || !token
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
        {
            return Err(StateError::InvalidValue);
        }
    }
    let namespace = source.source_namespace;
    if namespace.is_empty()
        || namespace.len() > 256
        || namespace.trim().is_empty()
        || namespace.chars().any(char::is_control)
    {
        return Err(StateError::InvalidValue);
    }
    let (key_type, key) = source.key.parts();
    if key.is_empty()
        || key.len() > 2048
        || key.trim().is_empty()
        || key.chars().any(char::is_control)
    {
        return Err(StateError::InvalidValue);
    }
    if key_type == "raw_sha256"
        && (key.len() != 64
            || !key
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
    {
        return Err(StateError::InvalidValue);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_ids_are_versioned_domain_separated_and_stable() {
        let one = SourceDescriptor {
            provider: "chatgpt",
            source_namespace: "user-account-1",
            source_kind: "conversation",
            key: SourceKey::External("abc"),
        };
        let id = canonical_source_id(&one).unwrap();
        assert!(id.starts_with("src:v1:"));
        assert_eq!(id.len(), 71);
        assert_eq!(id, canonical_source_id(&one).unwrap());
        assert_ne!(
            id,
            canonical_source_id(&SourceDescriptor {
                source_namespace: "user-account-2",
                ..one
            })
            .unwrap()
        );
        assert_ne!(
            id,
            canonical_source_id(&SourceDescriptor {
                provider: "claude",
                ..one
            })
            .unwrap()
        );
        assert_ne!(
            id,
            canonical_source_id(&SourceDescriptor {
                source_kind: "message",
                ..one
            })
            .unwrap()
        );
        assert_ne!(
            id,
            canonical_source_id(&SourceDescriptor {
                key: SourceKey::RawSha256(&"a".repeat(64)),
                ..one
            })
            .unwrap()
        );
    }

    #[test]
    fn rejects_unstable_or_unqualified_source_keys() {
        let base = SourceDescriptor {
            provider: "chatgpt",
            source_namespace: "account-a",
            source_kind: "conversation",
            key: SourceKey::External("conversation-1"),
        };
        assert!(canonical_source_id(&SourceDescriptor {
            provider: "CHATGPT",
            ..base
        })
        .is_err());
        assert!(canonical_source_id(&SourceDescriptor {
            source_namespace: "",
            ..base
        })
        .is_err());
        assert!(canonical_source_id(&SourceDescriptor {
            key: SourceKey::External("line\nfeed"),
            ..base
        })
        .is_err());
        assert!(canonical_source_id(&SourceDescriptor {
            key: SourceKey::RawSha256("not-a-sha"),
            ..base
        })
        .is_err());
    }
}
