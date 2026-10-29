<script setup lang="ts">
import { ref } from 'vue'
import { withBase } from 'vitepress'

const formats = ['toml', 'json', 'yaml'] as const
type Output = (typeof formats)[number]
const format = ref<Output>('toml')

function choose(next: Output) {
  format.value = next
}

function step(event: KeyboardEvent) {
  const delta = event.key === 'ArrowRight' ? 1 : event.key === 'ArrowLeft' ? -1 : 0
  if (delta === 0) return
  event.preventDefault()
  const i = (formats.indexOf(format.value) + delta + formats.length) % formats.length
  format.value = formats[i]
  const tabs = (event.currentTarget as HTMLElement).querySelectorAll<HTMLButtonElement>('[role="tab"]')
  tabs[i]?.focus()
}

const properties = [
  {
    name: 'Typed',
    body: 'The whole program is type checked before any of it runs. Unknown names, broken imports and wrong types are reported up front, with the closest valid name as a hint.',
    code: 'weight: Int where 1..=100',
    link: '/reference/types',
  },
  {
    name: 'Total',
    body: 'Evaluation always ends. Functions cannot call themselves, types cannot be recursive, loops run over finite collections, and fuel limits stop runaway growth.',
    code: 'fold(range(1, 6), 1, f)',
    link: '/reference/totality',
  },
  {
    name: 'Eager',
    body: 'Statements run in order, once. The only deferred value is a late-bound self, read after merging along a graph that is checked to be acyclic.',
    code: 'url = "#{self.version}"',
    link: '/guide/self',
  },
  {
    name: 'Pure',
    body: 'Evaluation reads nothing but the source files it is given. No environment, no clock, no network. The same inputs give the same value, byte for byte.',
    code: 'stav eval --canonical',
    link: '/guide/introduction#pure-and-deterministic',
  },
  {
    name: 'Merging',
    body: 'Every file contributes to one value. Lists concatenate, maps merge per key, and single values settle by priority, from declared default up to force.',
    code: 'port = force(443)',
    link: '/reference/merging',
  },
  {
    name: 'Parts',
    body: 'Typed fragments of a body, written as functions or files. Pass parameters in, then include them wherever their struct fits.',
    code: 'include web("api")',
    link: '/guide/structuring',
  },
]

const stages = [
  { name: 'Check', body: 'Every file is parsed and the whole program is type checked against its root struct.' },
  { name: 'Evaluate', body: 'Statements run eagerly, in order, and each one contributes values at a path and a priority.' },
  { name: 'Merge', body: 'Contributions meet by path. The highest priority wins, collections combine, conflicts are reported.' },
  { name: 'Render', body: 'The final value is checked once more and printed as JSON, TOML or YAML, or read into Rust types.' },
]
</script>

