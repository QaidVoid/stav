//! Declarations: collecting the names of every file, resolving the types of
//! fields, variants and signatures, and the checks on the shape of types.

use std::collections::BTreeMap;

use super::load::{FileId, Role};
use super::model::{LocalDef, LocalId, SelfRead, Step};
use super::suggest::did_you_mean;
use super::totality::Node;
use super::types::{
    FieldDef, GlobalId, Number, ParamDef, Predicate, Refinement, RefinementId, StructDef, StructId,
    SumDef, SumId, SymbolDef, SymbolId, Type, TypeId, VariantDef,
};
use super::{Checker, Cx, SelfMode, graph};
use crate::ast::{self, Expr, Item, LiteralKind, Stmt, UnaryOp};
use crate::diagnostic::Diagnostic;
use crate::lexer::{parse_float, parse_int};
use crate::span::Span;
use crate::syntax::{SyntaxElement, SyntaxNode, SyntaxToken};
use crate::token::TokenKind;

/// The type names that only built-in types may use.
const RESERVED: [&str; 13] = [
    "Bool", "Int", "Float", "Str", "Path", "Data", "List", "Set", "Map", "Option", "Ref", "Part",
    "Fn",
];

/// A name in the value namespace of a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ValueItem {
    Global(GlobalId),
    /// An import alias, with the module or `None` when it failed to load.
    Module(Option<FileId>),
}

/// A name in the type namespace of a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TypeItem {
    Struct(StructId),
    Sum(SumId),
    Symbol(SymbolId),
}

/// The names a file declares or imports.
#[derive(Debug, Clone, Default)]
pub(crate) struct FileScope {
    pub(crate) values: BTreeMap<String, ValueItem>,
    pub(crate) types: BTreeMap<String, TypeItem>,
}

/// A top-level function or `let`.
#[derive(Debug, Clone)]
pub(crate) struct Global {
    pub(crate) name: String,
    pub(crate) file: FileId,
    /// The span of the name.
    pub(crate) span: Span,
    /// The node index of the declaration.
    pub(crate) node: usize,
    pub(crate) kind: GlobalKind,
}

#[derive(Debug, Clone)]
pub(crate) enum GlobalKind {
    Fn(FnSig),
    Let(LetGlobal),
}

/// The signature of a top-level function.
#[derive(Debug, Clone)]
pub(crate) struct FnSig {
    pub(crate) params: Vec<ParamDef>,
    pub(crate) ret: TypeId,
    /// The locals the parameters are bound to in the body.
    pub(crate) binds: Vec<LocalId>,
}

/// A top-level `let`.
#[derive(Debug, Clone)]
pub(crate) struct LetGlobal {
    /// The type annotation, with its refinements.
    pub(crate) annotation: Option<TypeId>,
    pub(crate) state: LetState,
    /// The start of the declaration, for finding forward references.
    pub(crate) start: usize,
}

/// How far the type of a top-level `let` is known.
#[derive(Debug, Clone)]
pub(crate) enum LetState {
    Pending,
    InProgress,
    Done { ty: TypeId, reads: Vec<SelfRead> },
}

/// A predicate that calls a function, resolved once every signature is
/// known.
pub(crate) struct PendingPredicate<'p> {
    /// The file the refinement is written in.
    pub(crate) file: FileId,
    /// The refinement the predicate belongs to.
    pub(crate) refinement: RefinementId,
    /// The type the refinement restricts.
    pub(crate) base: TypeId,
    pub(crate) node: ast::NamedPredicate<'p>,
    /// The declaration that depends on the predicate function.
    pub(crate) owner: Option<Node>,
}

/// The arguments of a predicate call after the refined value, with the
/// parameter types they are checked against. They are checked once every
/// top-level `let` is, so that checking them never recurses through a
/// chain of `let` values.
pub(crate) struct PredicateArgs<'p> {
    pub(crate) file: FileId,
    pub(crate) owner: Option<Node>,
    pub(crate) args: Vec<(Expr<'p>, TypeId)>,
}

/// Returns the doc comment directly before `node`, without the `##`
/// markers.
pub(crate) fn doc_comment(node: SyntaxNode<'_>) -> Option<String> {
    let parent = node.parent()?;
    let mut lines: Vec<&str> = Vec::new();
    for element in parent.children_with_tokens() {
        match element {
            SyntaxElement::Node(child) if child.index() == node.index() => break,
            SyntaxElement::Token(token) if token.kind() == TokenKind::DocComment => {
                lines.push(token.text());
            }
            SyntaxElement::Token(token)
                if matches!(
                    token.kind(),
                    TokenKind::Newline | TokenKind::Whitespace | TokenKind::Pipe
                ) => {}
            _ => lines.clear(),
        }
    }
    if lines.is_empty() {
        return None;
    }
    let text: Vec<&str> = lines
        .iter()
        .map(|line| {
            let line = line.trim_end();
            let line = line.strip_prefix("##").unwrap_or(line);
            line.strip_prefix(' ').unwrap_or(line)
        })
        .collect();
    Some(text.join("\n"))
}

fn token_name(token: Option<SyntaxToken<'_>>) -> Option<(String, Span)> {
    token.map(|t| (t.text().to_owned(), t.span()))
}

