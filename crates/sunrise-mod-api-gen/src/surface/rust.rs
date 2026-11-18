//! Rust as the surface writes it: one normalising function for types and signatures (paths
//! relative to `mod_api`, lifetimes kept, one spacing), and each type's wire layout description
//! for the shape function.
use crate::index::{Found, Index, ItemId};
use mod_source::layouts::{WireForm, Wires};
use mod_surface::shape::Shape;
use proc_macro2::{Delimiter, Spacing, TokenStream, TokenTree};
use quote::ToTokens;
use std::collections::{BTreeSet, HashMap};
use syn::visit_mut::VisitMut;
use syn::{Item, Type};

/// The crates whose paths are written by their last segment (`Arc`, `HashMap`, `Duration`).
const STD: &[&str] = &["std", "core", "alloc"];

/// `tokens` with one spacing: a space after `,`, `;` and a lone `:`, around `->`, `=`, `+` and
/// `=>`, between two words, after a word-ending `>`, `)` or `]` before a word; none elsewhere.
/// A comma before a closing bracket or the end is dropped.
pub(super) fn print(tokens: TokenStream) -> String {
    #[derive(Clone, Copy, PartialEq)]
    enum Last {
        Start,
        Word,
        Close,
        Glue,
        Spaced,
    }
    fn walk(tokens: TokenStream, out: &mut String, last: &mut Last) {
        let trees: Vec<TokenTree> = tokens.into_iter().collect();
        let mut i = 0;
        while i < trees.len() {
            match &trees[i] {
                TokenTree::Ident(ident) => {
                    if matches!(*last, Last::Word | Last::Close) {
                        out.push(' ');
                    }
                    out.push_str(&ident.to_string());
                    *last = Last::Word;
                }
                TokenTree::Literal(literal) => {
                    if matches!(*last, Last::Word | Last::Close) {
                        out.push(' ');
                    }
                    out.push_str(&literal.to_string());
                    *last = Last::Word;
                }
                TokenTree::Group(group) => {
                    let (open, close) = match group.delimiter() {
                        Delimiter::Parenthesis => ("(", ")"),
                        Delimiter::Bracket => ("[", "]"),
                        Delimiter::Brace => (" { ", " }"),
                        Delimiter::None => ("", ""),
                    };
                    out.push_str(open);
                    let mut inner = Last::Start;
                    walk(group.stream(), out, &mut inner);
                    out.push_str(close);
                    *last = Last::Close;
                }
                TokenTree::Punct(punct) => {
                    // Gather a joint run (`::`, `->`, `'a`).
                    let mut run = punct.as_char().to_string();
                    let mut joint = punct.spacing() == Spacing::Joint;
                    while joint && i + 1 < trees.len() {
                        let TokenTree::Punct(next) = &trees[i + 1] else {
                            break;
                        };
                        if punct.as_char() == '\'' {
                            break;
                        }
                        run.push(next.as_char());
                        joint = next.spacing() == Spacing::Joint;
                        i += 1;
                    }
                    let end = trees.get(i + 1).is_none()
                        || matches!(trees.get(i + 1), Some(TokenTree::Punct(p)) if p.as_char() == '>');
                    match run.as_str() {
                        "," if end => {}
                        "," | ";" | ":" => {
                            out.push_str(&run);
                            out.push(' ');
                            *last = Last::Spaced;
                        }
                        "->" | "=" | "+" | "=>" => {
                            if !out.ends_with(' ') && !out.is_empty() {
                                out.push(' ');
                            }
                            out.push_str(&run);
                            out.push(' ');
                            *last = Last::Spaced;
                        }
                        "'" => {
                            if *last == Last::Word {
                                out.push(' ');
                            }
                            out.push('\'');
                            // The lifetime's name follows as a word glued to the quote.
                            *last = Last::Glue;
                        }
                        ">" => {
                            out.push('>');
                            *last = Last::Close;
                        }
                        "&" | "&&" if *last == Last::Word => {
                            out.push(' ');
                            out.push_str(&run);
                            *last = Last::Glue;
                        }
                        _ => {
                            out.push_str(&run);
                            *last = Last::Glue;
                        }
                    }
                }
            }
            i += 1;
        }
    }
    let mut out = String::new();
    walk(tokens, &mut out, &mut Last::Start);
    out.trim().to_owned()
}