<template>
  <div class="sv-home">
    <section class="hero">
      <div class="hero-text">
        <p class="eyebrow">
          <span class="eyebrow-mark" aria-hidden="true" />
          A configuration language
        </p>
        <h1>
          Typed configuration
          <span class="accent">that always finishes.</span>
        </h1>
        <p class="lead">
          Stav is a statically typed, eager and total configuration language with block syntax. It checks the whole
          program before it runs any of it, merges what every file contributes by priority, and renders the result to
          JSON, TOML or YAML.
        </p>
        <div class="actions">
          <a class="btn brand" :href="withBase('/guide/first-program')">Write a first program</a>
          <a class="btn alt" :href="withBase('/guide/introduction')">Why Stav</a>
        </div>
        <p class="install">
          <span class="prompt" aria-hidden="true">$</span>
          <code>cargo install stav-cli</code>
        </p>
      </div>

      <div class="bench" role="group" aria-label="A Stav program and the value it evaluates to">
        <div class="bench-file">
          <div class="bench-bar">
            <span class="file-tab">app.stav</span>
          </div>
          <div class="bench-code vp-doc">
            <slot name="source" />
          </div>
        </div>
        <div class="bench-out">
          <div class="bench-bar" role="tablist" aria-label="Output format" @keydown="step">
            <span class="cmd" aria-hidden="true">
              <span class="prompt">$</span> stav eval -o {{ format }} app.stav
            </span>
            <span class="tabs">
              <button
                v-for="f in formats"
                :id="`sv-tab-${f}`"
                :key="f"
                role="tab"
                type="button"
                :aria-selected="format === f"
                :aria-controls="`sv-panel-${f}`"
                :tabindex="format === f ? 0 : -1"
                @click="choose(f)"
              >
                {{ f.toUpperCase() }}
              </button>
            </span>
          </div>
          <div
            v-for="f in formats"
            v-show="format === f"
            :id="`sv-panel-${f}`"
            :key="f"
            class="bench-code vp-doc"
            role="tabpanel"
            :aria-labelledby="`sv-tab-${f}`"
          >
            <slot :name="f" />
          </div>
        </div>
      </div>
    </section>

    <section class="frame" aria-labelledby="sv-properties">
      <h2 id="sv-properties">Six properties, held by the language</h2>
      <p class="section-lead">
        These are not conventions to follow. The checker and the evaluator enforce them for every program.
      </p>
      <div class="beam" aria-hidden="true" />
      <ul class="posts">
        <li v-for="p in properties" :key="p.name" class="post">
          <h3>
            <a :href="withBase(p.link)">{{ p.name }}</a>
          </h3>
          <p>{{ p.body }}</p>
          <code>{{ p.code }}</code>
        </li>
      </ul>
      <div class="sill" aria-hidden="true" />
    </section>

    <section class="stages" aria-labelledby="sv-stages">
      <h2 id="sv-stages">From source files to one value</h2>
      <ol>
        <li v-for="(s, i) in stages" :key="s.name">
          <span class="num" aria-hidden="true">{{ i + 1 }}</span>
          <h3>{{ s.name }}</h3>
          <p>{{ s.body }}</p>
        </li>
      </ol>
    </section>

    <section class="closing">
      <h2>Raise the frame first. Fill it in after.</h2>
      <p>
        Declare the shape of your configuration once, then let every file, part and overlay fill it in. Stav keeps
        the pieces square.
      </p>
      <div class="actions center">
        <a class="btn brand" :href="withBase('/guide/installation')">Install Stav</a>
        <a class="btn alt" :href="withBase('/guide/tour')">Take the tour</a>
        <a class="btn ghost" :href="withBase('/guide/embedding')">Embed it in Rust</a>
      </div>
    </section>
  </div>
</template>

<style scoped>
.sv-home {
  max-width: 1200px;
  margin: 0 auto;
  padding: 0 24px 96px;
}

h1,
h2,
h3,
.btn,
.num,
.eyebrow {
  font-family: var(--sv-font-display);
}

h2 {
  margin: 0 0 12px;
  font-size: clamp(28px, 3.6vw, 38px);
  font-weight: 750;
  letter-spacing: -0.02em;
  line-height: 1.15;
}

.section-lead {
  max-width: 620px;
  margin: 0 0 36px;
  font-size: 17px;
  line-height: 1.65;
  color: var(--vp-c-text-2);
}

/* -------------------------------------------------------------- hero --- */

.hero {
  display: grid;
  grid-template-columns: minmax(0, 0.92fr) minmax(0, 1.08fr);
  gap: 56px;
  align-items: start;
  padding: 64px 0 88px;
}

.hero-text {
  padding-top: 36px;
}

.eyebrow {
  display: inline-flex;
  align-items: center;
  gap: 10px;
  margin: 0 0 22px;
  font-size: 13px;
  font-weight: 700;
  letter-spacing: 0.14em;
  text-transform: uppercase;
  color: var(--vp-c-text-2);
}

.eyebrow-mark {
  width: 22px;
  height: 9px;
  background:
    linear-gradient(var(--sv-beam), var(--sv-beam)) 0 0 / 100% 3px no-repeat,
    linear-gradient(var(--sv-post), var(--sv-post)) 3px 0 / 3px 100% no-repeat,
    linear-gradient(var(--sv-post), var(--sv-post)) calc(100% - 3px) 0 / 3px 100% no-repeat;
}

h1 {
  margin: 0;
  font-size: clamp(40px, 5.6vw, 66px);
  font-weight: 800;
  line-height: 1.04;
  letter-spacing: -0.03em;
  color: var(--vp-c-text-1);
}

