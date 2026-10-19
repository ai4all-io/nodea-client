// What the node has downloaded, and the registry key the backend knows each
// file by.
//
// This used to be a table of models compiled into the binary, mirroring the
// backend's own. It existed for one reason: `LoadModel` named a GGUF file, but
// everything the node reports back (`ModelLoadStatus.model_ref`,
// `DeviceIdentity.loaded_model_ref` / `cached_model_refs`) must name the
// registry key, and only such a table could turn one into the other. The cost
// was that a model the build predates could not be named at all, so anything
// an organization added from Hugging Face would load and never become ready.
//
// `LoadModel.model_key` now carries the key, so nothing has to be derived and
// no model needs a release. The node only has to remember what it was told,
// because `cached_model_refs` still has to name files that were downloaded
// during some earlier run - possibly before a restart. That is this manifest:
// a small JSON file beside the models, written when a load is commanded and
// read back on connect.
//
// The key is never guessed. A file with no entry is simply not reported:
// claiming the wrong key does not fail loudly, it leaves the node silently
// unroutable, which is far more expensive than a model the backend has to
// send again.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const MANIFEST: &str = "model-keys.json";

fn manifest_path(cache_dir: &Path) -> PathBuf {
    cache_dir.join(MANIFEST)
}

// `LoadModel.model_name` and `model_ref` come from the backend and end up in
// `cache_dir.join(model_name)` and a Hugging Face URL. Unchecked, a name like
// `../../Library/LaunchAgents/x.plist` (or an absolute path) would write a
// download anywhere the user can write. So both are held to the shape a real
// registry entry has: a plain `*.gguf` file name, and an `owner/repo` id.

fn is_safe_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment != "."
        && segment != ".."
        && segment
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

/// Ok for a plain GGUF file name that stays inside the cache directory.
pub fn validate_model_file_name(name: &str) -> Result<(), String> {
    let ok = is_safe_segment(name)
        && !name.starts_with('.')
        && name.to_ascii_lowercase().ends_with(".gguf");
    if ok {
        Ok(())
    } else {
        Err(format!(
            "Refusing model file name {:?}: not a plain .gguf file name",
            name
        ))
    }
}

/// Ok for a Hugging Face `owner/repo` id.
pub fn validate_model_ref(model_ref: &str) -> Result<(), String> {
    let ok = match model_ref.split_once('/') {
        Some((owner, repo)) => is_safe_segment(owner) && is_safe_segment(repo),
        None => false,
    };
    if ok {
        Ok(())
    } else {
        Err(format!(
            "Refusing model ref {:?}: not an owner/repo id",
            model_ref
        ))
    }
}