/// Rewrites every path of a type or signature to the path a mod writes, collecting the ids of
/// the API types and traits it names.
pub(super) struct Paths<'p, 'a> {
    pub(super) index: &'p mut Index<'a>,
    pub(super) public: &'p HashMap<ItemId, Vec<String>>,
    pub(super) module: usize,
    /// Generic parameters in scope (and `Self`), written as they are.
    pub(super) params: Vec<String>,
    pub(super) refs: BTreeSet<String>,
}

/// The id prefix of a public item that a signature can name.
pub(super) fn ref_kind(item: &Item) -> Option<&'static str> {
    match item {
        Item::Struct(_) | Item::Enum(_) | Item::Union(_) | Item::Type(_) => Some("type"),
        Item::Trait(_) => Some("trait"),
        _ => None,
    }
}

impl Paths<'_, '_> {
    fn rewrite(&mut self, path: &mut syn::Path) {
        let segments: Vec<String> = path.segments.iter().map(|s| s.ident.to_string()).collect();
        if path.leading_colon.is_none()
            && segments.len() == 1
            && self.params.iter().any(|p| *p == segments[0])
            || segments.first().is_some_and(|s| s == "Self")
        {
            return;
        }
        let written = match self.index.resolve_path(self.module, &segments) {
            Ok(Found::Item(id)) => match self.public.get(&id) {
                Some(public) => {
                    if let Some(kind) = ref_kind(self.index.item(id)) {
                        self.refs.insert(format!("{kind}:{}", public.join("::")));
                    }
                    public.clone()
                }
                None => vec![segments.last().cloned().unwrap_or_default()],
            },
            Ok(Found::External(full)) if STD.contains(&full[0].as_str()) => {
                vec![full.last().cloned().unwrap_or_default()]
            }
            Ok(Found::External(full)) => full,
            Err(_) => segments.clone(),
        };
        if written == segments {
            return;
        }
        let arguments = path
            .segments
            .last()
            .map(|s| s.arguments.clone())
            .unwrap_or(syn::PathArguments::None);
        let mut rewritten: syn::Path =
            syn::parse_str(&written.join("::")).unwrap_or_else(|_| path.clone());
        if let Some(last) = rewritten.segments.last_mut() {
            last.arguments = arguments;
        }
        rewritten.leading_colon = None;
        *path = rewritten;
    }
}

impl VisitMut for Paths<'_, '_> {
    fn visit_type_path_mut(&mut self, ty: &mut syn::TypePath) {
        syn::visit_mut::visit_type_path_mut(self, ty);
        if ty.qself.is_none() {
            self.rewrite(&mut ty.path);
        }
    }
    fn visit_trait_bound_mut(&mut self, bound: &mut syn::TraitBound) {
        syn::visit_mut::visit_trait_bound_mut(self, bound);
        self.rewrite(&mut bound.path);
    }
    fn visit_attribute_mut(&mut self, _: &mut syn::Attribute) {}
}

/// The game's crates: types cross as their derive or hand-written impl says, and an opaque type
/// is written with the public paths of the API.
pub(super) struct GameWires<'f> {
    pub(super) forms: &'f HashMap<ItemId, WireForm>,
    pub(super) ids: &'f HashMap<ItemId, Vec<String>>,
}

impl Wires for GameWires<'_> {
    fn form(&self, id: ItemId, attrs: &[syn::Attribute], derives: &[String]) -> Option<WireForm> {
        super::wire_forms::form_of(id, attrs, derives, self.forms)
    }
    fn path(&self, id: ItemId) -> Option<&[String]> {
        self.ids.get(&id).map(Vec::as_slice)
    }
    fn opaque(&self, index: &mut Index<'_>, module: usize, ty: &Type, params: &[String]) -> Shape {
        let mut ty = ty.clone();
        let mut paths = Paths {
            index,
            public: self.ids,
            module,
            params: params.to_vec(),
            refs: BTreeSet::new(),
        };
        paths.visit_type_mut(&mut ty);
        Shape::Opaque(print(ty.to_token_stream()))
    }
}