.accent {
  display: block;
  color: var(--vp-c-brand-1);
}

.lead {
  max-width: 560px;
  margin: 26px 0 0;
  font-size: 18.5px;
  line-height: 1.65;
  color: var(--vp-c-text-2);
}

.actions {
  display: flex;
  flex-wrap: wrap;
  gap: 12px;
  margin-top: 34px;
}

.actions.center {
  justify-content: center;
}

.btn {
  display: inline-flex;
  align-items: center;
  height: 46px;
  padding: 0 22px;
  border: 2px solid transparent;
  border-radius: 3px;
  font-size: 15.5px;
  font-weight: 700;
  text-decoration: none;
  transition: transform 0.18s, background-color 0.18s, border-color 0.18s;
}

.btn:hover {
  transform: translateY(-2px);
}

.btn.brand {
  background: var(--sv-resin-bright);
  color: var(--sv-tar);
  box-shadow: inset 0 -3px 0 rgba(0, 0, 0, 0.18);
}

.btn.brand:hover {
  background: #e0a645;
}

.btn.alt {
  background: var(--vp-c-text-1);
  color: var(--vp-c-bg);
}

.btn.ghost {
  border-color: var(--vp-c-border);
  color: var(--vp-c-text-1);
}

.btn.ghost:hover {
  border-color: var(--sv-post);
}

.install {
  display: inline-flex;
  align-items: center;
  gap: 10px;
  margin: 26px 0 0;
  padding: 8px 14px;
  border-left: 3px solid var(--sv-post);
  background: var(--vp-c-bg-alt);
  font-family: var(--vp-font-family-mono);
  font-size: 14px;
}

.install code {
  font-family: inherit;
  color: var(--vp-c-text-1);
}

.prompt {
  color: var(--vp-c-brand-1);
  user-select: none;
}

/* ------------------------------------------------------------- bench --- */

.bench {
  display: grid;
  gap: 14px;
}

.bench-file,
.bench-out {
  border: 1px solid var(--vp-code-block-divider-color);
  border-left: 6px solid var(--sv-post);
  border-radius: 3px;
  background: var(--vp-code-block-bg);
  box-shadow: 0 22px 44px -32px rgba(27, 25, 22, 0.55);
  overflow: hidden;
}

.bench-bar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  min-height: 40px;
  padding: 0 8px 0 14px;
  border-bottom: 1px solid var(--vp-code-block-divider-color);
  background: var(--vp-code-tab-bg);
  font-family: var(--vp-font-family-mono);
  font-size: 13px;
  color: var(--vp-code-tab-text-color);
}

.file-tab {
  align-self: stretch;
  display: flex;
  align-items: center;
  color: var(--vp-code-tab-active-text-color);
  box-shadow: inset 0 -3px 0 var(--sv-beam);
}

