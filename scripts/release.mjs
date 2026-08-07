import { execFileSync } from 'node:child_process';
import { readFileSync, writeFileSync } from 'node:fs';

const input = process.argv[2] ?? '';
const version = /^\d+\.\d+$/.test(input) ? `${input}.0` : input;
if (!/^\d+\.\d+\.\d+$/.test(version)) {
  console.error('Usage: npm run release -- 1.0.1');
  process.exit(1);
}
const run = (command, args) => execFileSync(command, args, { stdio: 'inherit' });
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
process.env.APPIMAGE_EXTRACT_AND_RUN = '1';
const buildArgs = process.platform === 'linux'
  ? ['run', 'tauri', '--', 'build', '--bundles', 'deb,rpm,pacman']
  : ['run', 'tauri', '--', 'build'];
run('npm', buildArgs);
const remotes = execFileSync('git', ['remote'], { encoding: 'utf8' }).trim().split(/\s+/).filter(Boolean);
const remote = remotes.includes('origin') ? 'origin' : remotes[0];
if (!remote) throw new Error('No Git remote configured');
run('git', ['add', 'package.json', 'package-lock.json', 'src-tauri/tauri.conf.json', 'src-tauri/Cargo.toml', 'src-tauri/Cargo.lock']);
run('git', ['commit', '-m', `Release v${version}`]);
run('git', ['tag', '-a', `v${version}`, '-m', `Release v${version}`]);
run('git', ['push', remote, 'HEAD']);
run('git', ['push', remote, `v${version}`]);
