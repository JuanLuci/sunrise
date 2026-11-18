//! The prepared transport is part of the Wire ABI, beyond the public callback signature.
use super::{Method, exports};

pub(super) const SLOTS: [&str; 8] = [
    "head", "chest", "legs", "feet", "mainhand", "offhand", "body", "saddle",
];

#[derive(Clone, Debug)]
pub(super) struct Entry {
    pub name: String,
    pub input: Vec<&'static str>,
    pub output: Vec<&'static str>,
}
#[derive(Clone, Debug)]
pub(super) struct Abi {
    pub hook: String,
    pub entries: Vec<Entry>,
}

pub(super) fn describe(
    methods: &[Method],
    markers: &[(String, String)],
) -> Result<Vec<Abi>, String> {
    markers
        .iter()
        .map(|(hook, _)| {
            methods
                .iter()
                .filter(|m| exports().contains(&m.owner))
                .find(|m| m.owner == "ItemPart" && m.name == *hook)
                .ok_or_else(|| format!("prepared hook {hook} has no ItemPart export"))?;
            Ok(Abi {
                hook: hook.clone(),
                entries: vec![
                    Entry {
                        name: format!("prepare_{hook}"),
                        input: vec![
                            "slot:u32",
                            "generation:u32",
                            "stack:crate::ItemStack",
                            "kinds:Vec<String>",
                            "reads:Vec<crate::EquipReadRow>",
                        ],
                        output: vec!["token:(u32,u32)"],
                    },
                    Entry {
                        name: format!("ask_{hook}"),
                        input: vec![
                            "part:crate::wire::Id",
                            "slot:u32",
                            "generation:u32",
                            "equipment_slot:u32",
                            "entity:crate::LivingRef",
                            "kind:u32",
                            "initial:bool",
                        ],
                        output: vec!["answer:()", "entity:crate::LivingRef", "can_equip:bool"],
                    },
                    Entry {
                        name: format!("release_{hook}"),
                        input: vec!["slot:u32", "generation:u32"],
                        output: vec!["answer:()"],
                    },
                ],
            })
        })
        .collect()
}
