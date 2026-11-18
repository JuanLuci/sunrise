//! The names of the calls a wasm mod makes and answers (`mod_surface::call`), from the one
//! shape function: a call is a record of its parameters and answer, applied to the arguments of
//! its trait, so every type it carries counts through its fields and no type's path does.
//!
//! The wire generator (guest and host glue) and the surface export both call [`names`], so the
//! game's table and `surface.json`'s `call` fields are the same strings.
use super::rust::GameWires;
use super::walk::Walker;
use super::{API_CRATES, expand, wire_forms};
use crate::index::Index;
use mod_source::layouts::Layouts;
use mod_surface::call::{Kind, Name, Spec};
use mod_surface::shape::{Body, Definition, Shape, Shapes};

/// One method whose name is wanted: the types as written, in the module that wrote them.
pub(crate) struct Call {
    pub kind: Kind,
    /// The trait as written (`Hooks`, `ResourceHandler<ItemResource>`).
    pub owner: String,
    pub method: String,
    /// `Some(mutable)` for a `self` receiver.
    pub receiver: Option<bool>,
    pub module: usize,
    pub params: Vec<syn::Type>,
    /// `None` for `()`.
    pub ret: Option<syn::Type>,
    /// The trait's own type parameters, which `owner_args` replace.
    pub trait_params: Vec<String>,
    /// The arguments the owner is written with, in the API's root module.
    pub owner_args: Vec<syn::Type>,
    /// What else the call rests on (a hook's side).
    pub extra: String,
    pub version: Option<u32>,
}

/// The name of each of `calls`, in order. Fails naming a call whose types have no shape.
pub(crate) fn names(index: &mut Index<'_>, calls: &[Call]) -> Result<Vec<Name>, String> {
    index.add_crate("sunrise_mod_api", "crates/sunrise-mod-api/src");
    let mut expanded = Vec::new();
    {
        let mut walker = Walker::new(index);
        for krate in API_CRATES {
            if let Ok(found) = walker.all_modules(krate) {
                expanded.extend(found);
            }
        }
    }
    expand::expand_modules(index, &expanded);
    let mut walker = Walker::new(index);
    let public = walker.public()?;
    let mut crates = std::collections::BTreeSet::from(["mod_api", "sunrise_mod_api"]);
    for id in public.items.keys() {
        crates.insert(walker.index.modules[id.module].krate);
    }
    let mut modules = Vec::new();
    for krate in crates {
        modules.extend(walker.all_modules(krate)?);
    }
    let mut nameable = public.items.clone();
    nameable.extend(public.hidden.iter().map(|(id, path)| (*id, path.clone())));
    let forms = wire_forms::wire_forms(index, &modules);
    let api = index.module("mod_api", &[])?;
    let mut layouts = Layouts::new(GameWires {
        forms: &forms,
        ids: &nameable,
    });
    layouts.fields_without_form = true;
    let mut records = Vec::new();
    for call in calls {
        let mut fields: Vec<(String, Shape)> = call
            .params
            .iter()
            .enumerate()
            .map(|(i, ty)| {
                (
                    format!("p{i}"),
                    layouts.shape(index, call.module, ty, &call.trait_params, 0),
                )
            })
            .collect();
        fields.push((
            "ret".to_owned(),
            match &call.ret {
                Some(ty) => layouts.shape(index, call.module, ty, &call.trait_params, 0),
                None => Shape::Scalar(mod_surface::shape::Scalar::Unit),
            },
        ));
        let args = call
            .owner_args
            .iter()
            .map(|ty| layouts.shape(index, api, ty, &[], 0))
            .collect::<Vec<_>>();
        records.push((fields, args));
    }
    layouts.finish(index);
    let mut table = layouts.table;
    let keys: Vec<String> = (0..calls.len()).map(|i| format!("call#{i}")).collect();
    for ((key, call), (fields, _)) in keys.iter().zip(calls).zip(&records) {
        table.insert(
            key.clone(),
            Definition {
                params: call.trait_params.len(),
                body: Body::Record(fields.clone()),
            },
        );
    }
    let mut shapes = Shapes::new(&table);
    let mut out = Vec::new();
    for ((call, key), (_, args)) in calls.iter().zip(&keys).zip(records) {
        let shape = shapes
            .applied(key, args)
            .map_err(|e| format!("{}::{}: {e}", call.owner, call.method))?;
        // The record's text carries every parameter and the answer; its own digest is the
        // call's signature, which the name's hash then covers with the rest.
        let params = [shape];
        out.push(
            Spec {
                kind: call.kind,
                owner: &call.owner,
                method: &call.method,
                receiver: call.receiver,
                extra: &call.extra,
                params: &params,
                ret: "",
                version: call.version,
            }
            .name(),
        );
    }
    Ok(out)
}
