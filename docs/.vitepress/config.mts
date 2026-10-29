import { defineConfig } from 'vitepress'

import { birch, stavLanguage, tar } from './syntax'

const repo = 'https://github.com/QaidVoid/stav'

export default defineConfig({
  title: 'Stav',
  description:
    'A statically typed, eager and total configuration language with block syntax. Programs merge by priority and render to JSON, TOML or YAML.',
  lang: 'en-US',
  base: process.env.DOCS_BASE ?? '/',
  cleanUrls: true,
  lastUpdated: true,
  appearance: true,

  head: [
    ['link', { rel: 'icon', type: 'image/svg+xml', href: `${process.env.DOCS_BASE ?? '/'}favicon.svg` }],
    ['meta', { name: 'theme-color', content: '#c9862b' }],
    ['meta', { property: 'og:type', content: 'website' }],
    ['meta', { property: 'og:title', content: 'Stav' }],
    [
      'meta',
      {
        property: 'og:description',
        content: 'Typed, total configuration. Checked before it runs, merged by priority, rendered to JSON, TOML or YAML.',
      },
    ],
  ],

  markdown: {
    languages: [stavLanguage],
    theme: { light: birch, dark: tar },
  },

  themeConfig: {
    logo: '/logo.svg',

    nav: [
      { text: 'Guide', link: '/guide/introduction', activeMatch: '/guide/' },
      { text: 'Reference', link: '/reference/lexical-structure', activeMatch: '/reference/' },
      { text: 'Examples', link: '/examples/application', activeMatch: '/examples/' },
      {
        text: '0.1.0',
        items: [
          { text: 'Changelog', link: `${repo}/blob/main/CHANGELOG.md` },
          { text: 'stav on crates.io', link: 'https://crates.io/crates/stav' },
          { text: 'API documentation', link: 'https://docs.rs/stav' },
        ],
      },
    ],

    sidebar: {
      '/': [
        {
          text: 'Guide',
          items: [
            { text: 'What Stav is', link: '/guide/introduction' },
            { text: 'Installation', link: '/guide/installation' },
            { text: 'A first program', link: '/guide/first-program' },
            { text: 'A tour of the language', link: '/guide/tour' },
            { text: 'Structuring configuration', link: '/guide/structuring' },
            { text: 'Late-bound self', link: '/guide/self' },
            { text: 'Imports and modules', link: '/guide/modules' },
            { text: 'Embedding in Rust', link: '/guide/embedding' },
          ],
        },
        {
          text: 'Reference',
          items: [
            { text: 'Lexical structure', link: '/reference/lexical-structure' },
            { text: 'Types', link: '/reference/types' },
            { text: 'Expressions and statements', link: '/reference/expressions' },
            { text: 'Merging rules', link: '/reference/merging' },
            { text: 'Builtins', link: '/reference/builtins' },
            { text: 'Totality and limits', link: '/reference/totality' },
            { text: 'The stav command', link: '/reference/cli' },
            { text: 'Diagnostics', link: '/reference/diagnostics' },
            { text: 'Error codes', link: '/reference/error-codes' },
          ],
        },
        {
          text: 'Examples',
          items: [{ text: 'Application configuration', link: '/examples/application' }],
        },
      ],
    },

    socialLinks: [{ icon: 'github', link: repo }],

    editLink: {
      pattern: `${repo}/edit/main/docs/:path`,
      text: 'Edit this page',
    },

    search: { provider: 'local' },

    outline: { level: [2, 3], label: 'On this page' },

    footer: {
      message: 'Released under the MIT or Apache 2.0 license, at your option.',
    },
  },
})
