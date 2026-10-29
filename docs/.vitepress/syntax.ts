import type { LanguageRegistration, ThemeRegistration } from 'shiki'

import grammar from './stav.tmLanguage.json'

/** The TextMate grammar of Stav, registered with Shiki as `stav`. */
export const stavLanguage = { ...grammar, name: 'stav', aliases: ['stv'] } as unknown as LanguageRegistration

/** The colors a code theme gives each kind of token. */
interface Palette {
  name: string
  type: 'light' | 'dark'
  fg: string
  bg: string
  comment: string
  doc: string
  keyword: string
  operator: string
  punctuation: string
  type_: string
  string: string
  escape: string
  number: string
  func: string
  property: string
  block: string
  label: string
  self: string
  invalid: string
}

function theme(p: Palette): ThemeRegistration {
  return {
    name: p.name,
    type: p.type,
    colors: { 'editor.background': p.bg, 'editor.foreground': p.fg },
    fg: p.fg,
    bg: p.bg,
    tokenColors: [
      { scope: ['comment', 'punctuation.definition.comment'], settings: { foreground: p.comment, fontStyle: 'italic' } },
      { scope: ['comment.line.documentation', 'comment.block.documentation'], settings: { foreground: p.doc, fontStyle: 'italic' } },
      { scope: ['markup.inline.raw.stav'], settings: { foreground: p.doc, fontStyle: '' } },
      {
        scope: ['keyword', 'storage', 'storage.type', 'storage.modifier', 'keyword.control', 'keyword.other'],
        settings: { foreground: p.keyword },
      },
      { scope: ['keyword.other.priority'], settings: { foreground: p.keyword, fontStyle: 'italic' } },
      {
        scope: ['keyword.operator', 'punctuation.definition.generic', 'punctuation.separator.type'],
        settings: { foreground: p.operator },
      },
      {
        scope: ['punctuation', 'meta.brace', 'punctuation.separator', 'punctuation.accessor'],
        settings: { foreground: p.punctuation },
      },
      {
        scope: [
          'entity.name.type',
          'support.type',
          'entity.name.namespace',
          'entity.name.type.declaration',
          'support.class',
          'entity.name.class',
          'entity.other.inherited-class',
        ],
        settings: { foreground: p.type_ },
      },
      { scope: ['string', 'string.unquoted.path'], settings: { foreground: p.string } },
      { scope: ['string.unquoted.path'], settings: { foreground: p.string, fontStyle: 'italic' } },
      { scope: ['punctuation.definition.string'], settings: { foreground: p.string } },
      {
        scope: [
          'constant.character.escape',
          'punctuation.section.interpolation',
          'string.regexp',
        ],
        settings: { foreground: p.escape },
      },
      { scope: ['meta.interpolation', 'source.stav.embedded'], settings: { foreground: p.fg } },
      {
        scope: ['constant.numeric', 'constant.language', 'constant.other', 'support.constant'],
        settings: { foreground: p.number },
      },
      {
        scope: ['entity.name.function', 'support.function', 'meta.function-call', 'variable.function'],
        settings: { foreground: p.func },
      },
      {
        scope: [
          'variable.other.property',
          'variable.other.member',
          'support.type.property-name',
          'entity.name.tag.toml',
          'entity.name.tag.yaml',
          'meta.object-literal.key',
          'variable.other.object.property',
        ],
        settings: { foreground: p.property },
      },
      { scope: ['entity.name.tag', 'entity.name.section'], settings: { foreground: p.block, fontStyle: 'bold' } },
      { scope: ['entity.name.label', 'constant.other.symbol', 'entity.name.lifetime'], settings: { foreground: p.label } },
      { scope: ['constant.other.symbol'], settings: { foreground: p.label, fontStyle: 'italic' } },
      { scope: ['variable.language', 'variable.parameter'], settings: { foreground: p.self, fontStyle: 'italic' } },
      { scope: ['variable.other.declaration'], settings: { foreground: p.fg, fontStyle: 'bold' } },
      { scope: ['invalid', 'invalid.illegal'], settings: { foreground: p.invalid, fontStyle: 'underline' } },
      { scope: ['markup.heading', 'markup.bold'], settings: { foreground: p.keyword, fontStyle: 'bold' } },
      { scope: ['markup.italic'], settings: { fontStyle: 'italic' } },
      { scope: ['markup.inserted'], settings: { foreground: p.string } },
      { scope: ['markup.deleted'], settings: { foreground: p.invalid } },
    ],
  }
}

/** Birch: dark tar ink cut into a pale timber panel. */
export const birch = theme({
  name: 'stav-birch',
  type: 'light',
  fg: '#2a2520',
  bg: '#ebe7dd',
  comment: '#6b6459',
  doc: '#4f6446',
  keyword: '#8a4f06',
  operator: '#6e5a3c',
  punctuation: '#6b6459',
  type_: '#1d6655',
  string: '#4a6a12',
  escape: '#8a4f06',
  number: '#7a3a86',
  func: '#5b3d0c',
  property: '#9c3a22',
  block: '#8a4f06',
  label: '#1f5a8a',
  self: '#9c3a22',
  invalid: '#b3261e',
})

/** Tar: resin and lichen on pine tar, for dark pages. */
export const tar = theme({
  name: 'stav-tar',
  type: 'dark',
  fg: '#e7e1d5',
  bg: '#151a1e',
  comment: '#86928f',
  doc: '#a7b894',
  keyword: '#e8ac4c',
  operator: '#c8b48e',
  punctuation: '#98928a',
  type_: '#7cc7b4',
  string: '#b2cf8c',
  escape: '#e8ac4c',
  number: '#d3a6df',
  func: '#f1dbac',
  property: '#eda88c',
  block: '#e8ac4c',
  label: '#8dbbe6',
  self: '#eda88c',
  invalid: '#ff8a7a',
})
