//! A mod built by the build tool names each item of the game it uses; the real fixture loads on
//! the game's table, and is refused, naming the item, when one of those items changed.
#[path = "../src/test_stack.rs"]
mod test_stack;
use sunrise_mod_package::manifest::Package;
use sunrise_mod_wasm::{WasmPackage, load};

fn fixture() -> (WasmPackage, Vec<u8>) {
    let (Ok(fixture), Ok(std)) = (
        std::env::var("SUNRISE_WASM_FIXTURE"),
        std::env::var("SUNRISE_WASM_STD"),
    ) else {
        panic!(
            "SUNRISE_WASM_FIXTURE and SUNRISE_WASM_STD must be set: run with the wasm-mods workflow's environment"
        );
    };
    let path = std::path::Path::new(&fixture);
    let mut package = WasmPackage {
        manifest: Package::read(path).unwrap().manifest,
        module: Package::read_wasm(path).unwrap(),
    };
    package.manifest.inject.clear();
    (package, std::fs::read(std).unwrap())
}

/// The module with the last digit of the first name that starts `prefix` changed: the name keeps
/// its length, so no section moves.
fn changed(module: &[u8], prefix: &[u8]) -> (Vec<u8>, String) {
    let start = module
        .windows(prefix.len())
        .position(|window| window == prefix)
        .expect("the fixture imports such a name");
    let end = start
        + module[start..]
            .iter()
            .position(|&byte| byte == b'#')
            .unwrap()
        + 33;
    let mut out = module.to_vec();
    out[end - 1] = if out[end - 1] == b'0' { b'1' } else { b'0' };
    (out, String::from_utf8(module[start..end].to_vec()).unwrap())
}

#[test]
#[ignore = "needs SUNRISE_WASM_FIXTURE and SUNRISE_WASM_STD, which the wasm-mods workflow builds"]
fn the_fixture_loads_on_the_games_table() {
    test_stack::on_guest_stack(|| {
        mod_api::__private::initialize().unwrap();
        let (package, std) = fixture();
        let loaded = load(&std, vec![package]);
        assert!(loaded.refused.is_empty(), "{:?}", loaded.refused);
        assert_eq!(loaded.mods.len(), 1);
    })
}

#[test]
#[ignore = "needs SUNRISE_WASM_FIXTURE and SUNRISE_WASM_STD, which the wasm-mods workflow builds"]
fn a_changed_call_the_fixture_uses_refuses_it_by_name() {
    test_stack::on_guest_stack(|| {
        mod_api::__private::initialize().unwrap();
        for prefix in [&b"call:"[..], b"hook:", b"part:"] {
            let (mut package, std) = fixture();
            let (module, name) = changed(&package.module, prefix);
            package.module = module;
            let loaded = load(&std, vec![package]);
            assert!(loaded.mods.is_empty());
            let reason = loaded.refused[0].1.to_string();
            let item = name.split('#').next().unwrap();
            assert!(
                reason.contains(item.trim_start_matches("call:")) && reason.contains("changed"),
                "{name}: {reason}"
            );
            assert!(
                reason.ends_with("(or a type it carries changed)"),
                "{reason}"
            );
        }
    })
}

/// A mod built before a method was added to the API loads on the game that has it: the table is
/// searched by name, so an entry no import names changes nothing for the mod.
#[test]
#[ignore = "needs SUNRISE_WASM_FIXTURE and SUNRISE_WASM_STD, which the wasm-mods workflow builds"]
fn an_unrelated_host_method_added_to_the_game_refuses_nothing() {
    let (package, _) = fixture();
    let imports_before = sunrise_mod_wasm::fixture::mismatches_with_added_call(
        &package.module,
        "call:Host::a_method_added_after_this_mod_was_built#00112233445566778899aabbccddeeff",
    )
    .unwrap();
    assert!(imports_before.is_empty(), "{imports_before:?}");
    // The same table with a changed method the mod uses is refused, so the check above bites.
    let (module, _) = changed(&package.module, b"call:");
    let refused = sunrise_mod_wasm::fixture::mismatches_with_added_call(
        &module,
        "call:Host::a_method_added_after_this_mod_was_built#00112233445566778899aabbccddeeff",
    )
    .unwrap();
    assert_eq!(refused.len(), 1, "{refused:?}");
}

/// A mod built before this change, by the SDK of the base commit 36839e7ce7d (the one numbered
/// `host` import and the whole-API layout export), is refused once, with the rebuild text.
#[test]
#[ignore = "needs SUNRISE_WASM_OLD_FIXTURE and SUNRISE_WASM_OLD_STD: the fixture and std built by the SDK before the per-item imports"]
fn a_mod_built_before_the_break_is_refused_once_with_the_rebuild_text() {
    test_stack::on_guest_stack(|| {
        mod_api::__private::initialize().unwrap();
        let (Ok(old), Ok(std)) = (
            std::env::var("SUNRISE_WASM_OLD_FIXTURE"),
            std::env::var("SUNRISE_WASM_OLD_STD"),
        ) else {
            panic!("SUNRISE_WASM_OLD_FIXTURE and SUNRISE_WASM_OLD_STD must be set");
        };
        let path = std::path::Path::new(&old);
        let mut package = WasmPackage {
            manifest: Package::read(path).unwrap().manifest,
            module: Package::read_wasm(path).unwrap(),
        };
        package.manifest.inject.clear();
        sunrise_mod_wasm::set_game_build("0123abcd");
        let loaded = load(&std::fs::read(std).unwrap(), vec![package]);
        assert!(loaded.mods.is_empty());
        assert_eq!(loaded.refused.len(), 1, "{:?}", loaded.refused);
        let reason = loaded.refused[0].1.to_string();
        assert!(
            reason.starts_with("built for an older Sunrise, rebuild with SDK sdk-0123abcd: "),
            "{reason}"
        );
    })
}
