import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import ts from 'typescript';
import { computed, ref } from 'vue';

const compile = path => ts.transpileModule(readFileSync(new URL(path, import.meta.url), 'utf8'), {
  compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS },
}).outputText;
const hostCode = compile('../src/plugins/host.ts');
const queue = {};
new Function('exports', compile('../src/plugins/attachmentQueue.ts'))(queue);
const content = JSON.stringify({ manifest: { id: 'example.update', version: '2.0.0' } });
const source = { url: 'https://example.com/plugin.json' };

async function harness({ enabled = true, installed = true, installError, disableError, enableError } = {}) {
  let plugin = installed ? { manifest: { id: 'example.update', version: '1.0.0', permissions: [], contributions: [] }, enabled, supported: true, generation: 1 } : undefined;
  const calls = [];
  const invoke = async (command, args) => {
    calls.push([command, args]);
    if (command === 'plugins_list') return plugin ? [structuredClone(plugin)] : [];
    if (command === 'plugins_set_enabled') {
      if (!args.enabled && disableError) throw disableError;
      if (args.enabled && enableError) throw enableError;
      plugin.enabled = args.enabled;
      plugin.generation++;
      return structuredClone(plugin);
    }
    if (command === 'plugins_install') {
      assert.equal(plugin?.enabled ?? false, false);
      if (installError) throw installError;
      plugin = { ...(plugin ?? { enabled: false, supported: true, generation: 1 }), manifest: { id: 'example.update', version: '2.0.0', permissions: [], contributions: [] } };
      return structuredClone(plugin);
    }
    if (command === 'plugin_load') return { events: [] };
    if (command === 'plugin_native_call') return null;
    throw new Error(`Unexpected command ${command}`);
  };
  const exports = {};
  new Function('exports', 'require', hostCode)(exports, name => {
    if (name === 'vue') return { computed, ref, watch() {}, onMounted() {}, onUnmounted() {}, provide() {}, inject() {} };
    if (name === './attachmentQueue') return queue;
    if (name.endsWith('/core')) return { invoke };
    return { listen: async () => () => {} };
  });
  const host = exports.createPluginHost({ snapshot: () => ({}), transport: async () => {}, playSource: async () => {}, adopt: async () => {}, navigate() {} });
  await host.refresh();
  calls.length = 0;
  return { host, calls };
}

for (const fails of [false, true]) {
  test(`enabled replacement restores activation after installation ${fails ? 'failure' : 'success'}`, async () => {
    const failure = new Error('Approval fingerprint mismatch');
    const { host, calls } = await harness({ installError: fails ? failure : undefined });
    const operation = host.install(content, 'approved-fingerprint', source);
    if (fails) await assert.rejects(operation, error => error === failure);
    else await operation;
    assert.deepEqual(calls.map(([command]) => command), ['plugins_set_enabled', 'plugins_install', 'plugins_set_enabled', 'plugin_load', 'plugin_native_call', 'plugins_list']);
    assert.deepEqual(calls[0][1], { pluginId: 'example.update', enabled: false });
    assert.deepEqual(calls[1][1], { content, fingerprint: 'approved-fingerprint', source });
    assert.deepEqual(calls[2][1], { pluginId: 'example.update', enabled: true });
    assert.equal(host.plugins.value[0].enabled, true);
    assert.equal(host.plugins.value[0].manifest.version, fails ? '1.0.0' : '2.0.0');
    assert.deepEqual(host.running.value, ['example.update']);
  });
}

for (const installed of [true, false]) {
  test(`${installed ? 'disabled replacement' : 'new installation'} remains disabled`, async () => {
    const { host, calls } = await harness({ enabled: false, installed });
    await host.install(content, 'fingerprint', source);
    assert.deepEqual(calls.map(([command]) => command), ['plugins_install', 'plugins_list']);
    assert.equal(host.plugins.value[0].enabled, false);
    assert.deepEqual(host.running.value, []);
  });
}

test('disable failure aborts replacement without uninstalling', async () => {
  const failure = new Error('Cannot stop runtime');
  const { host, calls } = await harness({ disableError: failure });
  await assert.rejects(host.install(content, 'fingerprint'), error => error === failure);
  assert.deepEqual(calls.map(([command]) => command), ['plugins_set_enabled', 'plugins_list']);
  assert.equal(host.plugins.value[0].manifest.version, '1.0.0');
});

test('failed restoration still refreshes installed state and reports the failure', async () => {
  const failure = new Error('Cannot enable replacement');
  const { host, calls } = await harness({ enableError: failure });
  await assert.rejects(host.install(content, 'fingerprint'), error => error === failure);
  assert.equal(calls.at(-1)[0], 'plugins_list');
  assert.equal(host.plugins.value[0].manifest.version, '2.0.0');
  assert.equal(host.plugins.value[0].enabled, false);
  assert.equal(calls.some(([command]) => command === 'plugins_uninstall'), false);
});
