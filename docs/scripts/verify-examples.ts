// Checks every Stav example of the documentation with the `stav` binary.
//
// Markers in the Markdown sources drive the checks:
//
// - `<!-- file: NAME -->` writes the next code block to NAME in a scratch
//   directory that belongs to the page.
// - `<!-- run: ARGS -->` runs `stav ARGS` in that directory. When a code
//   block follows directly, it must equal everything the command printed.
//   Otherwise the command must succeed.
// - `<!-- unchecked -->` skips the next code block.
//
// Every other `stav` code block is a fragment. It must parse and already be
// in the layout that `stav fmt` gives it.
//
// The binary is `stav` on the `PATH`, or the one the `STAV` variable names.
// Pages given as arguments, relative to `docs/`, limit the check to them.

import { mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join, relative } from 'node:path'

const docs = join(import.meta.dir, '..')
const stav = process.env.STAV ?? 'stav'

type Event =
  | { kind: 'file'; name: string; line: number }
  | { kind: 'run'; args: string; line: number }
  | { kind: 'unchecked'; line: number }
  | { kind: 'fence'; lang: string; body: string; line: number; direct: boolean }

function markdownFiles(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const path = join(dir, entry.name)
    if (entry.name.startsWith('.') || entry.name === 'node_modules') return []
    if (entry.isDirectory()) return markdownFiles(path)
    return entry.name.endsWith('.md') ? [path] : []
  })
}

function events(text: string): Event[] {
  const lines = text.split('\n')
  const out: Event[] = []
  let direct = true
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i]
    const marker = line.match(/^<!--\s*(file|run|unchecked)\s*:?\s*(.*?)\s*-->$/)
    if (marker) {
      const [, kind, arg] = marker
      if (kind === 'file') out.push({ kind, name: arg, line: i + 1 })
      else if (kind === 'run') out.push({ kind, args: arg, line: i + 1 })
      else out.push({ kind: 'unchecked', line: i + 1 })
      direct = true
      continue
    }
    const open = line.match(/^(\s*)(`{3,})(\S*)/)
    if (open) {
      const [, indent, ticks, info] = open
      const body: string[] = []
      let j = i + 1
      while (j < lines.length && !lines[j].startsWith(indent + ticks)) {
        body.push(lines[j].slice(indent.length))
        j++
      }
      out.push({ kind: 'fence', lang: info.replace(/[{:\[].*$/, ''), body: body.join('\n') + '\n', line: i + 1, direct })
      direct = true
      i = j
    } else if (line.trim() !== '') {
      direct = false
    }
  }
  return out
}

function run(args: string, cwd: string, input?: string) {
  const result = Bun.spawnSync(['sh', '-c', `"$STAV_BIN" ${args}`], {
    cwd,
    env: { ...process.env, STAV_BIN: stav, NO_COLOR: '1' },
    stdin: input === undefined ? 'ignore' : new TextEncoder().encode(input),
  })
  const stdout = result.stdout.toString()
  const stderr = result.stderr.toString()
  return { ok: result.exitCode === 0, output: stdout + stderr }
}

let failures = 0
let checked = 0

function fail(page: string, line: number, message: string) {
  failures++
  console.error(`${page}:${line}: ${message}`)
}

const only = process.argv.slice(2).map((arg) => join(docs, arg))
const pages = only.length > 0 ? only : markdownFiles(docs).sort()

for (const path of pages) {
  const page = relative(docs, path)
  const list = events(readFileSync(path, 'utf8'))
  const scratch = mkdtempSync(join(tmpdir(), 'stav-docs-'))
  for (let i = 0; i < list.length; i++) {
    const event = list[i]
    const next = list[i + 1]
    if (event.kind === 'file') {
      if (next?.kind !== 'fence' || !next.direct) {
        fail(page, event.line, 'a file marker needs a code block after it')
        continue
      }
      const target = join(scratch, event.name)
      mkdirSync(dirname(target), { recursive: true })
      writeFileSync(target, next.body)
      i++
    } else if (event.kind === 'run') {
      checked++
      const { ok, output } = run(event.args, scratch)
      if (next?.kind === 'fence' && next.direct) {
        if (output.trimEnd() !== next.body.trimEnd()) {
          fail(page, event.line, `\`stav ${event.args}\` printed:\n${output}`)
        }
        i++
      } else if (!ok) {
        fail(page, event.line, `\`stav ${event.args}\` failed:\n${output}`)
      }
    } else if (event.kind === 'unchecked') {
      i++
    } else if (event.lang === 'stav') {
      checked++
      const { ok, output } = run('fmt -', scratch, event.body)
      if (!ok) fail(page, event.line, `the fragment does not parse:\n${output}`)
      else if (output !== event.body) fail(page, event.line, `\`stav fmt\` lays the fragment out as:\n${output}`)
    }
  }
  rmSync(scratch, { recursive: true, force: true })
}

console.log(`${checked} examples checked, ${failures} failed`)
process.exit(failures === 0 ? 0 : 1)
