//! A module's import table against the game's.
//!
//! A built wasm mod's imports of module `sunrise` are the exact list of what it uses of the game:
//! each host call it keeps, each hook, Part or handler method and point it implements, and the
//! shared protocol, named `<kind>:<owner>::<method>[@<version>]#<hash>`
//! (`mod_surface::call`). The game's table is generated from the same source by the same
//! function, so loading is a comparison of names: a name the game has binds, one it has with
//! another hash is a changed item, one it lacks is a removed item. Nothing is hashed here and no
//! guest code has run when [`check`] answers.
use crate::boundary::{CALLS, EXPORT_CALLS};
use mod_surface::call::{Kind, MODULE, Name, SHARED_PROTOCOL, protocol_name};
use std::fmt;

/// How many mismatches a refusal shows a player before it counts the rest; the log lists all.
pub(crate) const LISTED: usize = 8;

static GAME_BUILD: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// Records the game's build id, which names the SDK a refused mod is rebuilt with
/// (`sdk-<build id>`). The game sets it once before loading mods.
pub fn set_game_build(id: &str) {
    let _ = GAME_BUILD.set(id.to_owned());
}

/// What the game serves a name with.
pub(crate) enum Bound {
    /// A host call: the generated function that answers it.
    Call(crate::boundary::Invoke),
    /// A marker of an export the mod implements: nothing to run.
    Mark,
    /// The shared protocol's words.
    HostReply,
    Release,
    Log,
}

/// One import the game does not serve as the mod built it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mismatch {
    /// What the mod uses, as a refusal names it (`host:ServerHost::give_item`).
    pub item: String,
    pub why: Why,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Why {
    /// The game has the item, with another signature (or hook version).
    Changed {
        built: Option<u32>,
        game: Option<u32>,
        built_hash: String,
        game_hash: String,
    },
    /// The game has no such item.
    Missing,
    /// The name is not one the game's generator writes.
    Malformed(String),
}

impl fmt::Display for Mismatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.why {
            Why::Changed {
                built,
                game,
                built_hash,
                game_hash,
            } => {
                write!(f, "{} changed", self.item)?;
                if let (Some(built), Some(game)) = (built, game)
                    && built != game
                {
                    write!(f, " (version {built} → {game})")
                } else {
                    write!(
                        f,
                        " (mod #{}, game #{})",
                        &built_hash[..8.min(built_hash.len())],
                        &game_hash[..8.min(game_hash.len())]
                    )
                }
            }
            Why::Missing => write!(f, "{} is not in this game", self.item),
            Why::Malformed(reason) => write!(f, "{}: {reason}", self.item),
        }
    }
}

/// Finds the entry of `table` (names, sorted) for `kind:path`, whatever its version and hash.
fn find<'t>(table: impl Fn(usize) -> &'t str, len: usize, kind: Kind, path: &str) -> Option<usize> {
    let head = format!("{}:{path}", kind.word());
    let start = partition(&table, len, |name| name < head.as_str());
    (start..len)
        .take_while(|&i| table(i).starts_with(&head))
        .find(|&i| matches!(table(i).as_bytes().get(head.len()), Some(b'@' | b'#')))
}

fn partition<'t>(
    table: &impl Fn(usize) -> &'t str,
    len: usize,
    before: impl Fn(&str) -> bool,
) -> usize {
    let (mut low, mut high) = (0, len);
    while low < high {
        let mid = low + (high - low) / 2;
        if before(table(mid)) {
            low = mid + 1;
        } else {
            high = mid;
        }
    }
    low
}

/// What the game serves a name with, before the table's row is turned into its function.
enum Found {
    /// The row of the call table.
    Call(usize),
    Mark,
    HostReply,
    Release,
    Log,
}

/// What `name`, an import of module `sunrise`, is to this game.
pub(crate) fn resolve(name: &str) -> Result<Bound, Mismatch> {
    resolve_in(name, &|i| CALLS[i].name, CALLS.len()).map(|found| match found {
        Found::Call(at) => Bound::Call(CALLS[at].invoke),
        Found::Mark => Bound::Mark,
        Found::HostReply => Bound::HostReply,
        Found::Release => Bound::Release,
        Found::Log => Bound::Log,
    })
}

