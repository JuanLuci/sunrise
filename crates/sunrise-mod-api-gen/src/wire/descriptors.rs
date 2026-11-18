//! What the boundary does with a type beyond its Rust fields: a borrowed record is copied field by
//! field, an append list is checked and written back, a projection or view is lent and read back.
//! Those rules are private to the generator, they decide the bytes a call carries, and no shape
//! of the type's declaration sees them. A call's hash covers them as one canonical text, written
//! here with explicit spellings (never `Debug`), so a generator refactor cannot move a hash and
//! a change to a rule always does.
use super::ty::{BorrowedShape, Def, Field, List, Str, Ty, Types};
use std::collections::BTreeSet;
use std::fmt::Write as _;

/// The canonical text of the descriptors of every borrowed type the call's types reach.
pub(super) fn text(types: &Types<'_>, tys: &[&Ty]) -> String {
    let mut seen = BTreeSet::new();
    let mut work: Vec<usize> = Vec::new();
    for ty in tys {
        reach(ty, &mut work);
    }
    let mut texts = BTreeSet::new();
    while let Some(def) = work.pop() {
        if !seen.insert(def) {
            continue;
        }
        let d = &types.defs[def];
        for ty in members(d) {
            reach(ty, &mut work);
        }
        if d.lifetimes != 0 || d.append.is_some() || d.projection.is_some() || d.view.is_some() {
            texts.insert(describe(types, d));
        }
    }
    texts.into_iter().collect::<Vec<_>>().join(";")
}

fn reach(ty: &Ty, out: &mut Vec<usize>) {
    match ty {
        Ty::Named(def) => out.push(*def),
        Ty::List(t, _)
        | Ty::Option(t)
        | Ty::Array(t, _)
        | Ty::Ref(t, _)
        | Ty::Boxed(t)
        | Ty::Shared(t) => reach(t, out),
        Ty::Result(a, b) => {
            reach(a, out);
            reach(b, out);
        }
        Ty::Tuple(v) => v.iter().for_each(|t| reach(t, out)),
        _ => {}
    }
}

/// Every type a definition's rules mention.
fn members(d: &Def) -> Vec<&Ty> {
    let mut out: Vec<&Ty> = Vec::new();
    fn fields<'a>(fields: &'a [Field], out: &mut Vec<&'a Ty>) {
        out.extend(fields.iter().map(|f| &f.ty));
    }
    match &d.borrowed {
        BorrowedShape::Record(f) | BorrowedShape::TupleRecord(f) => fields(f, &mut out),
        BorrowedShape::Variant(cases) => {
            for case in cases {
                out.extend(case.payload.iter());
            }
        }
        BorrowedShape::None => {}
    }
    if let Some(append) = &d.append {
        fields(&append.fields, &mut out);
        out.push(&append.element);
    }
    if let Some(projection) = &d.projection {
        fields(projection, &mut out);
    }
    if let Some(view) = &d.view {
        out.push(&view.pointee);
        out.push(&view.snapshot);
    }
    out
}

fn describe(types: &Types<'_>, d: &Def) -> String {
    let mut out = format!("def({};{};", d.rust, d.lifetimes);
    let fields = |fields: &[Field]| {
        fields
            .iter()
            .map(|f| format!("{}:{}", f.rust, ty(types, &f.ty)))
            .collect::<Vec<_>>()
            .join(",")
    };
    match &d.borrowed {
        BorrowedShape::Record(f) => {
            let _ = write!(out, "record[{}]", fields(f));
        }
        BorrowedShape::TupleRecord(f) => {
            let _ = write!(out, "tuple[{}]", fields(f));
        }
        BorrowedShape::Variant(cases) => {
            let cases = cases
                .iter()
                .map(|c| {
                    format!(
                        "{}({})",
                        c.rust,
                        c.payload
                            .iter()
                            .map(|t| ty(types, t))
                            .collect::<Vec<_>>()
                            .join(",")
                    )
                })
                .collect::<Vec<_>>()
                .join(",");
            let _ = write!(out, "variant[{cases}]");
        }
        BorrowedShape::None => out.push_str("owned"),
    }
    if let Some(append) = &d.append {
        let _ = write!(
            out,
            ";append[{};{};{};{}]",
            fields(&append.fields),
            append.field,
            ty(types, &append.element),
            append.equal
        );
    }
    if let Some(projection) = &d.projection {
        let _ = write!(out, ";projection[{}]", fields(projection));
    }
    if let Some(view) = &d.view {
        let _ = write!(
            out,
            ";view[{};{};{};{}]",
            view.field,
            ty(types, &view.pointee),
            view.mutable,
            ty(types, &view.snapshot)
        );
    }
    out.push(')');
    out
}

fn ty(types: &Types<'_>, t: &Ty) -> String {
    let one = |name: &str, t: &Ty| format!("{name}({})", ty(types, t));
    match t {
        Ty::Unit => "unit".into(),
        Ty::Prim(p) => format!("prim:{p}"),
        Ty::Wide(p) => format!("wide:{p}"),
        Ty::Duration => "duration".into(),
        Ty::Str(kind) => format!(
            "str:{}",
            match kind {
                Str::Owned => "owned",
                Str::Borrowed => "borrowed",
                Str::Static => "static",
                Str::Boxed => "boxed",
                Str::Shared => "shared",
                Str::Cow => "cow",
            }
        ),
        Ty::List(inner, kind) => {
            let kind = match kind {
                List::Vec => "vec".to_owned(),
                List::Map(name) => format!("map:{name}"),
                List::Set(name) => format!("set:{name}"),
                List::Slice => "slice".to_owned(),
                List::Boxed => "boxed".to_owned(),
                List::Shared => "shared".to_owned(),
            };
            one(&format!("list:{kind}"), inner)
        }
        Ty::Option(inner) => one("option", inner),
        Ty::Result(a, b) => format!("result({},{})", ty(types, a), ty(types, b)),
        Ty::Tuple(items) => format!(
            "tuple({})",
            items
                .iter()
                .map(|t| ty(types, t))
                .collect::<Vec<_>>()
                .join(",")
        ),
        Ty::Array(inner, n) => format!("array({};{n})", ty(types, inner)),
        Ty::Ref(inner, mutable) => one(if *mutable { "refmut" } else { "ref" }, inner),
        Ty::Boxed(inner) => one("box", inner),
        Ty::Shared(inner) => one("arc", inner),
        Ty::Named(def) => format!("named({})", types.defs[*def].rust),
        Ty::Dyn(name, mutable) => format!("dyn({name};{mutable})"),
        Ty::Handler(name, boxed) => format!("handler({name};{boxed})"),
    }
}
