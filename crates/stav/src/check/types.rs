//! Types, declarations of structs and sum types, and refinements.

use std::collections::HashMap;

use super::load::FileId;
use crate::span::Span;

macro_rules! id {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub(crate) struct $name(pub(crate) u32);

        impl $name {
            pub(crate) fn new(index: usize) -> Self {
                Self(u32::try_from(index).unwrap_or(u32::MAX))
            }

            pub(crate) fn index(self) -> usize {
                self.0 as usize
            }
        }
    };
}

id!(
    /// An interned [`Type`].
    TypeId
);
id!(
    /// A declared struct.
    StructId
);
id!(
    /// A declared sum type.
    SumId
);
id!(
    /// A host symbol type.
    SymbolId
);
id!(
    /// A refinement: the predicates of one `where` clause.
    RefinementId
);
id!(
    /// A top-level function or `let` of a file.
    GlobalId
);

/// A type. Types are interned, so equal types have equal [`TypeId`]s.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum Type {
    /// The type of an expression whose type could not be determined because
    /// of an error that has already been reported. It is compatible with
    /// every type, so that one error does not cause others.
    Error,
    Bool,
    Int,
    Float,
    Str,
    Path,
    Data,
    List(TypeId),
    Set(TypeId),
    Map(TypeId, TypeId),
    Option(TypeId),
    Ref(StructId),
    Part(StructId),
    Fn(Vec<TypeId>, TypeId),
    Struct(StructId),
    Sum(SumId),
    Symbol(SymbolId),
    /// A type restricted by the predicates of a refinement. Refinements are
    /// checked when the program is evaluated, so they play no part in type
    /// compatibility.
    Refined(TypeId, RefinementId),
}

/// A field of a struct.
#[derive(Debug, Clone)]
pub(crate) struct FieldDef {
    pub(crate) name: String,
    pub(crate) span: Span,
    /// The declared type, with its refinements.
    pub(crate) ty: TypeId,
    /// The span of the type as written.
    pub(crate) ty_span: Span,
    /// The node index of the default expression in the declaring file.
    pub(crate) default: Option<usize>,
    /// The block kind of a `block kind` clause.
    pub(crate) block: Option<String>,
    pub(crate) doc: Option<String>,
}

impl FieldDef {
    /// Returns whether the field has neither a default nor an `Option`
    /// type, so that the final value must set it.
    pub(crate) fn is_required(&self, types: &Types) -> bool {
        self.default.is_none() && !matches!(types.get(types.peel(self.ty)), Type::Option(_))
    }
}

/// A struct declaration.
#[derive(Debug, Clone)]
pub(crate) struct StructDef {
    pub(crate) name: String,
    pub(crate) file: FileId,
    pub(crate) span: Span,
    pub(crate) fields: Vec<FieldDef>,
    pub(crate) doc: Option<String>,
}

impl StructDef {
    pub(crate) fn field(&self, name: &str) -> Option<(usize, &FieldDef)> {
        self.fields.iter().enumerate().find(|(_, f)| f.name == name)
    }
}

/// A parameter of a function, or a payload field of a variant.
#[derive(Debug, Clone)]
pub(crate) struct ParamDef {
    pub(crate) name: String,
    pub(crate) span: Span,
    /// The declared type, with its refinements.
    pub(crate) ty: TypeId,
    /// The span of the type as written.
    pub(crate) ty_span: Span,
    /// The node index of the default expression in the declaring file.
    pub(crate) default: Option<usize>,
}

/// A variant of a sum type.
#[derive(Debug, Clone)]
pub(crate) struct VariantDef {
    pub(crate) name: String,
    /// The payload fields, or `None` for a nullary variant.
    pub(crate) fields: Option<Vec<ParamDef>>,
    pub(crate) doc: Option<String>,
}

/// A sum type declaration.
#[derive(Debug, Clone)]
pub(crate) struct SumDef {
    pub(crate) name: String,
    pub(crate) file: FileId,
    pub(crate) span: Span,
    pub(crate) variants: Vec<VariantDef>,
    pub(crate) doc: Option<String>,
}

