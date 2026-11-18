//! The name of one thing a built wasm mod uses of the game, and its hash.
//!
//! A wasm module's import section is the table of what it uses: every host call, every hook or
//! Part method it implements, every point listener and the shared protocol is a function import
//! of module [`MODULE`], named `<kind>:<owner>::<method>[@<version>]#<hash>` (a protocol import
//! `protocol:<name>#<hash>`). The linker keeps an import only for code it keeps, so the table is
//! exact and no tool writes it. The game's table is generated from the same source by the same
//! function, so the loader compares names and never hashes.
//!
//! The hash is the first 128 bits of SHA-256 of one canonical text, `call{kind;owner;method;
//! receiver;extra;[param shapes];return shape;version}`. Each type that crosses enters as its
//! [`crate::shape`] value (so its fields count, transitively, and its path does not); parameter
//! names are not in it, so a rename refuses nothing. Nothing in it depends on the order of the
//! API's declarations.
use crate::shape::digest;

/// The import module every name here lives in.
pub const MODULE: &str = "sunrise";

/// The shared protocol's number, carried in the name of each protocol import (`host_reply`,
/// `release`, `log`) every guest links. The loader refuses a module whose number is not the
/// game's. Bump it when anything crossing the boundary changes that no declared shape covers:
/// the identity of a resource (zero is reserved), the group-state words in the std member, the
/// static-reply framing, the `ConfigFiles` root selector, the shape of `Hooks::subscribed`, or
/// the transport signature. A test in `sunrise-mod-api-gen` hashes those items and fails with
/// "bump SHARED_PROTOCOL" when they change and this number does not.
pub const SHARED_PROTOCOL: u32 = 1;

/// The longest name a module may import (bytes). The generator keeps its own under it.
pub const MAX_NAME_BYTES: usize = 256;

/// What a name stands for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Kind {
    /// A call of a game service: a method of a host trait.
    Call,
    /// A hook the mod implements.
    Hook,
    /// A method of a Part or handler trait the mod implements.
    Part,
    /// A point listener.
    Point,
    /// One word of the shared protocol (transport, release, log).
    Protocol,
}

impl Kind {
    const ALL: [Self; 5] = [
        Self::Call,
        Self::Hook,
        Self::Part,
        Self::Point,
        Self::Protocol,
    ];

    /// The text before the colon.
    pub fn word(self) -> &'static str {
        match self {
            Self::Call => "call",
            Self::Hook => "hook",
            Self::Part => "part",
            Self::Point => "point",
            Self::Protocol => "protocol",
        }
    }

    /// How a refusal names an item of this kind (`host:ServerHost::give_item`).
    pub fn item_word(self) -> &'static str {
        match self {
            Self::Call => "host",
            other => other.word(),
        }
    }
}

/// A parsed import name.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Name {
    pub kind: Kind,
    /// `Owner::method`, or a protocol word. Letters, digits, `_`, `:`, `<`, `>` and `,`.
    pub path: String,
    /// A hook's version, when it has one.
    pub version: Option<u32>,
    /// 32 lowercase hex digits.
    pub hash: String,
}

/// Why a name is not one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NameError {
    TooLong,
    Kind,
    Path,
    Version,
    Hash,
}

impl std::fmt::Display for NameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::TooLong => "longer than a name may be",
            Self::Kind => "not a known kind of name",
            Self::Path => "its path is empty or has a character no path has",
            Self::Version => "its version is not a number",
            Self::Hash => "its hash is not 32 hex digits",
        })
    }
}

impl std::error::Error for NameError {}

impl Name {
    /// The name as the import section spells it.
    pub fn text(&self) -> String {
        let mut out = format!("{}:{}", self.kind.word(), self.path);
        if let Some(version) = self.version {
            out.push_str(&format!("@{version}"));
        }
        out.push('#');
        out.push_str(&self.hash);
        out
    }

