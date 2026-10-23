use std::{
    env, fs,
    path::PathBuf,
};

const CONFIG_FILE: &str = "google-oauth.json";
const LOCAL_FILE: &str = "google-oauth.local.json";
const JSON_VAR: &str = "TALO_GOOGLE_OAUTH_JSON";
const PATH_VAR: &str = "TALO_GOOGLE_OAUTH_CLIENT";

fn read(path: &str) -> Option<String> {
    match fs::read_to_string(path) {
        Ok(text) if !text.trim().is_empty() => Some(text.trim().to_string()),
        Ok(_) => None,
        Err(error) => panic!("{PATH_VAR} could not be read: {path} ({error})"),
    }
}

fn embed_google_client() {
    println!("cargo:rerun-if-env-changed={JSON_VAR}");
    println!("cargo:rerun-if-env-changed={PATH_VAR}");
    println!("cargo:rerun-if-changed=build.rs");

    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let local = manifest.join(LOCAL_FILE);
    println!("cargo:rerun-if-changed={}", local.display());

    let value = match env::var(JSON_VAR) {
        Ok(text) if !text.trim().is_empty() => Some(text.trim().to_string()),
        _ => match env::var(PATH_VAR) {
            Ok(path) if !path.trim().is_empty() => read(path.trim()),
            _ if local.exists() => read(&local.to_string_lossy()),
            _ => None,
        },
    };

    let out = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR is set by cargo"));
    fs::write(
        out.join(CONFIG_FILE),
        value.unwrap_or_else(|| "null".to_string()),
    )
    .expect("write the embedded Google OAuth client");
}

fn main() {
    embed_google_client();
    tauri_build::build()
}