impl<'p> Checker<'p> {
    /// Collects the names every file declares and imports, and builds the
    /// prelude scope.
    pub(super) fn declare(&mut self) {
        for (i, symbol) in self.host.symbols.iter().enumerate() {
            self.types.symbols.push(SymbolDef {
                name: symbol.name.clone(),
                words: symbol.words,
                strings: symbol.strings,
                constructor: symbol.constructor.clone(),
            });
            self.prelude
                .types
                .entry(symbol.name.clone())
                .or_insert(TypeItem::Symbol(SymbolId::new(i)));
        }
        let program = self.program;
        for i in 0..program.files.len() {
            self.declare_file(FileId::new(i));
        }
        for &file in &program.prelude {
            let scope = self.scopes[file.index()].clone();
            for (name, item) in scope.values {
                self.prelude.values.entry(name).or_insert(item);
            }
            for (name, item) in scope.types {
                self.prelude.types.entry(name).or_insert(item);
            }
        }
        for (i, file) in program.files.iter().enumerate() {
            if file.role == Role::Part {
                continue;
            }
            self.check_prelude_shadowing(FileId::new(i));
        }
    }

    fn declare_file(&mut self, id: FileId) {
        let file = self.program.file(id);
        let mut scope = FileScope::default();
        let mut spans: BTreeMap<String, Span> = BTreeMap::new();
        for link in &file.imports {
            if let Some(&first) = spans.get(&link.alias) {
                self.duplicate(id, &link.alias, link.alias_span, first);
                continue;
            }
            spans.insert(link.alias.clone(), link.alias_span);
            scope
                .values
                .insert(link.alias.clone(), ValueItem::Module(link.target));
        }
        let mut type_spans: BTreeMap<String, Span> = BTreeMap::new();
        let items = file
            .parsed
            .file()
            .items()
            .filter(|_| file.role != Role::Part);
        for item in items {
            match item {
                Item::Struct(decl) => {
                    let Some((name, span)) = token_name(decl.name()) else {
                        continue;
                    };
                    if !self.type_name_ok(id, &name, span, &type_spans) {
                        continue;
                    }
                    type_spans.insert(name.clone(), span);
                    let sid = StructId::new(self.types.structs.len());
                    self.types.structs.push(StructDef {
                        name: name.clone(),
                        file: id,
                        span,
                        fields: Vec::new(),
                        doc: doc_comment(decl.syntax()),
                    });
                    scope.types.insert(name, TypeItem::Struct(sid));
                }
                Item::Type(decl) => {
                    let Some((name, span)) = token_name(decl.name()) else {
                        continue;
                    };
                    if !self.type_name_ok(id, &name, span, &type_spans) {
                        continue;
                    }
                    type_spans.insert(name.clone(), span);
                    let sid = SumId::new(self.types.sums.len());
                    self.types.sums.push(SumDef {
                        name: name.clone(),
                        file: id,
                        span,
                        variants: Vec::new(),
                        doc: doc_comment(decl.syntax()),
                    });
                    scope.types.insert(name, TypeItem::Sum(sid));
                }
                Item::Fn(decl) => {
                    let Some((name, span)) = token_name(decl.name()) else {
                        continue;
                    };
                    if let Some(&first) = spans.get(&name) {
                        self.duplicate(id, &name, span, first);
                        continue;
                    }
                    spans.insert(name.clone(), span);
                    let g = GlobalId::new(self.globals.len());
                    self.globals.push(Global {
                        name: name.clone(),
                        file: id,
                        span,
                        node: decl.syntax().index(),
                        kind: GlobalKind::Fn(FnSig {
                            params: Vec::new(),
                            ret: TypeId::ERROR,
                            binds: Vec::new(),
                        }),
                    });
                    scope.values.insert(name, ValueItem::Global(g));
                }
                Item::Stmt(Stmt::Let(decl)) => {
                    let Some((name, span)) = token_name(decl.name()) else {
                        continue;
                    };
                    if let Some(&first) = spans.get(&name) {
                        self.duplicate(id, &name, span, first);
                        continue;
                    }
                    spans.insert(name.clone(), span);
                    let g = GlobalId::new(self.globals.len());
                    self.globals.push(Global {
                        name: name.clone(),
                        file: id,
                        span,
                        node: decl.syntax().index(),
                        kind: GlobalKind::Let(LetGlobal {
                            annotation: None,
                            state: LetState::Pending,
                            start: decl.syntax().span().start,
                        }),
                    });
                    scope.values.insert(name, ValueItem::Global(g));
                }
                Item::Import(_) | Item::Stmt(_) => {}
            }
        }
        self.scopes[id.index()] = scope;
    }

    fn type_name_ok(
        &mut self,
        file: FileId,
        name: &str,
        span: Span,
        seen: &BTreeMap<String, Span>,
    ) -> bool {
        if RESERVED.contains(&name) {
            self.error(
                file,
                Diagnostic::error(
                    "E0301",
                    span,
                    format!("`{name}` is the name of a built-in type"),
                )
                .with_help("choose another name"),
            );
            return false;
        }
        if let Some(&first) = seen.get(name) {
            self.duplicate(file, name, span, first);
            return false;
        }
        true
    }

    fn duplicate(&mut self, file: FileId, name: &str, span: Span, first: Span) {
        self.error(
            file,
            Diagnostic::error(
                "E0306",
                span,
                format!("`{name}` is declared more than once"),
            )
            .with_label(None, first, "first declared here")
            .with_help("rename or remove one of the declarations"),
        );
    }

    /// Reports declarations that reuse a name of the prelude or of a host
    /// symbol type.
    fn check_prelude_shadowing(&mut self, file: FileId) {
        let scope = self.scopes[file.index()].clone();
        for (name, item) in &scope.types {
            let Some(found) = self.prelude.types.get(name) else {
                continue;
            };
            if found == item {
                continue;
            }
            let span = match item {
                TypeItem::Struct(s) => self.types.structure(*s).span,
                TypeItem::Sum(s) => self.types.sum(*s).span,
                TypeItem::Symbol(_) => continue,
            };
            self.shadows_prelude(file, name, span);
        }
        for (name, item) in &scope.values {
            let ValueItem::Global(g) = item else {
                continue;
            };
            if self
                .prelude
                .values
                .get(name)
                .is_some_and(|found| found != item)
            {
                let span = self.globals[g.index()].span;
                self.shadows_prelude(file, name, span);
            }
        }
    }