/// Every known filename -> registry key pair. Empty when the node has never
/// been told to load anything, and on any unreadable or corrupt manifest: the
/// backend re-sends `LoadModel`, so a lost manifest costs a reload, never
/// correctness.
pub fn known_keys(cache_dir: &Path) -> BTreeMap<String, String> {
    std::fs::read_to_string(manifest_path(cache_dir))
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

/// Remember that `filename` is the file for `key`, so it can still be named
/// after a restart. Best effort: a failed write costs a reload, so it is
/// logged rather than propagated into the load path.
pub fn remember(cache_dir: &Path, filename: &str, key: &str) {
    if key.is_empty() || validate_model_file_name(filename).is_err() {
        return;
    }
    let mut keys = known_keys(cache_dir);
    if keys.get(filename).map(String::as_str) == Some(key) {
        return;
    }
    keys.insert(filename.to_string(), key.to_string());

    if let Err(e) = std::fs::create_dir_all(cache_dir) {
        eprintln!("Could not create {}: {}", cache_dir.display(), e);
        return;
    }
    match serde_json::to_string_pretty(&keys) {
        Ok(body) => {
            if let Err(e) = std::fs::write(manifest_path(cache_dir), body) {
                eprintln!("Could not write the model key manifest: {}", e);
            }
        }
        Err(e) => eprintln!("Could not serialise the model key manifest: {}", e),
    }
}

/// The registry key for a GGUF filename, or None for a file this node was
/// never told the key for.
pub fn registry_key(cache_dir: &Path, filename: &str) -> Option<String> {
    known_keys(cache_dir).remove(filename)
}

/// Registry keys of the models actually downloaded into `cache_dir`.
///
/// Intersected with the files on disk, so a model the user deleted by hand is
/// not offered, and a manifest entry that outlived its file is ignored. An
/// entry that is not a plain file name (written before names were checked) is
/// never joined to `cache_dir` at all.
pub fn cached_model_refs(cache_dir: &Path) -> Vec<String> {
    known_keys(cache_dir)
        .into_iter()
        .filter(|(filename, _)| validate_model_file_name(filename).is_ok())
        .filter(|(filename, _)| cache_dir.join(filename).exists())
        .map(|(_, key)| key)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("nodea-catalogue-{}-{}", name, std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn remembers_a_key_across_reads() {
        let dir = temp_dir("roundtrip");
        remember(
            &dir,
            "Qwen3.5-4B-Q4_K_M.gguf",
            "unsloth/Qwen3.5-4B-GGUF:Q4_K_M",
        );

        assert_eq!(
            registry_key(&dir, "Qwen3.5-4B-Q4_K_M.gguf").as_deref(),
            Some("unsloth/Qwen3.5-4B-GGUF:Q4_K_M"),
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_file_it_was_never_told_about_has_no_key() {
        let dir = temp_dir("unknown");
        assert_eq!(registry_key(&dir, "mystery.gguf"), None);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn cached_refs_skip_a_remembered_file_that_is_gone() {
        let dir = temp_dir("cached");
        std::fs::write(dir.join("here.gguf"), b"x").unwrap();
        remember(&dir, "here.gguf", "owner/here-GGUF:Q4_K_M");
        remember(&dir, "deleted.gguf", "owner/deleted-GGUF:Q4_K_M");

        assert_eq!(cached_model_refs(&dir), vec!["owner/here-GGUF:Q4_K_M"]);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn accepts_real_registry_file_names() {
        for name in [
            "Llama-3.2-1B-Instruct-Q4_K_M.gguf",
            "Qwen2.5-Coder-7B-Instruct-Q4_K_M.gguf",
            "Qwen3.5-4B-Q4_K_M.gguf",
            "UPPER.GGUF",
        ] {
            assert!(validate_model_file_name(name).is_ok(), "{}", name);
        }
    }

    #[test]
    fn rejects_file_names_that_could_leave_the_cache_dir() {
        for name in [
            "",
            ".",
            "..",
            "../x.gguf",
            "../../Library/LaunchAgents/x.plist",
            "/abs.gguf",
            "a/b.gguf",
            "a\\b.gguf",
            "C:\\x.gguf",
            ".hidden.gguf",
            "x.bin",
            "x.gguf?y",
            "x.gguf#y",
            "x%2F.gguf",
            "x\0.gguf",
            "x .gguf",
        ] {
            assert!(validate_model_file_name(name).is_err(), "{:?}", name);
        }
    }

    #[test]
    fn accepts_owner_repo_refs() {
        for model_ref in [
            "bartowski/Llama-3.2-1B-Instruct-GGUF",
            "unsloth/Qwen3.5-4B-GGUF",
            "a.b/c_d-e",
        ] {
            assert!(validate_model_ref(model_ref).is_ok(), "{}", model_ref);
        }
    }

    #[test]
    fn rejects_refs_that_are_not_owner_repo() {
        for model_ref in [
            "",
            "owner",
            "/repo",
            "owner/",
            "a/b/c",
            "../x",
            "a/..",
            "./b",
            "a/b?x",
            "a/b#x",
            "a/b%2F..",
            "https://evil.example/a",
        ] {
            assert!(validate_model_ref(model_ref).is_err(), "{:?}", model_ref);
        }
    }

    #[test]
    fn remember_ignores_an_unsafe_file_name() {
        let dir = temp_dir("unsafe-remember");
        remember(&dir, "../escape.gguf", "owner/x-GGUF:Q4_K_M");

        assert!(known_keys(&dir).is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn cached_refs_skip_a_poisoned_manifest_entry() {
        // A manifest written before names were checked could hold a path
        // that exists outside the cache dir; it must not be reported.
        let dir = temp_dir("poisoned");
        let outside = dir.join("outside.gguf");
        std::fs::write(&outside, b"x").unwrap();
        let cache = dir.join("models");
        std::fs::create_dir_all(&cache).unwrap();
        std::fs::write(
            manifest_path(&cache),
            r#"{"../outside.gguf": "owner/outside-GGUF:Q4_K_M"}"#,
        )
        .unwrap();

        assert_eq!(cached_model_refs(&cache), Vec::<String>::new());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_corrupt_manifest_reads_as_empty_rather_than_panicking() {
        let dir = temp_dir("corrupt");
        std::fs::write(manifest_path(&dir), b"not json").unwrap();

        assert!(known_keys(&dir).is_empty());
        assert_eq!(cached_model_refs(&dir), Vec::<String>::new());
        std::fs::remove_dir_all(&dir).ok();
    }
}
