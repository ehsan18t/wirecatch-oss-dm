// Fails the build if any sample data from the mock backend, or the dev-only component gallery, ended up in the production bundle.
// Run after `vite build` (the `build` script does this). Mock builds (`--mode mock`) skip it.
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join } from 'node:path';

const MARKERS = [
  'ubuntu-24.04.3-desktop-amd64.iso',
  'Pretend the browser sent a download',
  'cdn.example-conf.com',
  'Sample data only',
  // The sample app's floating download windows, a chunk only the mock backend loads; the main page names it where it imports it.
  'MockWindows',
  // The component gallery's page (gallery/index.html), served by the dev server only. Its modules are kept out by the build itself (scripts/dev-only-modules.mjs), which reads the module graph.
  'Wirecatch component gallery',
];

function* files(dir) {
  for (const name of readdirSync(dir)) {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) yield* files(path);
    else if (/\.(js|html|css)$/.test(name)) yield path;
  }
}

const found = [];
let scanned = 0;
for (const file of files('build')) {
  scanned += 1;
  const text = readFileSync(file, 'utf8');
  for (const marker of MARKERS) if (text.includes(marker)) found.push(`${file}: ${marker}`);
}

if (found.length > 0) {
  console.error('The production build contains mock backend or gallery code:\n' + found.join('\n'));
  process.exit(1);
}
console.log(`Checked ${scanned} files in build: no mock backend or gallery code.`);
