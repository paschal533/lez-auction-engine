//! CLI errors and exit codes (Shared Reference S4, ADR-044). The mapping lives here and only here:
//! `From<SdkError>` classifies, `exit_code` numbers.

use auction_sdk::{error::SdkError, keystore::KeystoreError};

#[derive(Debug, thiserror::Error)]
pub enum CliError {
    #[error("{0}")]
    Usage(String),
    #[error("verification failed: {0}")]
    Verify(SdkError),
    #[error("transaction dropped (not applied by block {height}): nothing was applied; re-quote and resend (spec §6.5)")]
    Dropped { height: u64 },
    #[error("keystore: {0}")]
    Keystore(KeystoreError),
    #[error("{0}")]
    Sdk(SdkError),
    #[error("cannot write output: {0}")]
    Output(String),
}

impl CliError {
    /// 1 usage or other, 2 verification (I4), 3 dropped (re-quote), 4 keystore.
    #[must_use]
    pub fn exit_code(&self) -> u8 {
        match self {
            Self::Usage(_) | Self::Sdk(_) | Self::Output(_) => 1,
            Self::Verify(_) => 2,
            Self::Dropped { .. } => 3,
            Self::Keystore(_) => 4,
        }
    }
}

impl From<SdkError> for CliError {
    fn from(e: SdkError) -> Self {
        match e {
            SdkError::Keystore(k) => Self::Keystore(k),
            e @ SdkError::Mismatch { .. } => Self::Verify(e),
            other => Self::Sdk(other),
        }
    }
}

impl From<KeystoreError> for CliError {
    fn from(e: KeystoreError) -> Self {
        Self::Keystore(e)
    }
}

impl From<std::io::Error> for CliError {
    fn from(e: std::io::Error) -> Self {
        Self::Output(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mismatch() -> SdkError {
        SdkError::Mismatch {
            what: "engine program",
            expected: "a".into(),
            found: "b".into(),
        }
    }

    #[test]
    fn every_error_class_has_its_exit_code() {
        assert_eq!(CliError::Usage("x".into()).exit_code(), 1);
        assert_eq!(CliError::Output("x".into()).exit_code(), 1);
        assert_eq!(CliError::from(SdkError::Chain("x".into())).exit_code(), 1);
        assert_eq!(CliError::from(mismatch()).exit_code(), 2);
        assert_eq!(CliError::Dropped { height: 7 }.exit_code(), 3);
        assert_eq!(
            CliError::from(SdkError::Keystore(KeystoreError::HomeNotSet)).exit_code(),
            4
        );
        assert_eq!(CliError::from(KeystoreError::HomeNotSet).exit_code(), 4);
    }

    #[test]
    fn deployment_file_errors_are_not_verification_failures() {
        let e = SdkError::Deployment {
            path: "d.json".into(),
            reason: "missing".into(),
        };
        assert_eq!(CliError::from(e).exit_code(), 1);
    }
}