impl SumDef {
    pub(crate) fn variant(&self, name: &str) -> Option<(usize, &VariantDef)> {
        self.variants
            .iter()
            .enumerate()
            .find(|(_, v)| v.name == name)
    }

    /// Returns whether every variant is nullary.
    pub(crate) fn is_nullary(&self) -> bool {
        self.variants.iter().all(|v| v.fields.is_none())
    }
}

/// A host symbol type.
#[derive(Debug, Clone)]
pub(crate) struct SymbolDef {
    pub(crate) name: String,
    pub(crate) words: bool,
    pub(crate) strings: bool,
    pub(crate) constructor: String,
}

/// A number in a range predicate.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Number {
    Int(i64),
    Float(f64),
}

/// One predicate of a refinement.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Predicate {
    /// A range on the value of a number, or on the length of a string or
    /// collection when `length` is set.
    Range {
        start: Option<Number>,
        end: Option<Number>,
        inclusive: bool,
        length: bool,
    },
    /// The length is at least one.
    NonEmpty,
    /// The whole string matches the regular expression.
    Matches(String),
    /// A call of a function that returns `Bool`, with the value as first
    /// argument and the node indexes of the extra arguments.
    Call {
        function: GlobalId,
        args: Vec<usize>,
    },
}

/// The predicates of one `where` clause, checked when the program is
/// evaluated.
#[derive(Debug, Clone)]
pub(crate) struct Refinement {
    #[expect(dead_code, reason = "read by the evaluator")]
    pub(crate) file: FileId,
    #[expect(dead_code, reason = "read by the evaluator")]
    pub(crate) span: Span,
    pub(crate) predicates: Vec<Predicate>,
}

/// The type table and every declaration of a program.
#[derive(Debug, Clone)]
pub(crate) struct Types {
    list: Vec<Type>,
    index: HashMap<Type, TypeId>,
    pub(crate) structs: Vec<StructDef>,
    pub(crate) sums: Vec<SumDef>,
    pub(crate) symbols: Vec<SymbolDef>,
    pub(crate) refinements: Vec<Refinement>,
}

impl TypeId {
    pub(crate) const ERROR: Self = Self(0);
    pub(crate) const BOOL: Self = Self(1);
    pub(crate) const INT: Self = Self(2);
    pub(crate) const FLOAT: Self = Self(3);
    pub(crate) const STR: Self = Self(4);
    pub(crate) const PATH: Self = Self(5);
    pub(crate) const DATA: Self = Self(6);
}

impl Types {
    pub(crate) fn new() -> Self {
        let mut types = Self {
            list: Vec::new(),
            index: HashMap::new(),
            structs: Vec::new(),
            sums: Vec::new(),
            symbols: Vec::new(),
            refinements: Vec::new(),
        };
        for ty in [
            Type::Error,
            Type::Bool,
            Type::Int,
            Type::Float,
            Type::Str,
            Type::Path,
            Type::Data,
        ] {
            types.intern(ty);
        }
        types
    }

    pub(crate) fn intern(&mut self, ty: Type) -> TypeId {
        if let Some(&id) = self.index.get(&ty) {
            return id;
        }
        let id = TypeId::new(self.list.len());
        self.list.push(ty.clone());
        self.index.insert(ty, id);
        id
    }

    pub(crate) fn get(&self, id: TypeId) -> &Type {
        self.list.get(id.index()).unwrap_or(&Type::Error)
    }

    pub(crate) fn list(&mut self, elem: TypeId) -> TypeId {
        self.intern(Type::List(elem))
    }

    pub(crate) fn set(&mut self, elem: TypeId) -> TypeId {
        self.intern(Type::Set(elem))
    }

    pub(crate) fn map(&mut self, key: TypeId, value: TypeId) -> TypeId {
        self.intern(Type::Map(key, value))
    }

    /// Returns `Option<inner>`, or `inner` itself when it already is an
    /// option, so that options never nest.
    pub(crate) fn option(&mut self, inner: TypeId) -> TypeId {
        if matches!(self.get(self.peel(inner)), Type::Option(_)) || inner == TypeId::ERROR {
            return inner;
        }
        self.intern(Type::Option(inner))
    }