/// [`resolve`] against a call table given as `len` names, sorted.
fn resolve_in<'t>(
    name: &str,
    calls: &dyn Fn(usize) -> &'t str,
    len: usize,
) -> Result<Found, Mismatch> {
    let built = Name::parse(name).map_err(|error| Mismatch {
        item: shown(name),
        why: Why::Malformed(error.to_string()),
    })?;
    let changed = |game: &str| {
        let game = Name::parse(game).expect("the game's table holds names");
        Mismatch {
            item: built.item(),
            why: Why::Changed {
                built: built.version,
                game: game.version,
                built_hash: built.hash.clone(),
                game_hash: game.hash,
            },
        }
    };
    let missing = || Mismatch {
        item: built.item(),
        why: Why::Missing,
    };
    match built.kind {
        Kind::Protocol => {
            let expected = protocol_name(&built.path, SHARED_PROTOCOL);
            let found = match built.path.as_str() {
                "host_reply" => Found::HostReply,
                "release" => Found::Release,
                "log" => Found::Log,
                _ => return Err(missing()),
            };
            if expected.text() == name {
                Ok(found)
            } else {
                Err(Mismatch {
                    item: built.item(),
                    why: Why::Changed {
                        built: None,
                        game: None,
                        built_hash: built.hash.clone(),
                        game_hash: expected.hash,
                    },
                })
            }
        }
        Kind::Call => match find(calls, len, Kind::Call, &built.path) {
            Some(at) if calls(at) == name => Ok(Found::Call(at)),
            Some(at) => Err(changed(calls(at))),
            None => Err(missing()),
        },
        Kind::Hook | Kind::Part => {
            match find(
                |i| EXPORT_CALLS[i],
                EXPORT_CALLS.len(),
                built.kind,
                &built.path,
            ) {
                Some(at) if EXPORT_CALLS[at] == name => Ok(Found::Mark),
                Some(at) => Err(changed(EXPORT_CALLS[at])),
                None => Err(missing()),
            }
        }
        Kind::Point => {
            let points = mod_points::POINT_CALLS;
            match find(|i| points[i], points.len(), Kind::Point, &built.path) {
                Some(at) if points[at] == name => Ok(Found::Mark),
                Some(at) => Err(changed(points[at])),
                None => Err(missing()),
            }
        }
    }
}

/// The mismatches of a module's imports against this game's table with one more call in it: a
/// method added to the API since the mod was built. A mod that uses none of it has none.
#[cfg(feature = "runtime-fixture")]
pub(crate) fn mismatches_with_added_call(
    module: &[u8],
    added: &str,
) -> Result<Vec<Mismatch>, String> {
    let mut table: Vec<&str> = CALLS.iter().map(|call| call.name).collect();
    table.push(added);
    table.sort_unstable();
    let mut found = Vec::new();
    for name in sunrise_imports(module)? {
        if let Err(mismatch) = resolve_in(&name, &|i| table[i], table.len()) {
            found.push(mismatch);
        }
    }
    Ok(found)
}

/// A name for a message: bounded, whatever the module wrote.
fn shown(name: &str) -> String {
    let mut text: String = name.chars().take(60).collect();
    if text.len() < name.len() {
        text.push('…');
    }
    format!("import \"{text}\"")
}

/// Reads one member's imports of the game's module; others are the group linker's.
pub(crate) fn sunrise_imports(module: &[u8]) -> Result<Vec<String>, String> {
    let mut found = Vec::new();
    for payload in wasmparser::Parser::new(0).parse_all(module) {
        match payload.map_err(|e| e.to_string())? {
            wasmparser::Payload::ImportSection(section) => {
                for import in section.into_imports() {
                    let import = import.map_err(|e| e.to_string())?;
                    if import.module == MODULE {
                        found.push(import.name.to_owned());
                    }
                }
                break;
            }
            wasmparser::Payload::CodeSectionStart { .. } => break,
            _ => {}
        }
    }
    Ok(found)
}

