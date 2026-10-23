import { createHash } from 'node:crypto';
import { mkdir, readFile, writeFile, cp, rm } from 'node:fs/promises';
import { execFileSync } from 'node:child_process';
import path from 'node:path';

export const release = {
  version: '4.10.1', commit: '3169d2d3e8e426ec0c0eca5cac15c05d83dcdc6f',
  archiveSha256: '5cd1cbc6c5352aa913a3b7bf595792f2b189b53adfbfd41115d1e3e4aaa11f20',
  asset: 'donsetch-win32-x64.tar.gz',
  sourceSha256: '48d073b9eed2b329616cd0b2b76b209f2729ca8e442c7af3569e77f3030fafc5',
  files: {
    'donsetch.exe': '2cb15d1a98dd7a3bce9f5438e60837e63c4c6dc96a35e6772502e1c2d74b86d0',
    'onnxruntime.dll': '114947d633e6844ce3c4b51ef6678f776628571d08a5763859c61642c8dcca9c',
    'pdfium.dll': 'f1bdfac9323cb4ff5d71cbddea7e38c53c663201135a8a0369794b5d76ebb8a6',
  },
  models: [
    { name: 'model_quantized.onnx', file: 'onnx/model_quantized.onnx', sha256: 'e9d8ebf845c413e981c175bfe49a3bfa9b3dcce2a3ba54875ee5df5a58639fbe' },
    { name: 'tokenizer.json', file: 'tokenizer.json', sha256: 'd241a60d5e8f04cc1b2b3e9ef7a4921b27bf526d9f6050ab90f9267a1f9e5c66' },
  ],
};
export const sha256 = data => createHash('sha256').update(data).digest('hex');
async function download(url) {
  const r = await fetch(url, { signal: AbortSignal.timeout(120_000) });
  if (!r.ok) throw new Error(`Download failed (${r.status}): ${url}`);
  return Buffer.from(await r.arrayBuffer());
}
async function verified(url, file, expected) {
  let bytes;
  try { bytes = await readFile(file); } catch { /* first bundle */ }
  if (!bytes || sha256(bytes) !== expected) bytes = await download(url);
  if (sha256(bytes) !== expected) throw new Error(`Checksum mismatch: ${file}`);
  await mkdir(path.dirname(file), { recursive: true }); await writeFile(file, bytes);
  return bytes;
}

