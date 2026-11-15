import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';
import test from 'node:test';
import ts from 'typescript';

const app = readFileSync(new URL('../src/App.vue', import.meta.url), 'utf8');
const script = app.split('<script setup lang="ts">')[1].split('</script>')[0];
const parsed = ts.createSourceFile('App.ts', script, ts.ScriptTarget.Latest, true);
const names = ['playHomePinnedItem', 'getSmartPlaylistPage', 'playFromPlaylist', 'playTrackFrom', 'sourceList', 'refillQueue', 'queuedSourceHasMore', 'loadNextQueuedSourcePage', 'playNext'];
const functions = parsed.statements.filter(node => ts.isFunctionDeclaration(node) && names.includes(node.name?.text));
assert.equal(functions.length, names.length);
const code = ts.transpileModule(functions.map(node => node.getText(parsed)).join('\n'), { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS } }).outputText;

for (const kind of ['regular', 'smart']) for (const includeDuplicates of [false, true]) {
  test(`Home ${kind} playlist preserves lazy order from preview index and stops at end (duplicates ${includeDuplicates})`, async () => {
    const all = Array.from({ length: 13 }, (_, id) => ({ id, duration_secs: 10, is_duplicate: false }));
    const calls = [], played = [];
    const state = { LIBRARY_TRACK_PAGE_SIZE: 5, console, playbackRestorePromise: null };
    for (const key of ['queuePlaylistTracks', 'queue', 'queueLibraryTracks']) state[key] = { value: [] };
    for (const key of ['queuedPagedSource', 'playlistView', 'smartView', 'contentRef', 'nowPlaying']) state[key] = { value: null };
    for (const key of ['queuedPageLoading', 'libraryLoadingMore', 'smartViewLoadingMore', 'playbackRestoring']) state[key] = { value: false };
    Object.assign(state, { showDuplicateTracks: { value: includeDuplicates }, repeatMode: { value: 0 }, queueSource: { value: 'playlist' }, queueSourceIndex: { value: 0 }, isPlaying: { value: true }, duration: { value: 0 }, currentTime: { value: 0 } });
    for (const name of ['bumpPlaybackTransitionSequence', 'stopTicker', 'syncAndroid', 'restoreContentScroll', 'recordPlayIfTracked', 'startTicker', 'refreshPlaybackState', 'stopCurrentOutput']) state[name] = () => {};
    state.shouldSuppressTrackRowClick = () => false;
    state.playTrackOnCurrentOutput = async track => played.push(track.id);
    state.invoke = async (command, args) => {
      assert.equal(command, kind === 'regular' ? 'get_playlist_tracks_page' : 'get_smart_playlist_tracks_page');
      calls.push({ command, ...args });
      assert.equal(args.includeDuplicates, includeDuplicates);
      assert.equal(args.limit, 5);
      return { tracks: all.slice(args.offset, args.offset + args.limit), has_more: args.offset + args.limit < all.length, total: all.length };
    };
    vm.createContext(state); vm.runInContext(code, state);
    const playlist = { id: 42, name: 'Pinned', rules: [{ field: 'genre', value: 'Rock' }], match: 'all' };
    await state.playHomePinnedItem({ kind, playlist, trackCount: all.length, tracks: all.slice(0, 3) }, 2);
    assert.deepEqual(played, [2]);
    assert.deepEqual(calls.map(c => c.offset), [0]);
    assert.equal(state.queuedPagedSource.value.kind, `${kind}-playlist`);
    assert.equal(state.queuedPagedSource.value.includeDuplicates, includeDuplicates);
    // A later preference change must not alter the queued source's paging policy.
    state.showDuplicateTracks.value = !includeDuplicates;
    for (let i = 0; i < 11; i++) await state.playNext();
    assert.deepEqual(played, Array.from({ length: 11 }, (_, i) => i + 2));
    assert.deepEqual(calls.map(c => c.offset), [0, 5, 10]);
    assert.equal(state.isPlaying.value, false);
    assert.equal(state.queuedPagedSource.value.hasMore, false);
    if (kind === 'regular') assert.ok(calls.every(c => c.playlistId === 42));
    else assert.ok(calls.every(c => c.rulesJson === JSON.stringify(playlist.rules) && c.matchMode === 'all'));
  });
}

test('Home preview rows use the paged playlist entry point with their selected index', () => {
  assert.match(app, /@play="playHomePinnedItem\(item, idx\)"/);
});
