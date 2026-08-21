'use strict';

const { createHash } = require('node:crypto');
const {
  chmod,
  copyFile,
  lstat,
  mkdir,
  readdir,
  readFile,
  rm,
  writeFile,
} = require('node:fs/promises');
const path = require('node:path');

const { createVSIX } = require('@vscode/vsce');
const { platformTarget, serverExecutableName } = require('../out/server-command.js');
const manifest = require('../package.json');

const maximumServerBytes = 128 * 1024 * 1024;
const maximumLicenseBytes = 256 * 1024;
const maximumNoticesBytes = 2 * 1024 * 1024;
const extensionRoot = path.resolve(__dirname, '..');
const repositoryRoot = path.resolve(extensionRoot, '..', '..');
const serverRoot = path.join(extensionRoot, 'server');

async function main() {
  const target = platformTarget(process.platform, process.arch);
  if (!target) {
    throw new Error(`unsupported VSIX target: ${process.platform}-${process.arch}`);
  }
  if (typeof manifest.version !== 'string' || !/^\d+\.\d+\.\d+$/u.test(manifest.version)) {
    throw new Error('extension version must be an exact semantic version');
  }
  const outputRoot = path.join(extensionRoot, 'dist');
  const output = path.join(
    outputRoot,
    `yanshu-vscode-${manifest.version}-${target}.vsix`,
  );
  if (path.dirname(output) !== outputRoot) {
    throw new Error('refusing to write VSIX outside the extension dist directory');
  }
  await rm(output, { force: true });

  const executableName = serverExecutableName(process.platform);
  const override = process.env.YANSHU_LSP_BINARY?.trim();
  if (override && !path.isAbsolute(override)) {
    throw new Error('YANSHU_LSP_BINARY must be an absolute path');
  }
  const source = override || path.join(repositoryRoot, 'target', 'release', executableName);
  let sourceMetadata;
  try {
    sourceMetadata = await lstat(source);
  } catch {
    throw new Error('Yanshu LSP package input does not exist');
  }
  if (!sourceMetadata.isFile() || sourceMetadata.isSymbolicLink()) {
    throw new Error('Yanshu LSP package input must be a regular non-symlink file');
  }
  if (sourceMetadata.size === 0 || sourceMetadata.size > maximumServerBytes) {
    throw new Error('Yanshu LSP package input is empty or exceeds 128 MiB');
  }

  const stageRoot = path.join(serverRoot, target);
  if (path.dirname(stageRoot) !== serverRoot) {
    throw new Error('refusing to stage outside the extension server directory');
  }
  const destination = path.join(stageRoot, executableName);
  const stagedLicense = path.join(extensionRoot, 'LICENSE.txt');
  const stagedNotices = path.join(extensionRoot, 'THIRD_PARTY_NOTICES.txt');

  await rm(stageRoot, { recursive: true, force: true });
  await mkdir(stageRoot, { recursive: true });
  let packaged = false;
  try {
    await copyFile(source, destination);
    if (process.platform !== 'win32') {
      await chmod(destination, 0o755);
    }
    const binary = await readFile(destination);
    const digest = createHash('sha256').update(binary).digest('hex');
    await writeFile(
      path.join(stageRoot, 'manifest.json'),
      `${JSON.stringify({ schemaVersion: 1, sha256: digest, bytes: binary.length })}\n`,
      'utf8',
    );
    await writeFile(stagedNotices, await thirdPartyNotices(), 'utf8');
    const mitLicense = await readFile(path.join(repositoryRoot, 'LICENSE-MIT'), 'utf8');
    const apacheLicense = await readFile(path.join(repositoryRoot, 'LICENSE-APACHE'), 'utf8');
    await writeFile(
      stagedLicense,
      `Yanshu is available under either the MIT License or Apache License 2.0.\n\n--- MIT License ---\n\n${mitLicense}\n\n--- Apache License 2.0 ---\n\n${apacheLicense}`,
      'utf8',
    );
    await mkdir(outputRoot, { recursive: true });
    await createVSIX({
      cwd: extensionRoot,
      packagePath: output,
      target,
      ignoreOtherTargetFolders: true,
      dependencies: false,
      useYarn: false,
      gitTagVersion: false,
      updatePackageJson: false,
    });
    packaged = true;
    process.stdout.write(`Packaged ${output}\n`);
  } finally {
    await rm(stageRoot, { recursive: true, force: true });
    await rm(stagedLicense, { force: true });
    await rm(stagedNotices, { force: true });
    if (!packaged) {
      await rm(output, { force: true });
    }
  }
}

async function thirdPartyNotices() {
  const lock = JSON.parse(
    await readFile(path.join(extensionRoot, 'package-lock.json'), 'utf8'),
  );
  const packages = Object.entries(lock.packages)
    .filter(([relativePath, metadata]) => relativePath && metadata.dev !== true)
    .sort(([left], [right]) => (left < right ? -1 : left > right ? 1 : 0));
  if (packages.length === 0 || packages.length > 1024) {
    throw new Error('production dependency count is outside bounds');
  }
  let notices = 'Third-party software bundled in the Yanshu VS Code extension:\n';

  for (const [relativePath] of packages) {
    if (!relativePath.startsWith('node_modules/') || relativePath.includes('..')) {
      throw new Error('production dependency path is outside node_modules');
    }
    const packageRoot = path.resolve(extensionRoot, relativePath);
    if (!packageRoot.startsWith(`${path.join(extensionRoot, 'node_modules')}${path.sep}`)) {
      throw new Error('production dependency escaped node_modules');
    }
    const packageManifest = JSON.parse(
      await readFile(path.join(packageRoot, 'package.json'), 'utf8'),
    );
    if (
      typeof packageManifest.name !== 'string'
      || typeof packageManifest.version !== 'string'
      || typeof packageManifest.license !== 'string'
    ) {
      throw new Error('production dependency has incomplete license metadata');
    }
    const licenseFiles = (await readdir(packageRoot, { withFileTypes: true }))
      .filter((entry) => entry.isFile() && /^licen[cs]e(?:\.|$)/iu.test(entry.name))
      .map((entry) => entry.name)
      .sort();
    const [licenseFile] = licenseFiles;
    if (!licenseFile) {
      throw new Error(`production dependency has no license text: ${packageManifest.name}`);
    }
    const licensePath = path.join(packageRoot, licenseFile);
    const licenseMetadata = await lstat(licensePath);
    if (
      !licenseMetadata.isFile()
      || licenseMetadata.isSymbolicLink()
      || licenseMetadata.size === 0
      || licenseMetadata.size > maximumLicenseBytes
    ) {
      throw new Error(`production dependency license is outside bounds: ${packageManifest.name}`);
    }
    const license = await readFile(licensePath, 'utf8');
    notices += `\n\n=== ${packageManifest.name} ${packageManifest.version} (${packageManifest.license}) ===\n\n${license.trim()}\n`;
    if (Buffer.byteLength(notices, 'utf8') > maximumNoticesBytes) {
      throw new Error('third-party notices exceed 2 MiB');
    }
  }
  return notices;
}

main().catch((error) => {
  const message = error instanceof Error ? error.message : 'unknown packaging failure';
  process.stderr.write(`VSIX_PACKAGE_FAILED: ${message}\n`);
  process.exitCode = 1;
});
