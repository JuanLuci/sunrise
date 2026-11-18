# Wire types

Native and wasm mods share `mod_api::wire::{Wire, Reader, Writer, WireError}`.
Derive `mod_api::Wire` explicitly on a type whose values cross a target boundary.
The build never adds a derive. Types used only inside one target do not need it.
Every field of a derived type must also implement `Wire`.

```rust,ignore
#[derive(mod_api::Wire)]
pub struct ChemicalAmount {
    pub chemical: String,
    pub amount: u64,
}
```

Records encode fields in declaration order. Enums encode a case index followed by
its fields. Lengths and numbers are little-endian; collections are length-prefixed.
Decoding returns `WireError` for invalid data rather than accepting an unknown tag,
invalid UTF-8 or an unsupported length.

## Nesting and stacks

Structural decoding and encoding allow **32** nested records, payload-carrying
variants and `Box`/`Arc` values (`MAX_DEPTH`). Collection iteration, `Option`
and tuples do not consume a level. The reader returns `WireError::Depth` before
entering level 33; the writer records the same error. Always check `Writer::error`
before sending its bytes. This is a transport budget for unbounded mod-defined
trees, including ingredients, item places, loot vehicles and recipe slot displays.

Vanilla trees are not restricted by that structural budget: NBT `Tag` uses an
explicit work stack with **512** nested containers; `Text` crosses as the existing
validated flat `TextForm` table, preserving its **512** component-depth contract.
These encodings are identical on native and wasm. Text's layout identity is the
flat table's identity.

JSON-backed `serde_json::Value`, JSON maps and `ModelPart` also refuse beyond
32 JSON containers. Their encoders preflight the in-memory tree iteratively
before serde recursion; their decoders scan the JSON's nesting outside quoted
strings before parsing. A model part's JSON includes object, children-list and
named-child-tuple containers, so one part can consume several JSON levels.

The wasm linker reserves a **256 KiB** guest shadow stack. Wasmtime's wasm
call-stack limit is **128 MiB**. These are separate budgets. Hand-written recursive
`Wire` implementations must enforce their own stack contract; the reader cannot
police code outside its codec.

Wire does not own a borrowed value's destructor. Constructing an arbitrarily deep
Rust tree and then dropping it can overflow a thread independently of Wire.
Use explicit work stacks when disposing of hostile trees (`discard_tag` for NBT),
and do not transfer arbitrarily deep owned Rust trees to an encoder.

## Checked representations

Use `#[wire(via = String)]` (or another `Wire` type) when a checked constructor owns
validation. Implement `From<&YourType> for String` and `TryFrom<String> for YourType`.
The derive writes the representation and maps a failed conversion to
`WireError::Invalid`. Its layout is the representation's layout.

## Layout identity

A type's identity is its shape (`mod_surface::shape`): a canonical description of its fields,
case names and child types, whose hash a built mod carries inside the name of every call that
carries the type. The derive declares the shape from the Rust fields; a hand-written impl declares
one with `layout_fields!` (its fields) or `layout_via!(T)` (it crosses as `T`). Container shapes
follow encoding: `Vec<T>` and `VecDeque<T>` agree, as do `String` and `Box<str>`; `Box<T>` is
transparent. A type's path is not part of its shape. Changing a crossing field requires rebuilding
dependents; the wire format has no compatibility versions or default fields.

`cargo sunrise-mod build` writes `layouts.json` at the package root, mapping mod IDs
to public type paths and their identities (the same shape function, 32 hexadecimal digits). It
includes the mod libraries it builds against. The identities are computed from the libraries' source,
with no compile: nested types enter through their own shapes, a `mod_api` type by its shape in the
pinned SDK's `surface.json` (inside the game checkout, where no SDK is published, the surface exported from that checkout), and a type
of another mod library by that library's identity. Fields under `#[cfg]` all count. When
there are no concrete crossing types, no file is written.
A `layouts.json` of the old 16-digit format is read as legacy and the build writes it again. If `layouts.json` already exists, the build refuses to discard its recorded types.
Review the removal of the final public crossing type, then remove the file explicitly.
Private types without a public path and uninstantiated generic declarations are
not listed; a public re-export can give a private module's type a public path.

A mixed-target dependent with a different identity is refused naming the library
and differing type. Matching identities do not enable dependencies between native
and wasm mods: the loader refuses those edges explicitly.

## Hand-written implementations

The trait is unsealed. A hand-written implementation must supply `put`, `take` and `MIN`,
and declare its shape beside the impl with `layout_fields!` or `layout_via!`; the build
refuses a crossing type with none. The derive handles this automatically and is preferred.

A hand decoder owns tag validation and must use `Reader::nested` for recursive
values. `Reader` checks byte bounds and UTF-8, but cannot enforce invariants that
an implementation neglects. Keep validation at the checked constructor boundary.

Public declarations used in layout discovery must be unconditional. The identity scan refuses
`#[cfg]` and `#[cfg_attr]` on public types, modules, and re-exports, naming the declaration.
Keep target-specific implementation details private; publish an unconditional Wire representation.