    /// Returns the type without the refinements at its top.
    pub(crate) fn peel(&self, mut id: TypeId) -> TypeId {
        while let Type::Refined(base, _) = self.get(id) {
            id = *base;
        }
        id
    }

    /// Returns the type without refinements at its top, and the content
    /// type of an `Option` without its refinements.
    pub(crate) fn unwrap_option(&self, id: TypeId) -> Option<TypeId> {
        match self.get(self.peel(id)) {
            Type::Option(inner) => Some(self.peel(*inner)),
            _ => None,
        }
    }

    /// Returns whether the type contains [`Type::Error`] anywhere.
    pub(crate) fn has_error(&self, id: TypeId) -> bool {
        match self.get(id) {
            Type::Error => true,
            Type::List(t) | Type::Set(t) | Type::Option(t) | Type::Refined(t, _) => {
                self.has_error(*t)
            }
            Type::Map(k, v) => self.has_error(*k) || self.has_error(*v),
            Type::Fn(params, ret) => {
                params.iter().any(|p| self.has_error(*p)) || self.has_error(*ret)
            }
            _ => false,
        }
    }

    /// Returns whether the two types are equal once refinements are left
    /// out at every depth.
    pub(crate) fn same(&self, a: TypeId, b: TypeId) -> bool {
        let (a, b) = (self.peel(a), self.peel(b));
        if a == b {
            return true;
        }
        match (self.get(a), self.get(b)) {
            (Type::List(x), Type::List(y))
            | (Type::Set(x), Type::Set(y))
            | (Type::Option(x), Type::Option(y)) => self.same(*x, *y),
            (Type::Map(k1, v1), Type::Map(k2, v2)) => self.same(*k1, *k2) && self.same(*v1, *v2),
            (Type::Fn(p1, r1), Type::Fn(p2, r2)) => {
                p1.len() == p2.len()
                    && p1.iter().zip(p2).all(|(x, y)| self.same(*x, *y))
                    && self.same(*r1, *r2)
            }
            _ => false,
        }
    }

    /// Returns whether a value of type `found` is accepted where `expected`
    /// is, without coercions. A type that contains an error is compatible
    /// with everything.
    pub(crate) fn fits(&self, found: TypeId, expected: TypeId) -> bool {
        self.has_error(found) || self.has_error(expected) || self.same(found, expected)
    }

    /// Returns whether values of the type can be set elements: `Bool`,
    /// `Int`, `Str`, `Path`, nullary sum types, symbol types and `Ref<T>`.
    pub(crate) fn is_key(&self, id: TypeId) -> bool {
        match self.get(self.peel(id)) {
            Type::Error | Type::Bool | Type::Int | Type::Str | Type::Path => true,
            Type::Symbol(_) | Type::Ref(_) => true,
            Type::Sum(s) => self.sums[s.index()].is_nullary(),
            _ => false,
        }
    }

    /// Returns whether values of the type can be map keys: `Str` and
    /// nullary sum types.
    pub(crate) fn is_map_key(&self, id: TypeId) -> bool {
        match self.get(self.peel(id)) {
            Type::Error | Type::Str => true,
            Type::Sum(s) => self.sums[s.index()].is_nullary(),
            _ => false,
        }
    }

    /// Returns whether the type contains a function or part type, which
    /// makes its values impossible to render.
    pub(crate) fn has_code(&self, id: TypeId) -> bool {
        match self.get(id) {
            Type::Fn(..) | Type::Part(_) => true,
            Type::List(t) | Type::Set(t) | Type::Option(t) | Type::Refined(t, _) => {
                self.has_code(*t)
            }
            Type::Map(k, v) => self.has_code(*k) || self.has_code(*v),
            _ => false,
        }
    }

