import { execFileSync } from 'node:child_process';
import { readFileSync, writeFileSync } from 'node:fs';

const input = process.argv[2] ?? '';
const version = /^\d+\.\d+$/.test(input) ? `${input}.0` : input;
if (!/^\d+\.\d+\.\d+$/.test(version)) {
  console.error('Usage: npm run release -- 1.0.1');
  process.exit(1);
}
const run = (command, args) => execFileSync(command, args, { stdio: 'inherit' });
const runOut = (command, args) => execFileSync(command, args, { encoding: 'utf8' }).trim();

// Cek branch aktif saat ini
const currentBranch = runOut('git', ['branch', '--show-current']);

// Jika berada di branch selain 'main', pindah ke 'main' dan merge branch aktif tersebut
if (currentBranch && currentBranch !== 'main') {
  console.log(`Mengalihkan dari branch '${currentBranch}' ke 'main' dan melakukan merge...`);
  run('git', ['checkout', 'main']);
  run('git', ['merge', currentBranch]);
}

// Lakukan git pull --rebase untuk memastikan lokal sinkron dengan remote sebelum menambahkan release commit
const remotes = execFileSync('git', ['remote'], { encoding: 'utf8' }).trim().split(/\s+/).filter(Boolean);
const remote = remotes.includes('origin') ? 'origin' : remotes[0];
if (!remote) throw new Error('No Git remote configured');

try {
  run('git', ['pull', '--rebase', remote, 'main']);
} catch (e) {
  console.warn('Gagal git pull --rebase, melanjutkan...');
}

const json = (file) => JSON.parse(readFileSync(file, 'utf8'));
const packageJson = json('package.json');
packageJson.version = version;
writeFileSync('package.json', `${JSON.stringify(packageJson, null, 2)}\n`);
const lockPath = 'package-lock.json';
const packageLock = json(lockPath);
packageLock.version = version;
if (packageLock.packages?.['']) packageLock.packages[''].version = version;
writeFileSync(lockPath, `${JSON.stringify(packageLock, null, 2)}\n`);
const tauri = json('src-tauri/tauri.conf.json');
tauri.version = version;
writeFileSync('src-tauri/tauri.conf.json', `${JSON.stringify(tauri, null, 2)}\n`);
const cargoPath = 'src-tauri/Cargo.toml';
writeFileSync(cargoPath, readFileSync(cargoPath, 'utf8').replace(/^(version\s*=\s*")[^"]+(")/m, `$1${version}$2`));
const pkgbuildPath = 'PKGBUILD';
try {
  const content = readFileSync(pkgbuildPath, 'utf8');
  writeFileSync(pkgbuildPath, content.replace(/^(pkgver=)[^\n]+/m, `$1${version}`));
} catch (e) {}

run('git', ['add', 'package.json', 'package-lock.json', 'src-tauri/tauri.conf.json', 'src-tauri/Cargo.toml', 'src-tauri/Cargo.lock', 'PKGBUILD']);
run('git', ['commit', '-m', `Release v${version}`]);
run('git', ['tag', '-a', `v${version}`, '-m', `Release v${version}`]);
run('git', ['push', remote, 'HEAD']);
run('git', ['push', remote, `v${version}`]);
