import type { CatalogEntry, PluginManifest } from './types';

export function filterCatalog(entries: CatalogEntry[], query: string): CatalogEntry[] {
  const terms = query.toLocaleLowerCase().trim().split(/\s+/).filter(Boolean);
  return entries.filter(entry => {
    const author = typeof entry.author === 'string' ? entry.author : entry.author?.name || '';
    const text = [entry.id, entry.name, entry.description, author, ...entry.platforms, ...entry.tags].join(' ').toLocaleLowerCase();
    return terms.every(term => text.includes(term));
  }).sort((a, b) => b.addedAt.localeCompare(a.addedAt) || a.name.localeCompare(b.name));
}

/** Catalog metadata is only for display; install preview always uses the downloaded manifest. */
export function catalogManifest(entry: CatalogEntry): PluginManifest {
  return { id: entry.id, name: entry.name, version: entry.version || '', apiVersion: 2,
    description: entry.description, author: entry.author, image: entry.image,
    repository: entry.repository, platforms: entry.platforms, permissions: [], contributions: [] };
}

/** Only strictly newer valid SemVer versions are updates; build metadata has no precedence. */
export function isPluginUpdate(installedVersion: unknown, availableVersion: unknown): boolean {
  const parse = (version: unknown): string[] | undefined => {
    if (typeof version !== 'string') return;
    const match = /^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(?:-([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?$/.exec(version);
    if (!match || match[0] !== version) return;
    const prerelease = match[4]?.split('.') || [];
    if (prerelease.some(part => /^[0-9]+$/.test(part) && part.length > 1 && part[0] === '0')) return;
    return [match[1], match[2], match[3], ...prerelease];
  };
  const installed = parse(installedVersion), available = parse(availableVersion);
  if (!installed || !available) return false;
  const compareNumeric = (a: string, b: string): number => a.length - b.length || (a > b ? 1 : a < b ? -1 : 0);
  for (let i = 0; i < 3; i++) {
    const order = compareNumeric(available[i], installed[i]);
    if (order) return order > 0;
  }
  if (installed.length === 3 || available.length === 3) return installed.length > 3 && available.length === 3;
  for (let i = 3; i < Math.min(installed.length, available.length); i++) {
    const a = available[i], b = installed[i];
    if (a === b) continue;
    const aNumeric = /^[0-9]+$/.test(a), bNumeric = /^[0-9]+$/.test(b);
    if (aNumeric && bNumeric) return compareNumeric(a, b) > 0;
    if (aNumeric !== bNumeric) return !aNumeric;
    return a > b;
  }
  return available.length > installed.length;
}
