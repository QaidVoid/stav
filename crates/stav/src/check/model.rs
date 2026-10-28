//! What the checker records for the evaluator.
//!
//! A program is checked in units. A unit is one file checked in one
//! context: modules, entry files and prelude files have one unit each, and
//! a part file has one unit per struct it is included into. A unit stores a
//! [`Fact`] for every node of its file that the evaluator needs to
//! understand, indexed by node index.
//!
//! The declarations of a file, such as function bodies, field defaults,
//! payload defaults, parameter defaults, the arguments of predicates in
//! refinements and the values of top-level `let` declarations, are facts of
//! the unit of the declaring file. Top-level `let` values are evaluated
//! once, in the order the checker computed, so that every value comes after
//! the values it uses.
//!
//! Refinements belong to declared types. The evaluator checks the
//! refinements of the declared type of a struct field on the final value,
//! of a parameter on each argument, including arguments left to their
//! defaults, of a payload field when the variant is built, and of an
//! annotated `let` on its value.
//!
//! A parameter or payload field without default whose type is an `Option`
//! may be left out of a call, and is then `none`.

use super::host::RelPath;
use super::load::FileId;
use super::types::{GlobalId, StructId, SumId, SymbolId, TypeId};

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
    /// A unit: a file checked in one context.
    UnitId
);
id!(
    /// A local binding of a unit: a parameter, a `let` inside a body or
    /// expression, a loop variable or a pattern binding.
    LocalId
);

/// A file checked in one context.
#[derive(Debug, Clone)]
pub(crate) struct Unit {
    pub(crate) file: FileId,
    /// The struct the top level of a part file is a body of. `None` for
    /// files that are not part files.
    pub(crate) target: Option<StructId>,
    /// The fact of every node, by node index.
    pub(crate) facts: Vec<Fact>,
    pub(crate) locals: Vec<LocalDef>,
}

/// A local binding.
#[derive(Debug, Clone)]
pub(crate) struct LocalDef {
    pub(crate) name: String,
    pub(crate) ty: TypeId,
    /// The `self` fields the value reads, directly or through other
    /// bindings. A binding that reads `self` is late.
    pub(crate) reads: Vec<SelfRead>,
    /// The body whose blocks the reads refer to, for a late binding.
    pub(crate) top: Option<u32>,
}

/// What the checker learned about one node.
#[derive(Debug, Clone, Default)]
pub(crate) enum Fact {
    /// Nothing: the node is not one the evaluator looks up.
    #[default]
    None,
    Expr(ExprFact),
    Attr(AttrFact),
    Block(BlockFact),
    Include(IncludeFact),
    Let(LetFact),
    For(ForFact),
    Pattern(#[expect(dead_code, reason = "read by the evaluator")] PatternFact),
}

/// The type and meaning of an expression.
#[derive(Debug, Clone)]
pub(crate) struct ExprFact {
    /// The type of the value the expression produces, before coercion.
    pub(crate) ty: TypeId,
    /// Whether the value is converted to `Data` for its position.
    pub(crate) to_data: bool,
    /// Whether the value is wrapped into an `Option` for its position.
    pub(crate) wrap: bool,
    #[expect(dead_code, reason = "read by the evaluator")]
    pub(crate) meaning: Meaning,
}

/// What an expression node stands for, where the syntax alone does not
/// tell.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Meaning {
    /// The node means what its syntax says.
    Plain,
    /// A local binding.
    Local(LocalId),
    /// A top-level function or `let`.
    Global(GlobalId),
    /// A nullary variant.
    Variant { sum: SumId, variant: usize },
    /// An identifier, symbol word or string literal read as a literal of a
    /// symbol type.
    Symbol(SymbolId),
    /// An identifier or string literal read as the label of a reference.
    RefLabel(StructId),
    /// A field of a struct value. With `optional`, the access is `?.`.
    Field { index: usize, optional: bool },
    /// The field `field` of the merged value of the enclosing block that
    /// [`SelfRead::depth`] names.
    SelfField(SelfRead),
    /// A path literal, resolved inside its import root. An empty `rel`
    /// names the import root itself.
    Path { root: usize, rel: Option<RelPath> },
    /// A call.
    Call(CallFact),
    /// A map or struct literal checked as a value of a struct.
    Struct(StructId),
    /// A list or map comprehension, or a lambda, with the locals it binds.
    Binds(Vec<LocalId>),
    /// A map comprehension whose key is an identifier, with what the
    /// identifier means.
    MapCompKey {
        binds: Vec<LocalId>,
        key: Box<Meaning>,
    },
}

