




use std::ops::Deref;


const ENTROPY: &[u8] = b"Talo/provider-credential/v1";



pub struct Secret(Vec<u8>);

impl Secret {
    pub fn new(value: String) -> Self {
        Self(value.into_bytes())
    }

    pub fn as_str(&self) -> Result<&str, String> {
        std::str::from_utf8(&self.0).map_err(|_| "Stored credential is not valid text".to_string())
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl Drop for Secret {
    fn drop(&mut self) {


        for byte in &mut self.0 {
            unsafe { std::ptr::write_volatile(byte, 0) };
        }
    }
}

impl Deref for Secret {
    type Target = [u8];
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[cfg(windows)]
mod platform {
    use super::{Secret, ENTROPY};
    use std::ptr;
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::Cryptography::{
            CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
        },
    };

    fn blob(data: &[u8]) -> CRYPT_INTEGER_BLOB {
        CRYPT_INTEGER_BLOB {
            cbData: data.len() as u32,
            pbData: data.as_ptr() as *mut u8,
        }
    }

    fn take_output(output: CRYPT_INTEGER_BLOB) -> Vec<u8> {
        if output.pbData.is_null() || output.cbData == 0 {
            return Vec::new();
        }
        let bytes =
            unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec() };
        unsafe {
            LocalFree(output.pbData as _);
        }
        bytes
    }

    pub fn protect(plaintext: &[u8]) -> Result<Vec<u8>, String> {
        if plaintext.is_empty() {
            return Err("A credential cannot be empty".into());
        }
        let input = blob(plaintext);
        let entropy = blob(ENTROPY);
        let mut output = CRYPT_INTEGER_BLOB::default();
        let description = windows_sys::core::w!("Talo provider credential");
        let ok = unsafe {
            CryptProtectData(
                &input,
                description,
                &entropy,
                ptr::null(),
                ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        };
        if ok == 0 {
            return Err(format!(
                "Windows could not protect the credential (error {})",
                std::io::Error::last_os_error()
            ));
        }
        Ok(take_output(output))
    }

    pub fn unprotect(protected: &[u8]) -> Result<Secret, String> {
        if protected.is_empty() {
            return Err("No credential is stored for this connection".into());
        }
        let input = blob(protected);
        let entropy = blob(ENTROPY);
        let mut output = CRYPT_INTEGER_BLOB::default();
        let ok = unsafe {
            CryptUnprotectData(
                &input,
                ptr::null_mut(),
                &entropy,
                ptr::null(),
                ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        };
        if ok == 0 {
            return Err(format!(
                "Windows could not unlock the stored credential (error {})",
                std::io::Error::last_os_error()
            ));
        }
        Ok(Secret(take_output(output)))
    }
}

#[cfg(not(windows))]
mod platform {
    use super::Secret;
    pub fn protect(_plaintext: &[u8]) -> Result<Vec<u8>, String> {
        Err("Secure credential storage is available on Windows only".into())
    }
    pub fn unprotect(_protected: &[u8]) -> Result<Secret, String> {
        Err("Secure credential storage is available on Windows only".into())
    }
}

pub use platform::{protect, unprotect};

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn dpapi_round_trip_and_rejection() {
        let secret = Secret::new("sk-test-not-a-real-key-1234567890".into());
        let sealed = protect(secret.as_str().unwrap().as_bytes()).unwrap();
        assert_ne!(sealed.as_slice(), &secret[..]);
        let opened = unprotect(&sealed).unwrap();
        assert_eq!(
            opened.as_str().unwrap(),
            "sk-test-not-a-real-key-1234567890"
        );
        assert!(unprotect(b"not a dpapi blob").is_err());
    }

    #[test]
    fn empty_credentials_are_rejected() {
        assert!(protect(b"").is_err());
    }
}