    fn shadows_prelude(&mut self, file: FileId, name: &str, span: Span) {
        self.error(
            file,
            Diagnostic::error(
                "E0304",
                span,
                format!("`{name}` is already declared by the prelude"),
            )
            .with_help("choose another name"),
        );
    }

    /// Resolves the types of every field, variant, signature and annotated
    /// top-level `let`.
    pub(super) fn resolve_declarations(&mut self) {
        let program = self.program;
        for (i, file) in program.files.iter().enumerate() {
            let id = FileId::new(i);
            if file.role == Role::Part {
                continue;
            }
            let scope = self.scopes[i].clone();
            for item in file.parsed.file().items() {
                match item {
                    Item::Struct(decl) => {
                        let Some(name) = decl.name() else { continue };
                        let Some(TypeItem::Struct(sid)) = scope.types.get(name.text()).copied()
                        else {
                            continue;
                        };
                        if self.types.structure(sid).span != name.span() {
                            continue;
                        }
                        self.resolve_struct(id, sid, decl);
                    }
                    Item::Type(decl) => {
                        let Some(name) = decl.name() else { continue };
                        let Some(TypeItem::Sum(sid)) = scope.types.get(name.text()).copied() else {
                            continue;
                        };
                        if self.types.sum(sid).span != name.span() {
                            continue;
                        }
                        self.resolve_sum(id, sid, decl);
                    }
                    Item::Fn(decl) => {
                        let Some(ValueItem::Global(g)) = decl
                            .name()
                            .and_then(|n| scope.values.get(n.text()).copied())
                        else {
                            continue;
                        };
                        if self.globals[g.index()].node == decl.syntax().index() {
                            self.resolve_fn(id, g, decl);
                        }
                    }
                    Item::Stmt(Stmt::Let(decl)) => {
                        let Some(ValueItem::Global(g)) = decl
                            .name()
                            .and_then(|n| scope.values.get(n.text()).copied())
                        else {
                            continue;
                        };
                        if self.globals[g.index()].node != decl.syntax().index() {
                            continue;
                        }
                        if let Some(ty) = decl.ty() {
                            let ty = self.resolve_type(id, ty, Some(Node::Global(g)));
                            if let GlobalKind::Let(l) = &mut self.globals[g.index()].kind {
                                l.annotation = Some(ty);
                            }
                        }
                    }
                    Item::Import(_) | Item::Stmt(_) => {}
                }
            }
        }
        self.check_pending_predicates();
    }

    fn resolve_struct(&mut self, file: FileId, sid: StructId, decl: ast::StructDecl<'p>) {
        let owner = Some(Node::StructDefaults(sid));
        let mut fields: Vec<FieldDef> = Vec::new();
        let mut seen: BTreeMap<String, Span> = BTreeMap::new();
        for field in decl.fields() {
            let Some((name, span)) = token_name(field.name()) else {
                continue;
            };
            if let Some(&first) = seen.get(&name) {
                self.duplicate(file, &name, span, first);
                continue;
            }
            seen.insert(name.clone(), span);
            let ty = match field.ty() {
                Some(ty) => self.resolve_type(file, ty, owner),
                None => TypeId::ERROR,
            };
            self.check_field_type(file, ty, field.ty().map_or(span, |t| t.syntax().span()));
            fields.push(FieldDef {
                name,
                span,
                ty,
                ty_span: field.ty().map_or(span, |t| t.syntax().span()),
                default: field.default().map(|d| d.syntax().index()),
                block: field.block_kind().map(|k| k.text().to_owned()),
                doc: doc_comment(field.syntax()),
            });
        }
        self.types.structs[sid.index()].fields = fields;
    }

    fn check_field_type(&mut self, file: FileId, ty: TypeId, span: Span) {
        if self.types.has_code(ty) {
            self.error(
                file,
                Diagnostic::error(
                    "E0412",
                    span,
                    format!("a field cannot hold `{}`", self.types.render(ty)),
                )
                .with_help("functions and parts are not data, so pass them as parameters instead"),
            );
        }
    }

    fn resolve_sum(&mut self, file: FileId, sid: SumId, decl: ast::TypeDecl<'p>) {
        let owner = Some(Node::SumDefaults(sid));
        let mut variants: Vec<VariantDef> = Vec::new();
        let mut seen: BTreeMap<String, Span> = BTreeMap::new();
        for variant in decl.variants() {
            let Some((name, span)) = token_name(variant.name()) else {
                continue;
            };
            if let Some(&first) = seen.get(&name) {
                self.duplicate(file, &name, span, first);
                continue;
            }
            seen.insert(name.clone(), span);
            let fields = variant.params().map(|params| {
                let params = self.resolve_params(file, params, owner);
                for (param, node) in params.iter().zip(
                    variant
                        .params()
                        .into_iter()
                        .flat_map(ast::ParamList::params),
                ) {
                    let span = node.ty().map_or(param.span, |t| t.syntax().span());
                    self.check_field_type(file, param.ty, span);
                }
                params
            });
            variants.push(VariantDef {
                name,
                fields,
                doc: doc_comment(variant.syntax()),
            });
        }
        self.types.sums[sid.index()].variants = variants;
    }