    /// The name without its hash, the item a refusal names: `host:ServerHost::give_item`,
    /// `hook:Hooks::block_click@3`.
    pub fn item(&self) -> String {
        match self.version {
            Some(version) => format!("{}:{}@{version}", self.kind.item_word(), self.path),
            None => format!("{}:{}", self.kind.item_word(), self.path),
        }
    }

    /// Reads an import name. Never panics, whatever the bytes.
    pub fn parse(text: &str) -> Result<Self, NameError> {
        if text.len() > MAX_NAME_BYTES {
            return Err(NameError::TooLong);
        }
        let (kind, rest) = text.split_once(':').ok_or(NameError::Kind)?;
        let kind = Kind::ALL
            .into_iter()
            .find(|k| k.word() == kind)
            .ok_or(NameError::Kind)?;
        let (rest, hash) = rest.rsplit_once('#').ok_or(NameError::Hash)?;
        if hash.len() != 32 || !hash.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
            return Err(NameError::Hash);
        }
        let (path, version) = match rest.rsplit_once('@') {
            Some((path, version)) => (
                path,
                Some(
                    version
                        .parse::<u32>()
                        .ok()
                        .filter(|v| v.to_string() == version)
                        .ok_or(NameError::Version)?,
                ),
            ),
            None => (rest, None),
        };
        let valid = !path.is_empty()
            && !path.starts_with(':')
            && !path.ends_with(':')
            && path.bytes().all(|b| {
                b.is_ascii_alphanumeric() || matches!(b, b'_' | b':' | b'<' | b'>' | b',')
            });
        if !valid {
            return Err(NameError::Path);
        }
        Ok(Self {
            kind,
            path: path.to_owned(),
            version,
            hash: hash.to_owned(),
        })
    }
}

/// Everything that makes one call what it is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Spec<'a> {
    pub kind: Kind,
    /// The trait the method belongs to.
    pub owner: &'a str,
    pub method: &'a str,
    /// `Some(mutable)` for a `self` receiver.
    pub receiver: Option<bool>,
    /// What else the call's meaning rests on: a hook's side, kind and scope.
    pub extra: &'a str,
    /// The shape of each parameter, as [`crate::shape::Shapes`] writes it.
    pub params: &'a [String],
    pub ret: &'a str,
    /// A hook's version.
    pub version: Option<u32>,
}

impl Spec<'_> {
    /// The call's name.
    pub fn name(&self) -> Name {
        let receiver = match self.receiver {
            Some(true) => "&mut",
            Some(false) => "&",
            None => "",
        };
        let ret = format!("{}:{}", self.ret.len(), self.ret);
        let text = format!(
            "call{{{};{};{};{receiver};{};[{}];{ret};{}}}",
            self.kind.word(),
            self.owner,
            self.method,
            self.extra,
            self.params
                .iter()
                .map(|p| format!("{}:{p}", p.len()))
                .collect::<Vec<_>>()
                .join(","),
            self.version.map_or(String::new(), |v| v.to_string()),
        );
        Name {
            kind: self.kind,
            // A generic owner is written as the source writes it, a character no path has as `_`.
            path: format!(
                "{}::{}",
                self.owner
                    .chars()
                    .map(|c| {
                        if c.is_ascii_alphanumeric() || matches!(c, '_' | ':' | '<' | '>' | ',') {
                            c
                        } else {
                            '_'
                        }
                    })
                    .collect::<String>(),
                self.method
            ),
            version: self.version,
            hash: digest(&text),
        }
    }
}

/// The bit that marks a point listener's id. Every other export's id has it clear.
pub const POINT_TAG: u64 = 1 << 63;

impl Name {
    /// The id the game sends to `sunrise_hook` for an export of this name: 63 bits of a digest
    /// of the whole name, so it is the same constant in the game and in every guest and moves
    /// with nothing but the name. The generator refuses two names with one id.
    pub fn export_id(&self) -> u64 {
        let digest = digest(&format!("id{{{}}}", self.text()));
        u64::from_str_radix(&digest[..16], 16).expect("hex") & !POINT_TAG
    }