.cmd {
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.tabs {
  display: flex;
  gap: 2px;
}

.tabs button {
  padding: 4px 10px;
  border-radius: 2px;
  font-family: var(--vp-font-family-mono);
  font-size: 12px;
  font-weight: 600;
  letter-spacing: 0.04em;
  color: var(--vp-code-tab-text-color);
  transition: color 0.15s, background-color 0.15s;
}

.tabs button:hover {
  color: var(--vp-code-tab-hover-text-color);
}

.tabs button[aria-selected='true'] {
  background: var(--sv-resin-bright);
  color: var(--sv-tar);
}

.bench-code :deep(div[class*='language-']) {
  margin: 0 !important;
  border: none !important;
  border-radius: 0 !important;
  background: transparent !important;
}

.bench-code :deep(div[class*='language-'] pre) {
  padding: 16px 0;
}

.bench-out .bench-code {
  min-height: 300px;
}

.bench-code :deep(div[class*='language-'] code) {
  font-size: 13.5px;
  line-height: 1.6;
}

.bench-code :deep(span.lang) {
  display: none;
}

/* ------------------------------------------------------------- frame --- */

.frame {
  margin-bottom: 104px;
}

.beam,
.sill {
  height: 8px;
  border-radius: 1px;
  background: var(--sv-beam);
}

.sill {
  height: 5px;
  background: var(--sv-post);
  opacity: 0.7;
}

.posts {
  display: grid;
  grid-template-columns: repeat(3, 1fr);
  margin: 0;
  padding: 0;
  list-style: none;
}

.post {
  position: relative;
  display: flex;
  flex-direction: column;
  padding: 30px 28px 28px;
  border-left: 5px solid var(--sv-post);
}

.post:nth-child(3n) {
  border-right: 5px solid var(--sv-post);
}

.post:nth-child(n + 4) {
  border-top: 1px solid var(--vp-c-divider);
}

.post h3 {
  margin: 0 0 10px;
  font-size: 23px;
  font-weight: 750;
  letter-spacing: -0.01em;
}

.post h3 a {
  color: var(--vp-c-text-1);
  text-decoration: none;
}

.post h3 a:hover {
  color: var(--vp-c-brand-1);
  text-decoration: underline;
  text-underline-offset: 4px;
}

.post p {
  flex: 1;
  margin: 0 0 18px;
  font-size: 15.5px;
  line-height: 1.65;
  color: var(--vp-c-text-2);
}

.post code {
  align-self: flex-start;
  padding: 3px 8px;
  border-radius: 2px;
  background: var(--vp-code-bg);
  color: var(--vp-code-color);
  font-family: var(--vp-font-family-mono);
  font-size: 13px;
}

/* ------------------------------------------------------------ stages --- */

.stages {
  margin-bottom: 104px;
}

.stages h2 {
  margin-bottom: 40px;
}

.stages ol {
  position: relative;
  display: grid;
  grid-template-columns: repeat(4, 1fr);
  gap: 28px;
  margin: 0;
  padding: 0;
  list-style: none;
}

.stages ol::before {
  content: '';
  position: absolute;
  top: 20px;
  left: 0;
  right: 0;
  height: 4px;
  background: var(--vp-c-divider);
}

.stages li {
  position: relative;
}

.num {
  position: relative;
  display: grid;
  place-items: center;
  width: 44px;
  height: 44px;
  margin-bottom: 18px;
  border-radius: 2px;
  background: var(--vp-c-text-1);
  color: var(--vp-c-bg);
  font-size: 19px;
  font-weight: 750;
  box-shadow: 0 0 0 6px var(--vp-c-bg);
}

.stages h3 {
  margin: 0 0 6px;
  font-size: 20px;
  font-weight: 700;
}

.stages p {
  margin: 0;
  font-size: 15.5px;
  line-height: 1.65;
  color: var(--vp-c-text-2);
}

/* ----------------------------------------------------------- closing --- */

.closing {
  padding: 64px 24px 68px;
  border-top: 8px solid var(--sv-beam);
  border-bottom: 5px solid var(--sv-post);
  background: var(--vp-c-bg-alt);
  text-align: center;
}

.closing p {
  max-width: 560px;
  margin: 0 auto;
  font-size: 17px;
  line-height: 1.65;
  color: var(--vp-c-text-2);
}

/* -------------------------------------------------------- responsive --- */

@media (max-width: 1000px) {
  .hero {
    grid-template-columns: 1fr;
    gap: 44px;
    padding-top: 44px;
  }

  .posts {
    grid-template-columns: repeat(2, 1fr);
  }

  .post:nth-child(3n) {
    border-right: none;
  }

  .post:nth-child(2n) {
    border-right: 5px solid var(--sv-post);
  }

  .post:nth-child(n + 3) {
    border-top: 1px solid var(--vp-c-divider);
  }

  .stages ol {
    grid-template-columns: repeat(2, 1fr);
  }

  .stages ol::before {
    display: none;
  }
}

@media (max-width: 640px) {
  .sv-home {
    padding: 0 16px 72px;
  }

  .posts,
  .stages ol {
    grid-template-columns: 1fr;
  }

  .post,
  .post:nth-child(2n) {
    border-right: 5px solid var(--sv-post);
  }

  .post:nth-child(n + 2) {
    border-top: 1px solid var(--vp-c-divider);
  }

  .post {
    padding: 24px 20px;
  }

  .cmd {
    display: none;
  }
}
</style>
