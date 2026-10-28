//! The declared structs and sum types of a checked program, for tooling.

use super::Checked;
use super::load::FileId;
use super::types::{FieldDef, ParamDef};

/// Every struct and sum type a checked program declares, with the doc
/// comments of the declarations, their fields and their variants.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SchemaIndex {
    /// The structs, in the order they were declared.
    pub structs: Vec<SchemaStruct>,
    /// The sum types, in the order they were declared.
    pub sums: Vec<SchemaSum>,
}

/// A declared struct.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaStruct {
    /// The name of the struct.
    pub name: String,
    /// The file that declares it.
    pub file: FileId,
    /// The doc comment, without the `##` markers.
    pub doc: Option<String>,
    /// The fields in declaration order.
    pub fields: Vec<SchemaField>,
}

/// A field of a struct or a payload field of a variant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaField {
    /// The name of the field.
    pub name: String,
    /// The type as it is written, refinements included.
    pub ty: String,
    /// The doc comment, without the `##` markers.
    pub doc: Option<String>,
    /// Whether a value must set the field: it has neither a default nor an
    /// `Option` type.
    pub required: bool,
    /// The block kind of a `block kind` clause.
    pub block: Option<String>,
}

/// A declared sum type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaSum {
    /// The name of the sum type.
    pub name: String,
    /// The file that declares it.
    pub file: FileId,
    /// The doc comment, without the `##` markers.
    pub doc: Option<String>,
    /// The variants in declaration order.
    pub variants: Vec<SchemaVariant>,
}

/// A variant of a sum type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaVariant {
    /// The name of the variant.
    pub name: String,
    /// The doc comment, without the `##` markers.
    pub doc: Option<String>,
    /// The payload fields, or `None` for a variant without payload.
    pub fields: Option<Vec<SchemaField>>,
}

impl Checked {
    /// Returns every declared struct field and sum variant with its doc
    /// comment, for tooling such as completion and documentation.
    pub fn schema(&self) -> SchemaIndex {
        let text = |file: FileId, span: crate::Span| {
            self.source(file)
                .and_then(|source| source.get(span.range()))
                .unwrap_or_default()
                .to_owned()
        };
        let field = |file: FileId, f: &FieldDef| SchemaField {
            name: f.name.clone(),
            ty: text(file, f.ty_span),
            doc: f.doc.clone(),
            required: f.is_required(&self.types),
            block: f.block.clone(),
        };
        let param = |file: FileId, p: &ParamDef| SchemaField {
            name: p.name.clone(),
            ty: text(file, p.ty_span),
            doc: None,
            required: p.default.is_none() && self.types.unwrap_option(p.ty).is_none(),
            block: None,
        };
        SchemaIndex {
            structs: self
                .types
                .structs
                .iter()
                .map(|s| SchemaStruct {
                    name: s.name.clone(),
                    file: s.file,
                    doc: s.doc.clone(),
                    fields: s.fields.iter().map(|f| field(s.file, f)).collect(),
                })
                .collect(),
            sums: self
                .types
                .sums
                .iter()
                .map(|s| SchemaSum {
                    name: s.name.clone(),
                    file: s.file,
                    doc: s.doc.clone(),
                    variants: s
                        .variants
                        .iter()
                        .map(|v| SchemaVariant {
                            name: v.name.clone(),
                            doc: v.doc.clone(),
                            fields: v
                                .fields
                                .as_ref()
                                .map(|fields| fields.iter().map(|p| param(s.file, p)).collect()),
                        })
                        .collect(),
                })
                .collect(),
        }
    }
}