    pub(super) fn resolve_params(
        &mut self,
        file: FileId,
        params: ast::ParamList<'p>,
        owner: Option<Node>,
    ) -> Vec<ParamDef> {
        let mut out: Vec<ParamDef> = Vec::new();
        let mut seen: BTreeMap<String, Span> = BTreeMap::new();
        for param in params.params() {
            let Some((name, span)) = token_name(param.name()) else {
                continue;
            };
            if let Some(&first) = seen.get(&name) {
                self.duplicate(file, &name, span, first);
                continue;
            }
            seen.insert(name.clone(), span);
            let ty = match param.ty() {
                Some(ty) => self.resolve_type(file, ty, owner),
                None => {
                    self.error(
                        file,
                        Diagnostic::error(
                            "E0430",
                            span,
                            format!("parameter `{name}` needs a type annotation"),
                        )
                        .with_help(format!("write `{name}: Type`")),
                    );
                    TypeId::ERROR
                }
            };
            out.push(ParamDef {
                name,
                span,
                ty,
                ty_span: param.ty().map_or(span, |t| t.syntax().span()),
                default: param.default().map(|d| d.syntax().index()),
            });
        }
        out
    }

    fn resolve_fn(&mut self, file: FileId, g: GlobalId, decl: ast::FnDecl<'p>) {
        let owner = Some(Node::Global(g));
        let params = decl
            .params()
            .map(|p| self.resolve_params(file, p, owner))
            .unwrap_or_default();
        let ret = match decl.return_type() {
            Some(ty) => self.resolve_type(file, ty, owner),
            None => TypeId::ERROR,
        };
        if let GlobalKind::Fn(sig) = &mut self.globals[g.index()].kind {
            sig.params = params;
            sig.ret = ret;
        }
    }

    /// Returns the type `ty` written in `file` denotes, reporting unknown
    /// names and invalid types. Function predicates in refinements become
    /// dependencies of `owner`.
    pub(super) fn resolve_type(
        &mut self,
        file: FileId,
        ty: ast::Type<'p>,
        owner: Option<Node>,
    ) -> TypeId {
        match ty {
            ast::Type::Named(named) => self.resolve_named(file, named, owner),
            ast::Type::Fn(f) => {
                let params: Vec<TypeId> = f
                    .params()
                    .map(|p| self.resolve_type(file, p, owner))
                    .collect();
                let ret = match f.return_type() {
                    Some(r) => self.resolve_type(file, r, owner),
                    None => TypeId::ERROR,
                };
                self.types.intern(Type::Fn(params, ret))
            }
            ast::Type::Refined(refined) => {
                let Some(base) = refined.base() else {
                    return TypeId::ERROR;
                };
                let base = self.resolve_type(file, base, owner);
                let id = RefinementId::new(self.types.refinements.len());
                self.types.refinements.push(Refinement {
                    file,
                    span: refined.syntax().span(),
                    predicates: Vec::new(),
                });
                let mut predicates = Vec::new();
                for predicate in refined.predicates() {
                    if let Some(p) = self.predicate(file, base, predicate, id, owner) {
                        predicates.push(p);
                    }
                }
                self.types.refinements[id.index()].predicates = predicates;
                self.types.intern(Type::Refined(base, id))
            }
        }
    }

    fn resolve_named(
        &mut self,
        file: FileId,
        named: ast::NamedType<'p>,
        owner: Option<Node>,
    ) -> TypeId {
        let Some(path) = named.path() else {
            return TypeId::ERROR;
        };
        let segments: Vec<SyntaxToken<'p>> = path.segments().collect();
        let span = path.syntax().span();
        let args: Vec<ast::Type<'p>> = named.args().collect();
        let item = match segments.as_slice() {
            [name] => {
                let name = name.text();
                if RESERVED.contains(&name) {
                    return self.builtin_type(file, name, &args, named.syntax().span(), owner);
                }
                self.lookup_type(file, name)
            }
            [alias, name] => match self.scopes[file.index()].values.get(alias.text()) {
                Some(ValueItem::Module(Some(module))) => {
                    self.scopes[module.index()].types.get(name.text()).copied()
                }
                Some(ValueItem::Module(None)) => return TypeId::ERROR,
                _ => {
                    self.error(
                        file,
                        Diagnostic::error(
                            "E0302",
                            alias.span(),
                            format!("unknown module `{}`", alias.text()),
                        )
                        .with_optional_help(self.suggest_module(file, alias.text())),
                    );
                    return TypeId::ERROR;
                }
            },
            _ => return TypeId::ERROR,
        };
        let Some(item) = item else {
            let name = segments.last().map_or("", |t| t.text());
            let help = self.suggest_type(file, &segments);
            self.error(
                file,
                Diagnostic::error("E0302", span, format!("unknown type `{name}`"))
                    .with_optional_help(help),
            );
            return TypeId::ERROR;
        };
        if !args.is_empty() {
            self.error(
                file,
                Diagnostic::error(
                    "E0444",
                    named.syntax().span(),
                    format!("`{}` takes no type arguments", path.syntax().text()),
                ),
            );
        }
        self.types.intern(match item {
            TypeItem::Struct(s) => Type::Struct(s),
            TypeItem::Sum(s) => Type::Sum(s),
            TypeItem::Symbol(s) => Type::Symbol(s),
        })
    }

    /// Looks a type name up in a file and then in the prelude.
    pub(super) fn lookup_type(&self, file: FileId, name: &str) -> Option<TypeItem> {
        self.scopes[file.index()]
            .types
            .get(name)
            .or_else(|| self.prelude.types.get(name))
            .copied()
    }

