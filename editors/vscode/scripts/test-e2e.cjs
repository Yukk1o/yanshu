'use strict';

const {
  chmod,
  copyFile,
  lstat,
  mkdir,
  mkdtemp,
  rm,
} = require('node:fs/promises');
const os = require('node:os');
const path = require('node:path');

const { downloadAndUnzipVSCode, runTests } = require('@vscode/test-electron');
const { platformTarget, serverExecutableName } = require('../out/server-command.js');
const manifest = require('../package.json');

const maximumServerBytes = 128 * 1024 * 1024;
const maximumExtensionFileBytes = 16 * 1024 * 1024;
const excludedTestEnvironmentName = /(?:auth|credential|key|password|secret|token)|^(?:npm_config_)?(?:all|http|https|no)_proxy$/iu;
const extensionRoot = path.resolve(__dirname, '..');
const repositoryRoot = path.resolve(extensionRoot, '..', '..');
const vscodeVersion = '1.101.2';

async function main() {
  const target = platformTarget(process.platform, process.arch);
  if (!target) {
    throw new Error(`unsupported Extension Host target: ${process.platform}-${process.arch}`);
  }
  if (typeof manifest.publisher !== 'string' || typeof manifest.name !== 'string') {
    throw new Error('extension manifest publisher and name must be strings');
  }
  const extensionId = `${manifest.publisher}.${manifest.name}`;

  const temporaryRoot = await mkdtemp(path.join(os.tmpdir(), 'yanshu-vscode-e2e-'));
  const developmentRoot = path.join(temporaryRoot, 'extension');
  const profileRoot = path.join(temporaryRoot, 'profile');
  const extensionsRoot = path.join(temporaryRoot, 'extensions');
  try {
    process.stdout.write(`VSCODE_E2E_STAGE: preparing ${target}\n`);
    await stageExtension(developmentRoot, target);
    await mkdir(profileRoot, { recursive: true });
    await mkdir(extensionsRoot, { recursive: true });

    const executableOverride = process.env.YANSHU_VSCODE_EXECUTABLE?.trim();
    process.stdout.write(
      executableOverride
        ? 'VSCODE_E2E_STAGE: validating executable override\n'
        : `VSCODE_E2E_STAGE: resolving VS Code ${vscodeVersion}\n`,
    );
    const vscodeExecutablePath = executableOverride
      ? await validateExecutableOverride(executableOverride)
      : await downloadAndUnzipVSCode({
        version: vscodeVersion,
        cachePath: path.join(extensionRoot, '.vscode-test'),
        timeout: 60_000,
      });

    process.stdout.write('VSCODE_E2E_STAGE: launching isolated Extension Host\n');
    await withoutHostCredentialsAndProxy(async () => runTests({
      vscodeExecutablePath,
      extensionDevelopmentPath: developmentRoot,
      extensionTestsPath: path.join(extensionRoot, 'out', 'test', 'suite', 'index.js'),
      launchArgs: [
        path.join(extensionRoot, 'tests', 'workspace'),
        '--new-window',
        '--disable-extensions',
        '--disable-telemetry',
        `--user-data-dir=${profileRoot}`,
        `--extensions-dir=${extensionsRoot}`,
      ],
      extensionTestsEnv: {
        YANSHU_E2E_EXTENSION_ID: extensionId,
        YANSHU_E2E_SERVER_PATH: path.join(
          developmentRoot,
          'server',
          target,
          serverExecutableName(process.platform),
        ),
      },
    }));
    process.stdout.write('VSCODE_E2E_OK\n');
  } finally {
    await rm(temporaryRoot, { recursive: true, force: true, maxRetries: 3, retryDelay: 100 });
  }
}

async function stageExtension(developmentRoot, target) {
  const executableName = serverExecutableName(process.platform);
  const override = process.env.YANSHU_LSP_BINARY?.trim();
  if (override && !path.isAbsolute(override)) {
    throw new Error('YANSHU_LSP_BINARY must be absolute');
  }
  const serverSource = override || path.join(
    repositoryRoot,
    'target',
    'release',
    executableName,
  );
  const serverMetadata = await regularFileMetadata(serverSource, 'release language server');
  if (serverMetadata.size === 0 || serverMetadata.size > maximumServerBytes) {
    throw new Error('release language server is empty or exceeds 128 MiB');
  }

  const files = [
    ['package.json', 'package.json'],
    ['language-configuration.json', 'language-configuration.json'],
    [path.join('syntaxes', 'yanshu.tmLanguage.json'), path.join('syntaxes', 'yanshu.tmLanguage.json')],
    [path.join('out', 'extension.js'), path.join('out', 'extension.js')],
  ];
  for (const [sourceRelative, destinationRelative] of files) {
    const source = path.join(extensionRoot, sourceRelative);
    const metadata = await regularFileMetadata(source, `extension file ${sourceRelative}`);
    if (metadata.size === 0 || metadata.size > maximumExtensionFileBytes) {
      throw new Error(`extension file is empty or exceeds 16 MiB: ${sourceRelative}`);
    }
    const destination = path.join(developmentRoot, destinationRelative);
    await mkdir(path.dirname(destination), { recursive: true });
    await copyFile(source, destination);
  }

  const serverDestination = path.join(
    developmentRoot,
    'server',
    target,
    executableName,
  );
  await mkdir(path.dirname(serverDestination), { recursive: true });
  await copyFile(serverSource, serverDestination);
  if (process.platform !== 'win32') {
    await chmod(serverDestination, 0o755);
  }
}

async function regularFileMetadata(candidate, label) {
  let metadata;
  try {
    metadata = await lstat(candidate);
  } catch {
    throw new Error(`${label} does not exist`);
  }
  if (!metadata.isFile() || metadata.isSymbolicLink()) {
    throw new Error(`${label} must be a regular non-symlink file`);
  }
  return metadata;
}

async function validateExecutableOverride(candidate) {
  if (!path.isAbsolute(candidate)) {
    throw new Error('YANSHU_VSCODE_EXECUTABLE must be absolute');
  }
  const metadata = await regularFileMetadata(candidate, 'VS Code executable override');
  if (metadata.size === 0) {
    throw new Error('VS Code executable override is empty');
  }
  return candidate;
}

async function withoutHostCredentialsAndProxy(operation) {
  const removed = [];
  for (const [name, value] of Object.entries(process.env)) {
    if (excludedTestEnvironmentName.test(name)) {
      removed.push([name, value]);
      delete process.env[name];
    }
  }
  try {
    return await operation();
  } finally {
    for (const [name, value] of removed) {
      if (value !== undefined) {
        process.env[name] = value;
      }
    }
  }
}

main().catch((error) => {
  const message = error instanceof Error ? error.message : 'unknown Extension Host failure';
  process.stderr.write(`VSCODE_E2E_FAILED: ${message}\n`);
  process.exitCode = 1;
});