/// Every mismatch of the mod members of a group (the shared standard library has no imports of
/// the game's), in import order, or none.
pub(crate) fn check(members: &[crate::link::Member<'_>]) -> Result<(), crate::Refusal> {
    let mut mismatches = Vec::new();
    for member in members.iter().filter(|m| m.name != "libstd.so") {
        let names = sunrise_imports(member.module)
            .map_err(|e| crate::Refusal::Runtime(format!("{}: {e}", member.name)))?;
        if !names.iter().any(|name| name.starts_with("protocol:log#")) {
            mismatches.push(Mismatch {
                item: "protocol".to_owned(),
                why: Why::Malformed(
                    "the module links no shared protocol: it was built for an older mod API"
                        .to_owned(),
                ),
            });
        }
        for name in &names {
            if let Err(mismatch) = resolve(name) {
                mismatches.push(mismatch);
            }
        }
    }
    if mismatches.is_empty() {
        return Ok(());
    }
    Err(crate::Refusal::Boundary {
        mismatches,
        build: GAME_BUILD.get().cloned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Refusal, runtime};

    /// A module with these imports of `sunrise` (each `(name, signature)`) and the guest entries.
    fn module(imports: &[(String, &str)]) -> Vec<u8> {
        let imports: String = imports
            .iter()
            .map(|(name, signature)| {
                format!("(import \"sunrise\" \"{name}\" (func {signature}))\n")
            })
            .collect();
        wat::parse_str(format!(
            r#"(module (@custom "dylink.0" "\01\04\00\00\00\00")
            (import "env" "memory" (memory 1))
            {imports}
            (func (export "sunrise_in") (param i32) (result i32) i32.const 0)
            (func (export "sunrise_hook") (param i64) (result i64) i64.const 0))"#
        ))
        .unwrap()
    }

    const CALL: &str = "(param i32 i32 i32 i32) (result i32)";

    fn log() -> (String, &'static str) {
        (protocol_name("log", SHARED_PROTOCOL).text(), CALL)
    }

    fn load(imports: &[(String, &str)]) -> Result<(), Refusal> {
        let bytes = module(imports);
        crate::test_stack::on_guest_stack(|| {
            runtime::WasmMod::instantiate(
                &runtime::engine().unwrap(),
                runtime::DEFAULT_GROUP_MEMORY_MIB,
                &[crate::link::Member {
                    name: "built",
                    module: &bytes,
                }],
                None,
            )
            .map(|_| ())
        })
    }

    fn mismatches(imports: &[(String, &str)]) -> (Vec<Mismatch>, usize) {
        match load(imports).unwrap_err() {
            Refusal::Boundary { mismatches, .. } => {
                let omitted = mismatches.len().saturating_sub(LISTED);
                (mismatches.into_iter().take(LISTED).collect(), omitted)
            }
            other => panic!("{other}"),
        }
    }

    /// `name` with its hash's last digit changed.
    fn changed(name: &str) -> String {
        let mut name = name.to_owned();
        let last = name.pop().unwrap();
        name.push(if last == '0' { '1' } else { '0' });
        name
    }

    #[test]
    fn a_module_that_imports_part_of_the_table_loads() {
        // The game's table has thousands more rows than this module uses: they are no concern of it.
        let first = CALLS[0].name.to_owned();
        let last = CALLS[CALLS.len() - 1].name.to_owned();
        let hook = EXPORT_CALLS[0].to_owned();
        load(&[log(), (first, CALL), (last, CALL), (hook, "")]).unwrap();
    }

    #[test]
    fn a_changed_call_is_refused_naming_it_and_both_hashes() {
        let name = CALLS[CALLS.len() / 2].name;
        let (list, omitted) = mismatches(&[log(), (changed(name), CALL)]);
        assert_eq!(omitted, 0);
        assert_eq!(list.len(), 1);
        let Why::Changed {
            built_hash,
            game_hash,
            ..
        } = &list[0].why
        else {
            panic!("{:?}", list[0]);
        };
        assert_ne!(built_hash, game_hash);
        assert!(name.contains(game_hash.as_str()));
        assert!(list[0].item.starts_with("host:"), "{}", list[0].item);
        assert!(list[0].to_string().contains("changed"));
    }

    #[test]
    fn a_removed_item_is_refused_as_missing() {
        let name = Name {
            kind: Kind::Call,
            path: "NoSuchHost::gone".into(),
            version: None,
            hash: "0".repeat(32),
        }
        .text();
        let (list, _) = mismatches(&[log(), (name, CALL)]);
        assert_eq!(list[0].why, Why::Missing);
        assert!(
            list[0]
                .to_string()
                .contains("NoSuchHost::gone is not in this game")
        );
    }

    #[test]
    fn a_hook_version_bump_is_refused_as_version_two_to_three() {
        let versioned = EXPORT_CALLS
            .iter()
            .find(|name| name.starts_with("hook:") && name.contains('@'))
            .expect("a hook with a version");
        let built = Name::parse(versioned).unwrap();
        let older = Name {
            version: Some(built.version.unwrap().saturating_sub(1)),
            ..built.clone()
        }
        .text();
        let (list, _) = mismatches(&[log(), (older, "")]);
        let text = list[0].to_string();
        assert!(
            text.contains(&format!(
                "version {} → {}",
                built.version.unwrap().saturating_sub(1),
                built.version.unwrap()
            )),
            "{text}"
        );
    }

    #[test]
    fn every_mismatch_is_listed_up_to_a_cap_and_the_rest_counted() {
        let imports: Vec<_> = std::iter::once(log())
            .chain((0..LISTED + 3).map(|n| {
                (
                    Name {
                        kind: Kind::Call,
                        path: format!("Gone{n}::x"),
                        version: None,
                        hash: "0".repeat(32),
                    }
                    .text(),
                    CALL,
                )
            }))
            .collect();
        let (list, omitted) = mismatches(&imports);
        assert_eq!((list.len(), omitted), (LISTED, 3));
    }

    #[test]
    fn a_mod_built_against_the_numbered_host_is_refused_once_telling_the_sdk_to_rebuild_with() {
        set_game_build("0123abcd");
        // What a mod built before this change imports: the one numbered `host` entry and a
        // whole-API layout export, with no shared protocol.
        let bytes = wat::parse_str(
            r#"(module (@custom "dylink.0" "\01\04\00\00\00\00")
            (import "env" "memory" (memory 1))
            (import "sunrise" "host" (func (param i32 i32 i32 i32 i32) (result i32)))
            (func (export "sunrise_layout") (result i64) i64.const 7)
            (func (export "sunrise_hook") (param i32) (result i64) i64.const 0))"#,
        )
        .unwrap();
        let refused = crate::test_stack::on_guest_stack(|| {
            runtime::WasmMod::instantiate(
                &runtime::engine().unwrap(),
                runtime::DEFAULT_GROUP_MEMORY_MIB,
                &[crate::link::Member {
                    name: "old",
                    module: &bytes,
                }],
                None,
            )
            .map(|_| ())
        })
        .unwrap_err();
        let text = refused.to_string();
        assert!(
            text.starts_with("built for an older Sunrise, rebuild with SDK sdk-0123abcd: uses "),
            "{text}"
        );
        assert!(text.contains("import \"host\""), "{text}");
    }

    #[test]
    fn checking_a_few_hundred_imports_is_a_binary_search_per_name() {
        let names: Vec<_> = CALLS
            .iter()
            .step_by(CALLS.len() / 400)
            .map(|c| c.name)
            .collect();
        let started = std::time::Instant::now();
        for name in &names {
            resolve(name).map_err(|m| m.to_string()).unwrap();
        }
        eprintln!(
            "resolved {} imports in {:?}",
            names.len(),
            started.elapsed()
        );
        assert!(started.elapsed() < std::time::Duration::from_millis(250));
    }

    #[test]
    fn a_name_the_generator_does_not_write_is_refused_by_kind_of_fault() {
        for bad in [
            "host:#00".to_owned(),
            "host:Host::x#zz".to_owned(),
            "mystery:Host::x#00112233445566778899aabbccddeeff".to_owned(),
            format!(
                "host:{}::x#00112233445566778899aabbccddeeff",
                "A".repeat(300)
            ),
            "host".to_owned(),
        ] {
            let (list, _) = mismatches(&[log(), (bad.clone(), CALL)]);
            assert!(
                matches!(list[0].why, Why::Malformed(_)),
                "{bad}: {:?}",
                list[0]
            );
        }
    }

    #[test]
    fn a_module_without_the_shared_protocol_is_refused() {
        let (list, _) = mismatches(&[(CALLS[0].name.to_owned(), CALL)]);
        assert!(matches!(list[0].why, Why::Malformed(_)));
        assert_eq!(list[0].item, "protocol");
    }

    #[test]
    fn another_protocol_number_is_a_changed_item() {
        let other = protocol_name("log", SHARED_PROTOCOL + 1).text();
        let (list, _) = mismatches(&[(other, CALL)]);
        assert!(
            list.iter().any(|m| matches!(m.why, Why::Changed { .. })),
            "{list:?}"
        );
    }
}

