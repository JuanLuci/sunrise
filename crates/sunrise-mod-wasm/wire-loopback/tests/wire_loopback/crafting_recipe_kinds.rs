//! The recipe owner, opaque value, frozen readers and owned remainders survive resource dispatch.
use super::*;
use mod_api::{CraftingGrid, CraftingRecipeHandler, CraftingRecipeValue};
struct Diagonal(mod_api::TagKey);
impl CraftingRecipeHandler for Diagonal {
    fn decode(&self, json: &str) -> Result<CraftingRecipeValue, String> {
        if json == r#"{"count":3}"# {
            Ok(CraftingRecipeValue::new(vec![3, 0, 0, 0]))
        } else {
            Err("count must be 3".into())
        }
    }
    fn write(&self, value: &CraftingRecipeValue) -> Result<String, String> {
        assert_eq!(value.bytes, [3, 0, 0, 0]);
        Ok(r#"{"count":3}"#.into())
    }
    fn matches(
        &self,
        context: &dyn mod_api::LoopContext,
        value: &CraftingRecipeValue,
        grid: CraftingGrid,
    ) -> bool {
        context.is_in_tag(REGISTERED_ITEM.get().unwrap(), self.0) == Some(true)
            && context.item_count(self.0) == Some(1)
            && context.item_at(self.0, 0) == REGISTERED_ITEM.get()
            && value.bytes == [3, 0, 0, 0]
            && grid.width == 3
            && grid.height == 3
            && grid
                .cells
                .iter()
                .enumerate()
                .all(|(i, c)| c.is_some() == [0, 4, 8].contains(&i))
    }
    fn assemble(
        &self,
        context: &dyn mod_api::LoopContext,
        _: &CraftingRecipeValue,
        _: CraftingGrid,
    ) -> Option<mod_api::ItemStack> {
        mod_api::ItemStack::new(context.item_at(self.0, 0)?, 3)
    }
    fn remaining_items(
        &self,
        context: &dyn mod_api::CraftingRemainders,
        _: &CraftingRecipeValue,
        _: CraftingGrid,
    ) -> Option<Vec<Option<mod_api::ItemStack>>> {
        if context.is_in_tag(REGISTERED_ITEM.get().unwrap(), self.0) != Some(true) {
            return None;
        }
        assert_eq!(context.item_count(self.0), Some(1));
        assert_eq!(context.item_at(self.0, 0), REGISTERED_ITEM.get());
        let mut out = vec![None; 9];
        out[4] = context.default_remainder(4);
        Some(out)
    }
}
struct CraftContext(mod_api::TagKey);
impl mod_api::LoopContext for CraftContext {
    fn is_in_tag(&self, item: mod_api::ItemKey, tag: mod_api::TagKey) -> Option<bool> {
        Some(tag == self.0 && Some(item) == REGISTERED_ITEM.get())
    }
    fn item_count(&self, tag: mod_api::TagKey) -> Option<usize> {
        Some(usize::from(tag == self.0))
    }
    fn item_at(&self, tag: mod_api::TagKey, index: usize) -> Option<mod_api::ItemKey> {
        (tag == self.0 && index == 0).then(|| REGISTERED_ITEM.get().unwrap())
    }
    fn side(&self) -> mod_api::LoopSide {
        Context.side()
    }
    fn registry(&self, name: &str) -> Option<Arc<[String]>> {
        Context.registry(name)
    }
    fn setting(&self, owner: &str, key: &str) -> Option<String> {
        Context.setting(owner, key)
    }
    fn mods(&self) -> mod_api::LoadedMods {
        Context.mods()
    }
    fn mod_recipe_property_contains(&self, set: &str, item: mod_api::ItemKey) -> bool {
        Context.mod_recipe_property_contains(set, item)
    }
}
impl mod_api::CraftingRemainders for CraftContext {
    fn default_remainder(&self, cell: u32) -> Option<mod_api::ItemStack> {
        if cell != 4 {
            return None;
        }
        let mut stack = mod_api::ItemStack::new(REGISTERED_ITEM.get().unwrap(), 1)?;
        stack.set_damage(17);
        Some(stack)
    }
}
#[test]
fn wire_loopback_crafting_kind_owner_value_grid_and_remainders() {
    mod_api::__private::initialize().unwrap();
    let _phase = mod_api::__private::PhaseOwner::scope(mod_api::__private::ModPhase::World);
    install_counter();
    let tag = mod_api::__private::tag_key(&mod_api::Identifier::parse("test:ingredients").unwrap())
        .unwrap();
    let context = CraftContext(tag);
    let mut out = Writer::new();
    {
        let _resources = guest::resource_scope();
        let handler: Arc<dyn CraftingRecipeHandler> = Arc::new(Diagonal(tag));
        out.put(&handler);
    }
    assert_eq!(out.error(), None);
    let module = WasmMod {
        group: 0,
        subscribed: &[],
        listeners: &[],
        corrupt: false,
    };
    let mut cx = boundary::Cx::default();
    cx.module = &module;
    let _resources = unsafe { boundary::resource_scope(&mut cx, true) };
    let handler = Reader::decode::<Arc<dyn CraftingRecipeHandler>>(out.bytes()).unwrap();
    let value = handler.decode(r#"{"count":3}"#).unwrap();
    assert_eq!(value.bytes, [3, 0, 0, 0]);
    assert_eq!(handler.write(&value).unwrap(), r#"{"count":3}"#);
    let key = REGISTERED_ITEM.get().unwrap();
    let mut cells = vec![None; 9];
    for i in [0, 4, 8] {
        cells[i] = mod_api::ItemStack::new(key, 2);
    }
    let grid = CraftingGrid::new(3, 3, cells).unwrap();
    assert!(handler.matches(&context, &value, grid.clone()));
    assert_eq!(
        handler.assemble(&context, &value, grid.clone()),
        mod_api::ItemStack::new(key, 3)
    );
    let mut expected = vec![None; 9];
    let mut remainder = mod_api::ItemStack::new(key, 1).unwrap();
    remainder.set_damage(17);
    expected[4] = Some(remainder);
    assert_eq!(
        handler.remaining_items(&context, &value, grid),
        Some(expected)
    );
    assert_eq!(handler.decode("{"), Err("count must be 3".into()));
}
