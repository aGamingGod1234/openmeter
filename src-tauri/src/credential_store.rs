use std::fmt;

use windows::core::{HRESULT, PCWSTR, PWSTR};
use windows::Win32::Foundation::ERROR_NOT_FOUND;
use windows::Win32::Security::Credentials::{
    CredDeleteW, CredFree, CredReadW, CredWriteW, CREDENTIALW, CRED_PERSIST_LOCAL_MACHINE,
    CRED_TYPE_GENERIC,
};

pub const HISTORY_KEY_TARGET: &str = "OpenMeter/sync/history-key";
pub const DEVICE_CREDENTIAL_TARGET: &str = "OpenMeter/sync/device-credential";

pub struct SecretVec(Vec<u8>);

impl SecretVec {
    pub fn new(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }

    pub fn expose(&self) -> &[u8] {
        &self.0
    }
}

impl fmt::Debug for SecretVec {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("[secret]")
    }
}

impl Drop for SecretVec {
    fn drop(&mut self) {
        self.0.fill(0);
    }
}

#[derive(Debug)]
pub struct CredentialError(&'static str);

impl fmt::Display for CredentialError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0)
    }
}

impl std::error::Error for CredentialError {}

pub struct CredentialStore;

impl CredentialStore {
    pub fn write(target: &str, secret: &SecretVec) -> Result<(), CredentialError> {
        validate_target(target)?;
        if secret.expose().is_empty() || secret.expose().len() > 2_560 {
            return Err(CredentialError("credential has an invalid size"));
        }
        let mut target = wide(target);
        let mut username = wide("OpenMeter");
        let blob_size = u32::try_from(secret.expose().len())
            .map_err(|_| CredentialError("credential has an invalid size"))?;
        let credential = CREDENTIALW {
            Type: CRED_TYPE_GENERIC,
            TargetName: PWSTR(target.as_mut_ptr()),
            CredentialBlobSize: blob_size,
            CredentialBlob: secret.expose().as_ptr().cast_mut(),
            Persist: CRED_PERSIST_LOCAL_MACHINE,
            UserName: PWSTR(username.as_mut_ptr()),
            ..Default::default()
        };
        unsafe { CredWriteW(&credential, 0) }
            .map_err(|_| CredentialError("Windows Credential Manager write failed"))
    }

    pub fn read(target: &str) -> Result<Option<SecretVec>, CredentialError> {
        validate_target(target)?;
        let target = wide(target);
        let mut pointer = std::ptr::null_mut::<CREDENTIALW>();
        let result = unsafe {
            CredReadW(
                PCWSTR(target.as_ptr()),
                CRED_TYPE_GENERIC,
                None,
                &mut pointer,
            )
        };
        if let Err(error) = result {
            if error.code() == HRESULT::from_win32(ERROR_NOT_FOUND.0) {
                return Ok(None);
            }
            return Err(CredentialError("Windows Credential Manager read failed"));
        }
        if pointer.is_null() {
            return Err(CredentialError(
                "Windows Credential Manager returned no data",
            ));
        }
        let credential = unsafe { &*pointer };
        let bytes = if credential.CredentialBlobSize == 0 {
            Vec::new()
        } else if credential.CredentialBlob.is_null() {
            unsafe { CredFree(pointer.cast()) };
            return Err(CredentialError(
                "Windows Credential Manager returned invalid data",
            ));
        } else {
            unsafe {
                std::slice::from_raw_parts(
                    credential.CredentialBlob,
                    credential.CredentialBlobSize as usize,
                )
                .to_vec()
            }
        };
        unsafe { CredFree(pointer.cast()) };
        Ok(Some(SecretVec::new(bytes)))
    }

    pub fn delete(target: &str) -> Result<(), CredentialError> {
        validate_target(target)?;
        let target = wide(target);
        match unsafe { CredDeleteW(PCWSTR(target.as_ptr()), CRED_TYPE_GENERIC, None) } {
            Ok(()) => Ok(()),
            Err(error) if error.code() == HRESULT::from_win32(ERROR_NOT_FOUND.0) => Ok(()),
            Err(_) => Err(CredentialError("Windows Credential Manager delete failed")),
        }
    }
}

fn validate_target(target: &str) -> Result<(), CredentialError> {
    if !target.starts_with("OpenMeter/") || target.contains('\0') || target.len() > 256 {
        return Err(CredentialError("credential target is invalid"));
    }
    Ok(())
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}