    /// A point listener's id: [`Name::export_id`] with [`POINT_TAG`] set.
    pub fn point_id(&self) -> u64 {
        self.export_id() | POINT_TAG
    }
}

/// A word of the shared protocol, at protocol number `number`.
pub fn protocol_name(word: &str, number: u32) -> Name {
    Name {
        kind: Kind::Protocol,
        path: word.to_owned(),
        version: None,
        hash: digest(&format!("protocol{{{word};{number}}}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec<'a>(params: &'a [String], ret: &'a str) -> Spec<'a> {
        Spec {
            kind: Kind::Call,
            owner: "ServerHost",
            method: "give_item",
            receiver: Some(true),
            extra: "",
            params,
            ret,
            version: None,
        }
    }

    #[test]
    fn a_name_round_trips_and_names_its_item() {
        let params = vec!["a".to_owned(), "b".to_owned()];
        let name = spec(&params, "r").name();
        assert_eq!(Name::parse(&name.text()), Ok(name.clone()));
        assert_eq!(name.item(), "host:ServerHost::give_item");
        let hook = Name {
            kind: Kind::Hook,
            version: Some(3),
            path: "Hooks::block_click".into(),
            ..name
        };
        assert_eq!(hook.item(), "hook:Hooks::block_click@3");
        assert_eq!(Name::parse(&hook.text()), Ok(hook));
        // A generic owner as the source writes it has a space after its comma: the name keeps
        // its characters a path has and still parses.
        let mut generic = spec(&params, "r");
        generic.owner = "ResourceHandler<A, B>";
        let generic = generic.name();
        assert_eq!(generic.path, "ResourceHandler<A,_B>::give_item");
        assert_eq!(Name::parse(&generic.text()), Ok(generic));
    }

    #[test]
    fn the_hash_follows_signature_and_version_and_nothing_else() {
        let params = vec!["a".to_owned(), "b".to_owned()];
        let base = spec(&params, "r").name();
        assert_eq!(base, spec(&params, "r").name());
        assert_ne!(base.hash, spec(&params, "other").name().hash);
        assert_ne!(base.hash, spec(&params[..1], "r").name().hash);
        let mut versioned = spec(&params, "r");
        versioned.version = Some(2);
        assert_ne!(base.hash, versioned.name().hash);
        let mut shared = spec(&params, "r");
        shared.receiver = Some(false);
        assert_ne!(base.hash, shared.name().hash);
        // Each shape carries its length, so no parameter list imitates another.
        let split = vec!["a,b".to_owned()];
        assert_ne!(base.hash, spec(&split, "r").name().hash);
    }

    #[test]
    fn malformed_names_are_typed_refusals() {
        let good = protocol_name("log", 1).text();
        assert!(Name::parse(&good).is_ok());
        let hash = "0123456789abcdef0123456789abcdef";
        let cases = [
            ("".to_owned(), NameError::Kind),
            ("call".to_owned(), NameError::Kind),
            (format!("other:A::b#{hash}"), NameError::Kind),
            ("call:A::b".to_owned(), NameError::Hash),
            (format!("call:A::b#{}", &hash[..31]), NameError::Hash),
            (
                format!("call:A::b#{}", hash.to_uppercase()),
                NameError::Hash,
            ),
            (format!("call:#{hash}"), NameError::Path),
            (format!("call:A b#{hash}"), NameError::Path),
            (format!("call:A::b@x#{hash}"), NameError::Version),
            (format!("call:A::b@01#{hash}"), NameError::Version),
            (format!("call:A::b@-1#{hash}"), NameError::Version),
            (
                format!("call:{}#{hash}", "a".repeat(MAX_NAME_BYTES)),
                NameError::TooLong,
            ),
            (format!("call:é#{hash}"), NameError::Path),
        ];
        for (text, error) in cases {
            assert_eq!(Name::parse(&text), Err(error), "{text}");
        }
    }
}