/// What a hand-written test module needs to call the game: every call the game serves as an
/// import, the protocol words, and `$host`, which calls one by its place in the table.
#[cfg(any(test, feature = "runtime-fixture"))]
pub(crate) mod fixtures {
    use crate::boundary::CALLS;
    use mod_surface::call::{SHARED_PROTOCOL, protocol_name};

    /// The id `$host` releases an object by.
    #[cfg(test)]
    pub(crate) fn release_id() -> u32 {
        CALLS.len() as u32
    }

    /// The id `$host` sends the game a message by.
    #[cfg(test)]
    pub(crate) fn log_id() -> u32 {
        CALLS.len() as u32 + 1
    }

    /// The one import every built module carries, for a module that calls nothing.
    pub(crate) fn log_import() -> String {
        format!(
            "(import \"sunrise\" \"{}\" (func (param i32 i32 i32 i32) (result i32)))",
            protocol_name("log", SHARED_PROTOCOL).text()
        )
    }

    /// The WAT items a module declares before its functions.
    pub(crate) fn wat() -> String {
        let mut text = String::new();
        let mut names: Vec<String> = CALLS.iter().map(|call| call.name.to_owned()).collect();
        names.push(protocol_name("release", SHARED_PROTOCOL).text());
        names.push(protocol_name("log", SHARED_PROTOCOL).text());
        for (index, name) in names.iter().enumerate() {
            text.push_str(&format!(
                "(import \"sunrise\" \"{name}\" (func $c{index} (param i32 i32 i32 i32) (result i32)))\n"
            ));
        }
        text.push_str(&format!(
            "(import \"sunrise\" \"{}\" (func $reply (param i32)))\n",
            protocol_name("host_reply", SHARED_PROTOCOL).text()
        ));
        // `$host(id, ptr, len, out, cap)` calls import `id`: a member declares no table of its own.
        text.push_str("(func $host (param i32 i32 i32 i32 i32) (result i32)\n");
        for index in (0..names.len()).rev() {
            text.push_str(&format!("(block $b{index}\n"));
        }
        text.push_str("local.get 0 br_table");
        for index in 0..names.len() {
            text.push_str(&format!(" $b{index}"));
        }
        text.push_str(&format!(" $b{}\n", names.len() - 1));
        for index in 0..names.len() {
            text.push_str(&format!(
                ")\nlocal.get 1 local.get 2 local.get 3 local.get 4 call $c{index} return\n"
            ));
        }
        text.push_str("unreachable)\n");
        text
    }
}