const tarDir = target => path.relative(process.cwd(), target).split(path.sep).join('/');
export async function bundleDonsetch(destination) {
  const archive = `vendor/downloads/${release.asset}`;
  await verified(`https://github.com/dondai44423/donsetch/releases/download/v${release.version}/${release.asset}`, archive, release.archiveSha256);
  const binaryDir = path.join(destination, 'node_modules/donsetch/binaries');
  await mkdir(binaryDir, { recursive: true });
  const entries = execFileSync('tar.exe', ['tzf', archive], { encoding: 'utf8', windowsHide: true }).trim().split(/\r?\n/u);
  if (entries.some(name => !/^(\.\/)?(donsetch\.exe|onnxruntime\.dll|pdfium\.dll)$/u.test(name))) throw new Error('Unexpected path in DonSeTch archive');


  execFileSync('tar.exe', ['xzf', archive, '-C', tarDir(binaryDir)], { windowsHide: true });
  const files = {};
  for (const name of ['donsetch.exe', 'onnxruntime.dll', 'pdfium.dll']) {
    files[name] = sha256(await readFile(path.join(binaryDir, name)));
    if (files[name] !== release.files[name]) throw new Error(`Extracted DonSeTch checksum mismatch: ${name}`);
  }
  const modelDir = path.join(destination, 'research-models/rerank');
  await rm(path.join(destination, 'research-models'), { recursive: true, force: true });
  for (const model of release.models) await verified(`https://huggingface.co/Xenova/ms-marco-MiniLM-L-6-v2/resolve/main/${model.file}`, path.join(modelDir, model.name), model.sha256);

  const source = await verified(`https://codeload.github.com/dondai44423/donsetch/tar.gz/${release.commit}`, 'vendor/downloads/donsetch-source.tar.gz', release.sourceSha256);
  const repository = process.env.GITHUB_REPOSITORY || 'Vortextbloons/talo';
  const appVersion = JSON.parse(await readFile('package.json', 'utf8')).version;
  const sourceAsset = `donsetch-v${release.version}-source.tar.gz`;
  await rm(path.join(destination, sourceAsset), { force: true });
  await writeFile(path.join(destination, 'DONSETCH-SOURCE.txt'), [
    `DonSeTch ${release.version} - Corresponding Source`,
    '',
    'Talo ships the unmodified DonSeTch Windows binaries. The complete Corresponding Source is published next to the installer:',
    '',
    `  https://github.com/${repository}/releases/download/v${appVersion}/${sourceAsset}`,
    '',
    `Source commit: ${release.commit}`,
    `Archive: ${sourceAsset}`,
    `SHA-256: ${release.sourceSha256}`,
    '',
    `The archive is generated from https://codeload.github.com/dondai44423/donsetch/tar.gz/${release.commit} and includes the upstream Cargo.lock, build.rs and release workflow.`,
    '',
    'If the download is unavailable, contact the distributor named in the release notes to obtain the source for no more than the cost of distribution.',
    '',
  ].join('\n'));
  await cp('node_modules/donsetch/LICENSE', path.join(destination, 'DONSETCH-LICENSE.txt'));
  const notices = path.join(destination, 'research-notices');
  await mkdir(notices, { recursive: true });
  for (const [name, url, hash] of [
    ['ONNX-RUNTIME-LICENSE.txt','https://raw.githubusercontent.com/microsoft/onnxruntime/v1.24.2/LICENSE','2f07c72751aed99790b8a4869cf2311df85a860b22ded05fa22803587a48922c'],
    ['ONNX-RUNTIME-NOTICES.txt','https://raw.githubusercontent.com/microsoft/onnxruntime/v1.24.2/ThirdPartyNotices.txt','0e07b95f3a8d6230037707c5c4a2b554d12c4cb67369669ac255635528ffcee2'],
    ['RERANK-MODEL-CARD.md','https://huggingface.co/cross-encoder/ms-marco-MiniLM-L-6-v2/resolve/main/README.md','7c0ec39941d0d1d766ccde671f592971d82fe45b7eea550a80d1ea864ebc3baa'],
  ]) {
    const bytes = await verified(url, path.join('vendor/downloads/notices', name), hash);
    await writeFile(path.join(notices, name), bytes);
  }
  await cp('node_modules/playwright/LICENSE', path.join(notices, 'APACHE-2.0.txt'));
  const pdfiumArchive = 'vendor/downloads/pdfium-licenses.tgz';
  await verified('https://github.com/bblanchon/pdfium-binaries/releases/download/chromium%2F7802/pdfium-win-x64.tgz', pdfiumArchive, '487156c28d81bd162107ca0ba85849cbfbb0127be4210a7cfec6def66802082d');
  execFileSync('tar.exe', ['xzf', pdfiumArchive, '-C', tarDir(notices), 'LICENSE', 'licenses'], { windowsHide: true });
  await cp('docs/DONSETCH-REDISTRIBUTION.md', path.join(destination, 'DONSETCH-REDISTRIBUTION.md'));
  await writeFile(path.join(destination, 'donsetch-manifest.json'), JSON.stringify({ ...release, files, sourceSha256: sha256(source) }, null, 2));
  console.log(`Verified DonSeTch ${release.version}, Windows DLLs, reranking model and the corresponding-source offer.`);
}
if (process.argv[1] && path.resolve(process.argv[1]) === path.resolve(import.meta.filename)) await bundleDonsetch(path.resolve('vendor/bundle'));
