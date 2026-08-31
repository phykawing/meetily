/**
 * Shared loader for the plain-node (`*.test.mjs`) lib tests: compiles a TypeScript source
 * file in-process and runs it in a fresh VM context, with just enough module resolution to
 * follow imports between `src/lib` modules - `@/…` path aliases and relative `./…` sibling
 * imports both resolve to the compiled-on-demand source, so a module under test can be
 * split into helpers without the test needing to know.
 *
 * Anything that isn't a project source import (node builtins, npm packages) falls through
 * to the real `require`.
 */

import fs from 'node:fs';
import path from 'node:path';
import vm from 'node:vm';
import ts from 'typescript';
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';

const SRC_DIR = path.join(path.dirname(fileURLToPath(import.meta.url)), '..', '..', 'src');
const SOURCE_EXTS = ['', '.ts', '.tsx', '.js', '.mjs', '/index.ts', '/index.tsx'];

function resolveSource(basePath) {
  for (const ext of SOURCE_EXTS) {
    const candidate = basePath + ext;
    if (fs.existsSync(candidate) && fs.statSync(candidate).isFile()) {
      return candidate;
    }
  }
  return null;
}

/**
 * Compiles and evaluates the TypeScript module at `entryPath` (an absolute path string, a
 * `file://` URL string, or a `URL`) and returns its `module.exports`.
 */
export function loadTsModule(entryPath) {
  const absEntry =
    entryPath instanceof URL || String(entryPath).startsWith('file:')
      ? fileURLToPath(entryPath)
      : path.resolve(entryPath);
  const cache = new Map();

  const compileAndRun = (absPath) => {
    if (cache.has(absPath)) {
      return cache.get(absPath);
    }
    const compiled = ts.transpileModule(fs.readFileSync(absPath, 'utf8'), {
      compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2020 },
    }).outputText;

    const mod = { exports: {} };
    cache.set(absPath, mod.exports); // seed before running, so import cycles terminate
    vm.runInNewContext(compiled, { exports: mod.exports, module: mod, require: makeRequire(absPath) });
    cache.set(absPath, mod.exports);
    return mod.exports;
  };

  const makeRequire = (fromPath) => (specifier) => {
    let base = null;
    if (specifier.startsWith('@/')) {
      base = path.join(SRC_DIR, specifier.slice(2));
    } else if (specifier.startsWith('./') || specifier.startsWith('../')) {
      base = path.resolve(path.dirname(fromPath), specifier);
    }

    if (base) {
      const resolved = resolveSource(base);
      if (resolved) {
        return compileAndRun(resolved);
      }
    }

    return createRequire(fromPath)(specifier);
  };

  return compileAndRun(absEntry);
}