/// What a call calls and which argument fills each parameter.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CallFact {
    pub(crate) callee: Callee,
    /// For each parameter in declaration order, the node index of the
    /// argument expression, or `None` when the default applies.
    pub(crate) args: Vec<Option<usize>>,
}

/// The function a call calls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Callee {
    /// A top-level function.
    Global(GlobalId),
    /// A built-in function.
    Builtin(super::builtins::Builtin),
    /// The constructor of a symbol type.
    Constructor(SymbolId),
    /// A payload variant.
    Variant { sum: SumId, variant: usize },
    /// The function value the callee expression evaluates to. Every
    /// argument is positional.
    Value,
}

/// A read of `self.field`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct SelfRead {
    /// The enclosing block, counted from the outermost block of the body
    /// the read belongs to: the root body of an entry file, the top level
    /// of a part file or a `part { }` body. In a field default, zero is the
    /// struct being defaulted.
    pub(crate) depth: usize,
    pub(crate) structure: StructId,
    pub(crate) field: usize,
}

/// The priority of a contribution written in a body, lowest first. The
/// declared default of a field ranks below all of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum Priority {
    Default,
    Plain,
    Force,
}

/// One step of an attribute path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Step {
    /// A struct field, or a payload field in a tagged block body. Stepping
    /// through an `Option` field makes the option present.
    Field(usize),
    /// A map key: the key text, or the variant name for a map keyed by a
    /// sum type.
    Key(String),
}

/// An attribute statement `path = value`.
#[derive(Debug, Clone)]
pub(crate) struct AttrFact {
    #[expect(dead_code, reason = "read by the evaluator")]
    pub(crate) steps: Vec<Step>,
    /// The type of the attribute the path ends at.
    pub(crate) ty: TypeId,
    pub(crate) priority: Priority,
    /// The node index of the contributed expression, inside the priority
    /// marker if there is one.
    #[expect(dead_code, reason = "read by the evaluator")]
    pub(crate) value: usize,
    /// The `self` fields the value reads. A contribution that reads `self`
    /// is late, and evaluated after merging.
    pub(crate) reads: Vec<SelfRead>,
}

/// What the body of a block statement is typed against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Target {
    Struct(StructId),
    /// A map, whose attributes are its entries.
    Map {
        key: TypeId,
        value: TypeId,
    },
    /// A variant of a sum type, whose attributes are its payload fields.
    Variant {
        sum: SumId,
        variant: usize,
    },
}

/// How a block statement maps to its attribute.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BlockRule {
    /// A `block kind` clause on a map field: the block adds the entry of
    /// its label.
    Clause,
    /// A field of struct type, or `Option` of one.
    Field,
    /// A map field, whose entries the body sets.
    MapField,
    /// A field of sum type: the label selects the variant, and the body
    /// sets its payload fields.
    Tagged,
}

/// The label of a block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LabelFact {
    /// A label known statically.
    Static(String),
    /// A string label with interpolation, evaluated when the statement is.
    Dynamic,
}

/// A block statement `kind label { body }`.
#[derive(Debug, Clone)]
pub(crate) struct BlockFact {
    pub(crate) rule: BlockRule,
    /// The field of the enclosing body the block contributes to.
    pub(crate) field: usize,
    pub(crate) label: Option<LabelFact>,
    pub(crate) target: Target,
}

/// An `include` statement.
#[derive(Debug, Clone, Copy)]
pub(crate) enum IncludeFact {
    /// A part file, checked in the given unit.
    File(UnitId),
    /// An expression of type `Part<S>`.
    Value,
}

/// A `let` statement, a top-level `let` or a binding of a `let ... in`
/// expression.
#[derive(Debug, Clone, Copy)]
pub(crate) enum LetFact {
    Local(LocalId),
    Global(GlobalId),
}

/// A `for` statement, with the locals its names are bound to.
#[derive(Debug, Clone)]
pub(crate) struct ForFact {
    pub(crate) binds: Vec<LocalId>,
}

/// What a pattern node matches.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum PatternFact {
    /// `_`.
    Wildcard,
    /// A name that binds the value.
    Binder(LocalId),
    /// A literal, compared with the value. For an `Option` scrutinee, a
    /// literal other than `none` matches the present value.
    Literal,
    /// A variant, with the payload field each field pattern matches and the
    /// binding of a field pattern without a nested pattern.
    Variant {
        sum: SumId,
        variant: usize,
        fields: Vec<(usize, Option<LocalId>)>,
    },
    /// Alternatives.
    Or,
}