    fn suggest_type(&self, file: FileId, segments: &[SyntaxToken<'_>]) -> Option<String> {
        let name = segments.last().map_or("", |t| t.text());
        let candidates: Vec<&str> = if segments.len() == 2 {
            match self.scopes[file.index()].values.get(segments[0].text()) {
                Some(ValueItem::Module(Some(m))) => self.scopes[m.index()]
                    .types
                    .keys()
                    .map(String::as_str)
                    .collect(),
                _ => Vec::new(),
            }
        } else {
            self.scopes[file.index()]
                .types
                .keys()
                .chain(self.prelude.types.keys())
                .map(String::as_str)
                .chain(RESERVED)
                .collect()
        };
        did_you_mean(name, candidates)
    }

    fn suggest_module(&self, file: FileId, name: &str) -> Option<String> {
        let candidates = self.scopes[file.index()]
            .values
            .iter()
            .filter(|(_, item)| matches!(item, ValueItem::Module(_)))
            .map(|(name, _)| name.as_str());
        did_you_mean(name, candidates)
    }

    fn builtin_type(
        &mut self,
        file: FileId,
        name: &str,
        args: &[ast::Type<'p>],
        span: Span,
        owner: Option<Node>,
    ) -> TypeId {
        let arity = match name {
            "List" | "Set" | "Option" | "Ref" | "Part" => 1,
            "Map" => 2,
            "Fn" => {
                self.error(
                    file,
                    Diagnostic::error("E0444", span, "`Fn` needs parameter and return types")
                        .with_help("write `Fn(A, B) -> R`"),
                );
                return TypeId::ERROR;
            }
            _ => 0,
        };
        if args.len() != arity {
            let message = match arity {
                0 => format!("`{name}` takes no type arguments"),
                1 => format!("`{name}` takes one type argument"),
                _ => format!("`{name}` takes two type arguments"),
            };
            self.error(file, Diagnostic::error("E0444", span, message));
            return TypeId::ERROR;
        }
        let resolved: Vec<TypeId> = args
            .iter()
            .map(|a| self.resolve_type(file, *a, owner))
            .collect();
        let arg_span = |i: usize| args.get(i).map_or(span, |a| a.syntax().span());
        match name {
            "Bool" => TypeId::BOOL,
            "Int" => TypeId::INT,
            "Float" => TypeId::FLOAT,
            "Str" => TypeId::STR,
            "Path" => TypeId::PATH,
            "Data" => TypeId::DATA,
            "List" => self.types.list(resolved[0]),
            "Set" => {
                if !self.types.is_key(resolved[0]) {
                    self.not_key(file, resolved[0], arg_span(0), "a set element");
                    return TypeId::ERROR;
                }
                self.types.set(resolved[0])
            }
            "Map" => {
                if !self.types.is_map_key(resolved[0]) {
                    self.not_key(file, resolved[0], arg_span(0), "a map key");
                    return TypeId::ERROR;
                }
                self.types.map(resolved[0], resolved[1])
            }
            "Option" => {
                if self.types.unwrap_option(resolved[0]).is_some() {
                    self.error(
                        file,
                        Diagnostic::error("E0413", span, "options cannot be nested")
                            .with_help("use a single `Option`"),
                    );
                    return TypeId::ERROR;
                }
                if resolved[0] == TypeId::ERROR {
                    return TypeId::ERROR;
                }
                self.types.intern(Type::Option(resolved[0]))
            }
            "Ref" | "Part" => {
                let Some(target) = self.types.as_struct(resolved[0]) else {
                    if !self.types.has_error(resolved[0]) {
                        self.error(
                            file,
                            Diagnostic::error(
                                "E0401",
                                arg_span(0),
                                format!(
                                    "`{name}` needs a struct type, found `{}`",
                                    self.types.render(resolved[0])
                                ),
                            ),
                        );
                    }
                    return TypeId::ERROR;
                };
                if name == "Ref" {
                    self.ref_uses.push((file, span, target));
                    self.types.intern(Type::Ref(target))
                } else {
                    self.types.intern(Type::Part(target))
                }
            }
            _ => TypeId::ERROR,
        }
    }

    fn not_key(&mut self, file: FileId, ty: TypeId, span: Span, what: &str) {
        if self.types.has_error(ty) {
            return;
        }
        let help = if what == "a map key" {
            "map keys are `Str` or a sum type whose variants have no fields"
        } else {
            "set elements are `Bool`, `Int`, `Str`, `Path`, sum types without fields, symbol types or `Ref<T>`"
        };
        self.error(
            file,
            Diagnostic::error(
                "E0411",
                span,
                format!("`{}` cannot be {what}", self.types.render(ty)),
            )
            .with_help(help),
        );
    }

    fn predicate(
        &mut self,
        file: FileId,
        base: TypeId,
        predicate: ast::Predicate<'p>,
        refinement: RefinementId,
        owner: Option<Node>,
    ) -> Option<Predicate> {
        let span = predicate.syntax().span();
        let plain = self.types.peel(base);
        let kind = self.types.get(plain).clone();
        let measured = matches!(
            kind,
            Type::Str | Type::List(_) | Type::Set(_) | Type::Map(..)
        );
        if kind == Type::Error {
            return None;
        }
        match predicate {
            ast::Predicate::Range(range) => {
                let numeric = matches!(kind, Type::Int | Type::Float);
                if !numeric && !measured {
                    self.error(
                        file,
                        Diagnostic::error(
                            "E0401",
                            span,
                            format!(
                                "a range applies to numbers, strings and collections, not `{}`",
                                self.types.render(base)
                            ),
                        ),
                    );
                    return None;
                }
                let start = self.bound(file, range.start(), &kind)?;
                let end = self.bound(file, range.end(), &kind)?;
                Some(Predicate::Range {
                    start,
                    end,
                    inclusive: range.is_inclusive(),
                    length: measured,
                })
            }
            ast::Predicate::Named(named) => {
                let segments: Vec<SyntaxToken<'p>> = named.segments().collect();
                let args: Vec<ast::Arg<'p>> =
                    named.args().map(|a| a.args().collect()).unwrap_or_default();
                match (segments.as_slice(), named.args().is_some()) {
                    ([name], false) if name.text() == "nonEmpty" => {
                        if !measured {
                            self.error(
                                file,
                                Diagnostic::error(
                                    "E0401",
                                    span,
                                    format!(
                                        "`nonEmpty` applies to strings and collections, not `{}`",
                                        self.types.render(base)
                                    ),
                                ),
                            );
                            return None;
                        }
                        Some(Predicate::NonEmpty)
                    }
                    ([name], true) if name.text() == "matches" => {
                        if kind != Type::Str {
                            self.error(
                                file,
                                Diagnostic::error(
                                    "E0401",
                                    span,
                                    format!(
                                        "`matches` applies to strings, not `{}`",
                                        self.types.render(base)
                                    ),
                                ),
                            );
                            return None;
                        }
                        let pattern = match args.as_slice() {
                            [arg] if arg.name().is_none() => match arg.value() {
                                Some(Expr::Str(s)) => s.value().map(|v| (v, s.syntax().span())),
                                _ => None,
                            },
                            _ => None,
                        };
                        let Some((pattern, pattern_span)) = pattern else {
                            self.error(
                                file,
                                Diagnostic::error(
                                    "E0401",
                                    span,
                                    "`matches` in a refinement takes one string literal",
                                )
                                .with_help("write `matches(r\"pattern\")`"),
                            );
                            return None;
                        };
                        if self.check_regex(file, &pattern, pattern_span) {
                            Some(Predicate::Matches(pattern))
                        } else {
                            None
                        }
                    }
                    _ => {
                        self.pending_predicates.push(PendingPredicate {
                            file,
                            refinement,
                            base,
                            node: named,
                            owner,
                        });
                        None
                    }
                }
            }
        }
    }

    /// Reports an invalid regular expression and returns whether it is
    /// valid.
    pub(super) fn check_regex(&mut self, file: FileId, pattern: &str, span: Span) -> bool {
        let Err(error) = super::builtins::whole_match_regex(pattern) else {
            return true;
        };
        self.error(
            file,
            Diagnostic::error(
                "E0415",
                span,
                format!("invalid regular expression: {error}"),
            ),
        );
        false
    }

    fn bound(
        &mut self,
        file: FileId,
        expr: Option<Expr<'p>>,
        kind: &Type,
    ) -> Option<Option<Number>> {
        let Some(expr) = expr else {
            return Some(None);
        };
        let (negative, literal) = match expr {
            Expr::Literal(l) => (false, l),
            Expr::Unary(u) if u.op() == Some(UnaryOp::Neg) => match u.operand() {
                Some(Expr::Literal(l)) => (true, l),
                _ => return None,
            },
            _ => return None,
        };
        let text = literal.token()?.text();
        let number = match literal.kind()? {
            LiteralKind::Int => {
                let value = parse_int(text)?;
                Number::Int(if negative {
                    value.checked_neg()?
                } else {
                    value
                })
            }
            LiteralKind::Float => {
                let value = parse_float(text)?;
                Number::Float(if negative { -value } else { value })
            }
            _ => return None,
        };
        match (kind, number) {
            (Type::Float, Number::Int(i)) => {
                #[allow(clippy::cast_precision_loss, reason = "bounds are small literals")]
                let value = i as f64;
                Some(Some(Number::Float(value)))
            }
            (Type::Float, n) | (Type::Int, n @ Number::Int(_)) => Some(Some(n)),
            (_, n @ Number::Int(_)) if !matches!(kind, Type::Int) => Some(Some(n)),
            _ => {
                let what = if matches!(kind, Type::Int) {
                    "a range on `Int` needs integer bounds"
                } else {
                    "a range on a length needs integer bounds"
                };
                self.error(file, Diagnostic::error("E0401", expr.syntax().span(), what));
                None
            }
        }
    }

    /// Resolves the predicates that call functions, once every signature
    /// is known. A type resolved inside a body calls it right after, so
    /// that the predicates of its refinements are resolved too.
    pub(super) fn check_pending_predicates(&mut self) {
        let pending = std::mem::take(&mut self.pending_predicates);
        for p in pending {
            if let Some(predicate) = self.function_predicate(&p) {
                self.types.refinements[p.refinement.index()]
                    .predicates
                    .push(predicate);
            }
        }
    }

    fn function_predicate(&mut self, p: &PendingPredicate<'p>) -> Option<Predicate> {
        let file = p.file;
        let segments: Vec<SyntaxToken<'p>> = p.node.segments().collect();
        let span = p.node.syntax().span();
        let found = match segments.as_slice() {
            [name] => self.lookup_value(file, name.text()),
            [alias, name] => match self.scopes[file.index()].values.get(alias.text()) {
                Some(ValueItem::Module(Some(m))) => {
                    self.scopes[m.index()].values.get(name.text()).copied()
                }
                Some(ValueItem::Module(None)) => return None,
                _ => None,
            },
            _ => None,
        };
        let name = segments.last().map_or("", |t| t.text());
        let Some(ValueItem::Global(g)) = found else {
            let candidates: Vec<String> = self.value_names(file);
            let help = did_you_mean(
                name,
                candidates
                    .iter()
                    .map(String::as_str)
                    .chain(["nonEmpty", "matches"]),
            );
            self.error(
                file,
                Diagnostic::error("E0302", span, format!("unknown predicate `{name}`"))
                    .with_optional_help(help),
            );
            return None;
        };
        let GlobalKind::Fn(sig) = &self.globals[g.index()].kind else {
            self.error(
                file,
                Diagnostic::error("E0401", span, format!("`{name}` is not a function")),
            );
            return None;
        };
        let sig = sig.clone();
        let base = self.types.render_plain(p.base);
        let args: Vec<ast::Arg<'p>> = p
            .node
            .args()
            .map(|a| a.args().collect())
            .unwrap_or_default();
        let fits = sig
            .params
            .first()
            .is_some_and(|first| self.types.fits(p.base, first.ty))
            && self.types.fits(sig.ret, TypeId::BOOL)
            && sig.params.len() == args.len() + 1;
        if !fits {
            let mut expected: Vec<String> = vec![base];
            expected.extend(args.iter().map(|_| "...".to_owned()));
            self.error(
                file,
                Diagnostic::error(
                    "E0401",
                    span,
                    format!(
                        "the predicate `{name}` must be a function of type `Fn({}) -> Bool`",
                        expected.join(", ")
                    ),
                ),
            );
            return None;
        }
        if let Some(owner) = p.owner {
            self.calls.depend(owner, Node::Global(g), file, span);
        }
        let mut checked = Vec::new();
        let mut nodes = Vec::new();
        for (arg, param) in args.iter().zip(sig.params.iter().skip(1)) {
            if arg.name().is_some() {
                self.error(
                    file,
                    Diagnostic::error(
                        "E0444",
                        arg.syntax().span(),
                        "predicate arguments are positional",
                    ),
                );
            }
            if let Some(value) = arg.value() {
                checked.push((value, param.ty));
                nodes.push(value.syntax().index());
            }
        }
        self.predicate_args.push(PredicateArgs {
            file,
            owner: p.owner,
            args: checked,
        });
        Some(Predicate::Call {
            function: g,
            args: nodes,
        })
    }

    /// Checks the queued arguments of predicate calls.
    pub(super) fn check_predicate_args(&mut self) {
        for queued in std::mem::take(&mut self.predicate_args) {
            let unit = self.file_unit(queued.file);
            let mut cx = Cx::new(
                unit,
                queued.file,
                SelfMode::Forbidden("a refinement"),
                queued.owner,
            );
            for (value, ty) in queued.args {
                self.expr(&mut cx, value, Some(ty));
            }
        }
    }

    /// Looks a value name up in a file and then in the prelude.
    pub(super) fn lookup_value(&self, file: FileId, name: &str) -> Option<ValueItem> {
        self.scopes[file.index()]
            .values
            .get(name)
            .or_else(|| self.prelude.values.get(name))
            .copied()
    }

    /// Returns the names of the value namespace of a file and the prelude.
    pub(super) fn value_names(&self, file: FileId) -> Vec<String> {
        self.scopes[file.index()]
            .values
            .keys()
            .chain(self.prelude.values.keys())
            .cloned()
            .collect()
    }

    /// Finds the root struct.
    pub(super) fn find_root(&mut self, name: &str) {
        let mut found = match self.prelude.types.get(name) {
            Some(TypeItem::Struct(s)) => Some(*s),
            _ => None,
        };
        for &entry in &self.program.entries {
            if found.is_some() {
                break;
            }
            if let Some(TypeItem::Struct(s)) = self.scopes[entry.index()].types.get(name) {
                found = Some(*s);
            }
        }
        if found.is_none() {
            let file = self.program.entries.first().copied();
            self.report(
                file,
                Diagnostic::error(
                    "E0710",
                    Span { start: 0, end: 0 },
                    format!("there is no root struct `{name}`"),
                )
                .with_help(format!(
                    "declare `struct {name} {{ ... }}` in the entry file"
                )),
            );
        }
        self.root = found;
    }

    /// Checks the shape of the declared types: no recursive types, valid
    /// `block` clauses, and a unique target map for every `Ref<T>`.
    pub(super) fn check_structure(&mut self) {
        self.check_recursion();
        self.check_block_clauses();
        self.check_ref_targets();
    }

    /// Returns the structs and sums a type contains, with the type, through
    /// collections and options but not through references.
    fn contained(&self, ty: TypeId, out: &mut Vec<usize>) {
        let structs = self.types.structs.len();
        match self.types.get(ty) {
            Type::Struct(s) => out.push(s.index()),
            Type::Sum(s) => out.push(structs + s.index()),
            Type::List(t) | Type::Set(t) | Type::Option(t) | Type::Refined(t, _) => {
                self.contained(*t, out);
            }
            Type::Map(k, v) => {
                self.contained(*k, out);
                self.contained(*v, out);
            }
            _ => {}
        }
    }

    fn check_recursion(&mut self) {
        let structs = self.types.structs.len();
        let total = structs + self.types.sums.len();
        let mut adj: Vec<Vec<usize>> = vec![Vec::new(); total];
        let mut spans: BTreeMap<(usize, usize), (FileId, Span)> = BTreeMap::new();
        for (i, s) in self.types.structs.iter().enumerate() {
            for field in &s.fields {
                let mut out = Vec::new();
                self.contained(field.ty, &mut out);
                for t in out {
                    adj[i].push(t);
                    spans.entry((i, t)).or_insert((s.file, field.span));
                }
            }
        }
        for (i, s) in self.types.sums.iter().enumerate() {
            for variant in &s.variants {
                for field in variant.fields.iter().flatten() {
                    let mut out = Vec::new();
                    self.contained(field.ty, &mut out);
                    for t in out {
                        adj[structs + i].push(t);
                        spans
                            .entry((structs + i, t))
                            .or_insert((s.file, field.span));
                    }
                }
            }
        }
        for list in &mut adj {
            list.sort_unstable();
            list.dedup();
        }
        let name = |i: usize| {
            if i < structs {
                self.types.structs[i].name.clone()
            } else {
                self.types.sums[i - structs].name.clone()
            }
        };
        let mut errors = Vec::new();
        for cycle in graph::cycles(&adj) {
            let first = cycle[0];
            let next = cycle.get(1).copied().unwrap_or(first);
            let Some(&(file, span)) = spans.get(&(first, next)) else {
                continue;
            };
            let mut chain: Vec<String> = cycle.iter().map(|&i| name(i)).collect();
            chain.push(name(first));
            errors.push((
                file,
                Diagnostic::error("E0410", span, format!("`{}` contains itself", name(first)))
                    .with_note(format!("cycle: {}", chain.join(" -> ")))
                    .with_help("values must be finite trees, so refer to other values by label with `Ref<T>`"),
            ));
        }
        for (file, diagnostic) in errors {
            self.error(file, diagnostic);
        }
    }

    fn check_block_clauses(&mut self) {
        let mut errors = Vec::new();
        for s in &self.types.structs {
            let mut kinds: BTreeMap<&str, Span> = BTreeMap::new();
            for field in &s.fields {
                let Some(kind) = &field.block else { continue };
                if let Some(&first) = kinds.get(kind.as_str()) {
                    errors.push((
                        s.file,
                        Diagnostic::error(
                            "E0306",
                            field.span,
                            format!(
                                "block kind `{kind}` is declared more than once in `{}`",
                                s.name
                            ),
                        )
                        .with_label(None, first, "first declared here"),
                    ));
                    continue;
                }
                kinds.insert(kind, field.span);
                let ty = self.types.peel(field.ty);
                let valid = match self.types.get(ty) {
                    Type::Map(_, v) => {
                        self.types.as_struct(*v).is_some() || self.types.has_error(*v)
                    }
                    Type::Sum(_) | Type::Error => true,
                    Type::Option(inner) => self.types.as_sum(*inner).is_some(),
                    _ => self.types.has_error(ty),
                };
                if !valid {
                    errors.push((
                        s.file,
                        Diagnostic::error(
                            "E0443",
                            field.span,
                            format!(
                                "a `block` clause needs a map of structs or a sum type, not `{}`",
                                self.types.render(field.ty)
                            ),
                        )
                        .with_help("declare the field as `Map<Str, T>` with a struct `T`"),
                    ));
                }
            }
        }
        for (file, diagnostic) in errors {
            self.error(file, diagnostic);
        }
    }

    fn check_ref_targets(&mut self) {
        let Some(root) = self.root else { return };
        let mut targets: BTreeMap<StructId, Vec<Vec<Step>>> = BTreeMap::new();
        let mut seen = vec![false; self.types.structs.len()];
        let mut queue = vec![(root, Vec::new())];
        seen[root.index()] = true;
        while let Some((s, path)) = queue.pop() {
            for (i, field) in self.types.structure(s).fields.iter().enumerate() {
                let ty = self.types.peel(field.ty);
                let mut step = path.clone();
                step.push(Step::Field(i));
                match self.types.get(ty) {
                    Type::Map(k, v)
                        if field.block.is_some() && self.types.same(*k, TypeId::STR) =>
                    {
                        if let Some(t) = self.types.as_struct(*v) {
                            targets.entry(t).or_default().push(step);
                        }
                    }
                    _ => {
                        let inner = self.types.unwrap_option(ty).unwrap_or(ty);
                        if let Some(next) = self.types.as_struct(inner)
                            && !seen[next.index()]
                        {
                            seen[next.index()] = true;
                            queue.push((next, step));
                        }
                    }
                }
            }
        }
        self.target_maps = Some(targets);
        self.check_ref_uses();
    }

    /// Checks that every `Ref<T>` resolved so far has exactly one target
    /// map, once the target maps are known. A type resolved inside a body
    /// calls it right after.
    pub(super) fn check_ref_uses(&mut self) {
        let Some(targets) = &self.target_maps else {
            return;
        };
        let mut errors = Vec::new();
        for (file, span, target) in std::mem::take(&mut self.ref_uses) {
            match targets.get(&target).map(Vec::as_slice) {
                Some([path]) => {
                    self.ref_targets.insert(target, path.clone());
                }
                found => {
                    let name = &self.types.structure(target).name;
                    let message = if found.is_some() {
                        format!("`Ref<{name}>` has more than one target map")
                    } else {
                        format!("`Ref<{name}>` has no target map")
                    };
                    let help = format!(
                        "declare exactly one field `Map<Str, {name}>` with a `block` clause reachable from the root struct"
                    );
                    errors.push((
                        file,
                        Diagnostic::error("E0414", span, message).with_help(help),
                    ));
                }
            }
        }
        for (file, diagnostic) in errors {
            self.error(file, diagnostic);
        }
    }

    /// Adds a local to the unit of `cx`.
    pub(super) fn local(
        &mut self,
        cx: &Cx,
        name: &str,
        ty: TypeId,
        reads: Vec<SelfRead>,
    ) -> LocalId {
        let top = match &cx.self_mode {
            SelfMode::Body(body) if !reads.is_empty() => Some(body.top),
            _ => None,
        };
        let unit = &mut self.units[cx.unit.index()];
        let id = LocalId::new(unit.locals.len());
        unit.locals.push(LocalDef {
            name: name.to_owned(),
            ty,
            reads,
            top,
        });
        id
    }
}
