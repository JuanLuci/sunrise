//! The wire identity of a mod library's public types: the one shape function over the types as
//! the library's source declares them ([`crate::layouts`]), with nested types through their own
//! shapes. A `mod_api` type enters by its shape in the pinned SDK's surface; a type of another
//! mod library enters by that library's recorded identity. Nothing here compiles the library.
use crate::index::{Found, Index, ItemId};
use crate::layouts::{Layouts, WireForm, Wires};
use mod_surface::Surface;
use mod_surface::shape::{Shape, Shapes};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::Path;
use syn::{Item, Type};

/// The crates whose paths name `mod_api` in a mod's source.
const API: &[&str] = &["mod_api", "sunrise_mod_api"];

/// What a library's types may name beyond its own crate.
pub struct Environment<'s> {
    /// The pinned SDK's surface; none for a build that has none.
    pub api: Option<&'s Surface>,
    /// Other mod libraries' identities, by crate name, then public type path.
    pub libraries: &'s BTreeMap<String, BTreeMap<String, String>>,
}

struct LibraryWires<'e> {
    environment: &'e Environment<'e>,
    /// What the types name that this build cannot describe.
    problems: RefCell<Vec<String>>,
}

impl Wires for LibraryWires<'_> {
    fn form(&self, _: ItemId, _: &[syn::Attribute], _: &[String]) -> Option<WireForm> {
        None
    }

    fn path(&self, _: ItemId) -> Option<&[String]> {
        None
    }

    fn opaque(&self, index: &mut Index<'_>, module: usize, ty: &Type, params: &[String]) -> Shape {
        let written = quote::ToTokens::to_token_stream(ty).to_string();
        let Type::Path(path) = ty else {
            return Shape::Opaque(written);
        };
        let segments: Vec<String> = path
            .path
            .segments
            .iter()
            .map(|segment| segment.ident.to_string())
            .collect();
        if segments.len() == 1 && params.contains(&segments[0]) {
            return Shape::Opaque(written);
        }
        let Ok(Found::External(full)) = index.resolve_path(module, &segments) else {
            return Shape::Opaque(written);
        };
        let Some((krate, rest)) = full.split_first() else {
            return Shape::Opaque(written);
        };
        if API.contains(&krate.as_str()) {
            let name = rest.join("::");
            let Some(surface) = self.environment.api else {
                self.problems.borrow_mut().push(format!(
                    "{name}: a public type of the library names a `mod_api` type, and this build has no pinned SDK to read its shape from"
                ));
                return Shape::Opaque(written);
            };
            let record = surface.types.iter().find(|record| {
                record.paths.contains(&name)
                    || record.id.strip_prefix("type:") == Some(name.as_str())
            });
            return match record.and_then(|record| record.shape.as_deref()) {
                Some(shape) => Shape::Opaque(format!("api:{shape}")),
                None => {
                    self.problems.borrow_mut().push(format!(
                        "{name}: the pinned SDK has no wire shape for this `mod_api` type"
                    ));
                    Shape::Opaque(written)
                }
            };
        }
        if let Some(types) = self.environment.libraries.get(krate)
            && let Some(identity) = types.get(&full.join("::"))
        {
            return Shape::Opaque(format!("library:{identity}"));
        }
        Shape::Opaque(written)
    }
}

/// The identity of each of the library's public wire types, by public path: `types` maps each
/// public path to the path its type is defined at (as [`crate::wire_types::public_type_definitions`]
/// lists them), in crate `krate` whose root file is `lib`.
pub fn identities(
    krate: &str,
    lib: &Path,
    types: &BTreeMap<String, String>,
    environment: &Environment<'_>,
) -> Result<BTreeMap<String, String>, String> {
    let directory = lib.parent().ok_or("library has no source directory")?;
    let file = lib
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("library root has no file name")?;
    let krate: &'static str = Box::leak(krate.to_owned().into_boxed_str());
    let mut index = Index::new(directory, &[], &[]);
    index.add_crate(krate, "");
    index.set_root_file(krate, file);
    let mut layouts = Layouts::new(LibraryWires {
        environment,
        problems: RefCell::new(Vec::new()),
    });
    layouts.fields_without_form = true;
    let mut keys = Vec::new();
    for (public, defined) in types {
        let segments: Vec<String> = defined.split("::").skip(1).map(str::to_owned).collect();
        let (name, module) = segments
            .split_last()
            .ok_or_else(|| format!("layouts: {public}: no type name"))?;
        let module = index.module(krate, module)?;
        let item = index.modules[module]
            .items
            .iter()
            .position(|item| match item {
                Item::Struct(i) => i.ident == name.as_str(),
                Item::Enum(i) => i.ident == name.as_str(),
                Item::Union(i) => i.ident == name.as_str(),
                _ => false,
            })
            .ok_or_else(|| format!("layouts: {public}: {defined} is not declared in its module"))?;
        keys.push((public, layouts.key(&index, ItemId { module, item })));
    }
    layouts.finish(&mut index);
    let problems = layouts.wires().problems.borrow().clone();
    if !problems.is_empty() {
        return Err(format!("layouts: {}", problems.join("; ")));
    }
    let mut shapes = Shapes::new(&layouts.table);
    let mut out = BTreeMap::new();
    for (public, key) in keys {
        let text = shapes
            .shape(&key)
            .map_err(|e| format!("layouts: {public}: {e}"))?;
        out.insert(public.clone(), text);
    }
    Ok(out)
}
