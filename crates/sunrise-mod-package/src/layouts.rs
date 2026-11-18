//! Checked layout fingerprints recorded by the mod compiler.
use serde::Deserialize;
use std::{collections::BTreeMap, io::Read};

struct Unique<V>(BTreeMap<String, V>);
impl<'de, V: Deserialize<'de>> Deserialize<'de> for Unique<V> {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct Visitor<V>(std::marker::PhantomData<V>);
        impl<'de, V: Deserialize<'de>> serde::de::Visitor<'de> for Visitor<V> {
            type Value = Unique<V>;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("an object with unique keys")
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut a: A,
            ) -> Result<Self::Value, A::Error> {
                let mut map = BTreeMap::new();
                while let Some((key, value)) = a.next_entry::<String, V>()? {
                    if map.insert(key.clone(), value).is_some() {
                        return Err(serde::de::Error::custom(format!("duplicate key {key}")));
                    }
                }
                Ok(Unique(map))
            }
        }
        d.deserialize_map(Visitor(std::marker::PhantomData))
    }
}

pub const FILE: &str = "layouts.json";
const MAX_BYTES: u64 = 1 << 20;

/// Type fingerprints indexed by owning mod and fully qualified type name.
#[derive(Clone, Debug, Default)]
pub struct Layouts {
    types: BTreeMap<String, BTreeMap<String, String>>,
    /// A file a build before 32-digit identities wrote: 16 hex digits per type. It is read so
    /// that the package reaches its own refusal (an old wasm module is refused at load, an old
    /// source folder is rebuilt), never trusted: its digests equal no current one.
    legacy: bool,
}

impl Layouts {
    /// Validate untrusted package metadata once, with a bounded read.
    pub fn read(reader: impl Read) -> Result<Self, String> {
        let mut bytes = Vec::new();
        reader
            .take(MAX_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| format!("{FILE}: {e}"))?;
        if bytes.len() as u64 > MAX_BYTES {
            return Err(format!("{FILE}: exceeds 1 MiB"));
        }
        let parsed: Unique<Unique<String>> =
            serde_json::from_slice(&bytes).map_err(|e| format!("{FILE} is malformed: {e}"))?;
        let maps: BTreeMap<_, _> = parsed.0.into_iter().map(|(k, v)| (k, v.0)).collect();
        let mut legacy = false;
        for (owner, types) in &maps {
            for (name, hash) in types {
                let hex = hash
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
                match hash.len() {
                    32 if hex => {}
                    16 if hex => legacy = true,
                    _ => {
                        return Err(format!(
                            "{FILE}: {owner} type {name}: expected 32 lowercase hex digits"
                        ));
                    }
                }
            }
        }
        Ok(Self {
            types: maps,
            legacy,
        })
    }

    /// Whether this is a file of the 16-digit format: a build writes it again.
    pub fn is_legacy(&self) -> bool {
        self.legacy
    }

