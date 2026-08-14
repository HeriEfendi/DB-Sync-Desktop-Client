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

// Cari git remote (origin atau remote pertama)
const remotes = runOut('git', ['remote']).split(/\s+/).filter(Boolean);
const remote = remotes.includes('origin') ? 'origin' : remotes[0];
if (!remote) throw new Error('No Git remote configured');

// Catat branch aktif awal (misal: 'dev')
const initialBranch = runOut('git', ['branch', '--show-current']);

try {
  // 1. Pastikan branch aktif (dev) sinkron dengan remote jika ada remote branch
  console.log(`Mengambil update terbaru dari remote '${remote}'...`);
  run('git', ['fetch', remote]);

  // 2. Jika dipanggil dari branch selain main (misal: dev), pindah ke main dan merge dev ke main
  if (initialBranch && initialBranch !== 'main') {
    console.log(`Mengalihkan dari branch '${initialBranch}' ke 'main'...`);
    run('git', ['checkout', 'main']);
    
    console.log(`Menyinkronkan 'main' dengan remote...`);
    try {
      run('git', ['pull', remote, 'main', '--rebase']);
    } catch (e) {
      console.warn('Gagal pull rebase main, melanjutkan...');
    }

    console.log(`Melakukan merge branch '${initialBranch}' ke 'main'...`);
    run('git', ['merge', initialBranch, '-X', 'theirs', '--no-edit']);
  } else {
    console.log(`Menyinkronkan 'main' dengan remote...`);
    try {
      run('git', ['pull', remote, 'main', '--rebase']);
    } catch (e) {
      console.warn('Gagal pull rebase main, melanjutkan...');
    }
  }

  // 3. Update versi file-file konfigurasi
  console.log(`Memperbarui versi ke v${version}...`);
  const json = (file) => JSON.parse(readFileSync(file, 'utf8'));
  
  const packageJson = json('package.json');
  packageJson.version = version;
  writeFileSync('package.json', `${JSON.stringify(packageJson, null, 2)}\n`);

  const lockPath = 'package-lock.json';
  if (readFileSync(lockPath, 'utf8')) {
    const packageLock = json(lockPath);
    packageLock.version = version;
    if (packageLock.packages?.['']) packageLock.packages[''].version = version;
    writeFileSync(lockPath, `${JSON.stringify(packageLock, null, 2)}\n`);
  }

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

  // 4. Commit release & tag vX.Y.Z
  console.log(`Membuat commit & tag release v${version}...`);
  run('git', ['add', 'package.json', 'package-lock.json', 'src-tauri/tauri.conf.json', 'src-tauri/Cargo.toml', 'src-tauri/Cargo.lock', 'PKGBUILD']);
  run('git', ['commit', '-m', `Release v${version}`]);
  run('git', ['tag', '-a', `v${version}`, '-m', `Release v${version}`]);

  // 5. Push branch main & tag ke GitHub remote (ini akan mentrigger release.yml di GitHub Actions)
  console.log(`Mendorong branch 'main' dan tag 'v${version}' ke GitHub (${remote})...`);
  run('git', ['push', remote, 'main']);
  run('git', ['push', remote, `v${version}`]);

  console.log(`Release v${version} sukses terdorong ke GitHub! workflow release.yml akan memproses build.`);

} finally {
  // 6. Selalu kembalikan pengguna ke branch awal ('dev') setelah selesai
  if (initialBranch && initialBranch !== 'main') {
    console.log(`Mengembalikan ke branch '${initialBranch}'...`);
    run('git', ['checkout', initialBranch]);
    try {
      console.log(`Menyinkronkan '${initialBranch}' dengan 'main'...`);
      run('git', ['merge', 'main', '-X', 'theirs', '--no-edit']);
      run('git', ['push', remote, initialBranch]);
    } catch (e) {
      console.warn(`Gagal sinkronisasi kembali 'main' ke '${initialBranch}'.`);
    }
  }
}