    /// Returns whether values of the type are totally ordered: `Int`,
    /// `Float`, `Str` and nullary sum types.
    pub(crate) fn is_ordered(&self, id: TypeId) -> bool {
        match self.get(self.peel(id)) {
            Type::Error | Type::Int | Type::Float | Type::Str => true,
            Type::Sum(s) => self.sums[s.index()].is_nullary(),
            _ => false,
        }
    }

    /// Returns whether values of the type can be interpolated into a
    /// string.
    pub(crate) fn is_text(&self, id: TypeId) -> bool {
        match self.get(self.peel(id)) {
            Type::Error | Type::Str | Type::Int | Type::Float | Type::Bool => true,
            Type::Symbol(_) | Type::Ref(_) => true,
            Type::Sum(s) => self.sums[s.index()].is_nullary(),
            _ => false,
        }
    }

    /// Returns the struct of a struct type.
    pub(crate) fn as_struct(&self, id: TypeId) -> Option<StructId> {
        match self.get(self.peel(id)) {
            Type::Struct(s) => Some(*s),
            _ => None,
        }
    }

    /// Returns the sum of a sum type.
    pub(crate) fn as_sum(&self, id: TypeId) -> Option<SumId> {
        match self.get(self.peel(id)) {
            Type::Sum(s) => Some(*s),
            _ => None,
        }
    }

    pub(crate) fn structure(&self, id: StructId) -> &StructDef {
        &self.structs[id.index()]
    }

    pub(crate) fn sum(&self, id: SumId) -> &SumDef {
        &self.sums[id.index()]
    }

    /// Returns the type as it is written in source text.
    pub(crate) fn render(&self, id: TypeId) -> String {
        match self.get(id) {
            Type::Error => "{unknown}".to_owned(),
            Type::Bool => "Bool".to_owned(),
            Type::Int => "Int".to_owned(),
            Type::Float => "Float".to_owned(),
            Type::Str => "Str".to_owned(),
            Type::Path => "Path".to_owned(),
            Type::Data => "Data".to_owned(),
            Type::List(t) => format!("List<{}>", self.render(*t)),
            Type::Set(t) => format!("Set<{}>", self.render(*t)),
            Type::Map(k, v) => format!("Map<{}, {}>", self.render(*k), self.render(*v)),
            Type::Option(t) => format!("Option<{}>", self.render(*t)),
            Type::Ref(s) => format!("Ref<{}>", self.structure(*s).name),
            Type::Part(s) => format!("Part<{}>", self.structure(*s).name),
            Type::Fn(params, ret) => {
                let params: Vec<String> = params.iter().map(|p| self.render(*p)).collect();
                format!("Fn({}) -> {}", params.join(", "), self.render(*ret))
            }
            Type::Struct(s) => self.structure(*s).name.clone(),
            Type::Sum(s) => self.sum(*s).name.clone(),
            Type::Symbol(s) => self.symbols[s.index()].name.clone(),
            Type::Refined(base, _) => format!("{} where ...", self.render(*base)),
        }
    }

    /// Returns the type as it is written, without its refinements.
    pub(crate) fn render_plain(&self, id: TypeId) -> String {
        match self.get(id) {
            Type::Refined(base, _) => self.render_plain(*base),
            Type::List(t) => format!("List<{}>", self.render_plain(*t)),
            Type::Set(t) => format!("Set<{}>", self.render_plain(*t)),
            Type::Map(k, v) => format!("Map<{}, {}>", self.render_plain(*k), self.render_plain(*v)),
            Type::Option(t) => format!("Option<{}>", self.render_plain(*t)),
            Type::Fn(params, ret) => {
                let params: Vec<String> = params.iter().map(|p| self.render_plain(*p)).collect();
                format!("Fn({}) -> {}", params.join(", "), self.render_plain(*ret))
            }
            _ => self.render(id),
        }
    }

    /// Returns the declaring file of a nominal type, for telling apart two
    /// types of the same name.
    pub(crate) fn origin(&self, id: TypeId) -> Option<FileId> {
        match self.get(self.peel(id)) {
            Type::Struct(s) => Some(self.structure(*s).file),
            Type::Sum(s) => Some(self.sum(*s).file),
            _ => None,
        }
    }
}