    /// First recorded library type whose layout changed, ordered by type name.
    pub fn first_difference<'a>(&'a self, library: &str, actual: &Self) -> Option<&'a str> {
        self.types.get(library)?.iter().find_map(|(name, hash)| {
            (actual.types.get(library).and_then(|types| types.get(name)) != Some(hash))
                .then_some(name.as_str())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn the_old_sixteen_digit_format_is_read_as_legacy_and_equals_no_current_digest() {
        let old = Layouts::read(&br#"{"lib":{"T":"0123456789abcdef"}}"#[..]).unwrap();
        assert!(old.is_legacy());
        let current =
            Layouts::read(&br#"{"lib":{"T":"0123456789abcdef0123456789abcdef"}}"#[..]).unwrap();
        assert!(!current.is_legacy());
        assert_eq!(old.first_difference("lib", &current), Some("T"));
    }

    #[test]
    fn malformed_metadata_is_refused() {
        for bytes in [
            b"{".as_slice(),
            br#"{"lib":{},"lib":{}}"#,
            br#"{"lib":{"T":"0123456789abcdef0123456789abcdef","T":"0123456789abcdef0123456789abcdef"}}"#,
            br#"{"lib":{"T":"ABCDEF0123456789"}}"#,
            br#"{"lib":{"T":12}}"#,
            br#"{"lib":{"T":"0123"}}"#,
            br#"{"lib":{"T":"0123456789abcdeg"}}"#,
        ] {
            assert!(Layouts::read(bytes).unwrap_err().starts_with(FILE));
        }
        assert!(
            Layouts::read(vec![b' '; MAX_BYTES as usize + 1].as_slice())
                .unwrap_err()
                .contains("exceeds")
        );
    }
}

/// Refuse a dependency crossing the two execution targets before graph resolution.
pub fn mixed_refusal(dependent: &crate::Package, library: &crate::Package) -> Option<String> {
    let dep = &dependent.manifest;
    let lib = &library.manifest;
    if !dep.dependencies.contains_key(&lib.id)
        || !((dep.native.is_some() && lib.wasm.is_some())
            || (dep.wasm.is_some() && lib.native.is_some()))
    {
        return None;
    }
    if let Some(name) = dependent
        .layouts
        .first_difference(&lib.id, &library.layouts)
    {
        return Some(format!(
            "{}: layout mismatch in library {} for type {name}",
            dep.id, lib.id
        ));
    }
    let (library_target, dependent_target) = if lib.native.is_some() {
        ("native", "wasm")
    } else {
        ("wasm", "native")
    };
    Some(format!(
        "{} is a {library_target} mod; {dependent_target} mod {} depends on it: mixed native and wasm dependencies aren't supported yet",
        lib.id, dep.id
    ))
}

#[cfg(test)]
mod edge_tests {
    use super::*;
    fn package(id: &str, native: bool, dependency: bool, hash: &str) -> crate::Package {
        let mut text = format!(
            "id='{id}'\nname='{id}'\nversion='1'\napi='0.2'\ncapabilities=['registries']\n"
        );
        if dependency {
            text += "[dependencies]\nlib='*'\n";
        }
        if native {
            text += &format!(
                "[native]\ngame='26.3'\nsunrise='{}'\nmixins='{}'\n",
                "a".repeat(40),
                "b".repeat(40)
            );
        } else {
            text += &format!(
                "[wasm]\nsha512='{}'\ntoolchain='t'\nbuilt_with='t'\n",
                "a".repeat(128)
            );
        }
        crate::Package {
            manifest: crate::Manifest::parse(&text).unwrap(),
            icon: None,
            layouts: Layouts::read(
                format!(r#"{{"lib":{{"Z":"{hash}","A":"{hash}"}}}}"#).as_bytes(),
            )
            .unwrap(),
        }
    }
    fn check(native: bool, mismatch: bool) {
        let dependent = package("addon", native, true, "0123456789abcdef0123456789abcdef");
        let library = package(
            "lib",
            !native,
            false,
            if mismatch {
                "1123456789abcdef1123456789abcdef"
            } else {
                "0123456789abcdef0123456789abcdef"
            },
        );
        let reason = mixed_refusal(&dependent, &library).unwrap();
        if mismatch {
            assert_eq!(reason, "addon: layout mismatch in library lib for type A");
        } else {
            let (lib_target, dep_target) = if native {
                ("wasm", "native")
            } else {
                ("native", "wasm")
            };
            assert_eq!(
                reason,
                format!(
                    "lib is a {lib_target} mod; {dep_target} mod addon depends on it: mixed native and wasm dependencies aren't supported yet"
                )
            );
        }
    }
    #[test]
    fn same_target_and_undeclared_edges_are_not_mixed() {
        let mut dependent = package("addon", true, true, "0123456789abcdef0123456789abcdef");
        let library = package("lib", true, false, "1123456789abcdef1123456789abcdef");
        assert!(mixed_refusal(&dependent, &library).is_none());
        dependent.manifest.dependencies.clear();
        let library = package("lib", false, false, "1123456789abcdef1123456789abcdef");
        assert!(mixed_refusal(&dependent, &library).is_none());
    }
    #[test]
    fn absent_recordings_need_no_layout_check_but_missing_library_type_does() {
        let mut dependent = package("addon", true, true, "0123456789abcdef0123456789abcdef");
        let mut library = package("lib", false, false, "0123456789abcdef0123456789abcdef");
        library.layouts = Layouts::default();
        assert!(
            mixed_refusal(&dependent, &library)
                .unwrap()
                .contains("type A")
        );
        dependent.layouts = Layouts::default();
        assert!(
            mixed_refusal(&dependent, &library)
                .unwrap()
                .contains("aren't supported yet")
        );
    }
    #[test]
    fn wasm_native_mismatch() {
        check(false, true);
    }
    #[test]
    fn wasm_native_match() {
        check(false, false);
    }
    #[test]
    fn native_wasm_mismatch() {
        check(true, true);
    }
    #[test]
    fn native_wasm_match() {
        check(true, false);
    }
}
