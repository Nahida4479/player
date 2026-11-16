import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import ts from 'typescript';

const helpers = {};
new Function('exports', ts.transpileModule(readFileSync(new URL('../src/plugins/catalog.ts', import.meta.url), 'utf8'), {
  compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS },
}).outputText)(helpers);
const { isPluginUpdate } = helpers;

function assertNewer(older, newer) {
  assert.equal(isPluginUpdate(older, newer), true, `${newer} > ${older}`);
  assert.equal(isPluginUpdate(newer, older), false, `${older} < ${newer}`);
}

test('plugin versions compare numeric major, minor and patch components without precision loss', () => {
  for (const [older, newer] of [
    ['1.9.9', '2.0.0'], ['1.2.99', '1.10.0'], ['1.0.9', '1.0.10'],
    ['9007199254740992.0.0', '9007199254740993.0.0'],
    ['1.9007199254740992.0', '1.9007199254740993.0'],
    ['1.0.9007199254740992', '1.0.9007199254740993'],
  ]) assertNewer(older, newer);
});

test('plugin prereleases follow SemVer identifier precedence', () => {
  const ordered = ['1.0.0-alpha', '1.0.0-alpha.1', '1.0.0-alpha.beta', '1.0.0-beta',
    '1.0.0-beta.2', '1.0.0-beta.11', '1.0.0-rc.1', '1.0.0'];
  for (let i = 0; i < ordered.length; i++) {
    for (let j = i + 1; j < ordered.length; j++) assertNewer(ordered[i], ordered[j]);
  }
  assertNewer('1.0.0-9007199254740992', '1.0.0-9007199254740993');
  assertNewer('1.0.0-Z', '1.0.0-a');
  assertNewer('1.0.0-0', '1.0.0-0.0');
  assertNewer('1.0.0', '1.0.1-alpha');
});

test('equal versions and build-only changes are not plugin updates', () => {
  for (const version of ['0.0.0', '1.2.3', '1.2.3-alpha.0', '1.2.3+001', '1.2.3-alpha+build']) {
    assert.equal(isPluginUpdate(version, version), false);
  }
  for (const [a, b] of [['1.2.3', '1.2.3+build.2'], ['1.2.3+first', '1.2.3+second'],
    ['1.2.3-alpha+001', '1.2.3-alpha+002']]) {
    assert.equal(isPluginUpdate(a, b), false);
    assert.equal(isPluginUpdate(b, a), false);
  }
  assertNewer('1.2.3-alpha+build', '1.2.3+build');
  assertNewer('1.2.3+build', '1.2.4+001');
});

test('missing and invalid versions never qualify as plugin updates', () => {
  for (const invalid of [undefined, null, 123, {}, [], '', '1', '1.2', 'v1.2.3', '=1.2.3',
    ' 1.2.3', '1.2.3 ', '1.2.3\n', '01.2.3', '1.02.3', '1.2.03', '-1.2.3',
    '1.2.3-', '1.2.3-alpha..1', '1.2.3-01', '1.2.3-alpha.01', '1.2.3+,2',
    '1.2.3+', '1.2.3+build..1', '1.2.3-alpha_beta', '1.2.3+build/1', '1.2.3.4']) {
    assert.equal(isPluginUpdate(invalid, '2.0.0'), false, `invalid installed: ${String(invalid)}`);
    assert.equal(isPluginUpdate('0.0.0', invalid), false, `invalid available: ${String(invalid)}`);
  }
});
