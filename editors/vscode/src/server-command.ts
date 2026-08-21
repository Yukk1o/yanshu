import { statSync } from 'node:fs';
import { isAbsolute, join } from 'node:path';

export type ServerSource = 'configured' | 'bundled' | 'path';

export interface ServerSelection {
  readonly command: string;
  readonly source: ServerSource;
}

export interface ServerSelectionOptions {
  readonly extensionPath: string;
  readonly configuredPath?: string;
  readonly platform?: NodeJS.Platform;
  readonly architecture?: string;
  readonly isRegularFile?: (path: string) => boolean;
}

const SENSITIVE_ENVIRONMENT_NAME = /(?:auth|credential|key|password|secret|token)/iu;

export function platformTarget(
  platform: NodeJS.Platform = process.platform,
  architecture: string = process.arch,
): string | undefined {
  const targets: Readonly<Record<string, Readonly<Record<string, string>>>> = {
    win32: { x64: 'win32-x64', arm64: 'win32-arm64' },
    linux: { x64: 'linux-x64', arm64: 'linux-arm64' },
    darwin: { x64: 'darwin-x64', arm64: 'darwin-arm64' },
  };
  return targets[platform]?.[architecture];
}

export function serverExecutableName(platform: NodeJS.Platform = process.platform): string {
  return platform === 'win32' ? 'yanshu-lsp.exe' : 'yanshu-lsp';
}

export function selectServerCommand(options: ServerSelectionOptions): ServerSelection {
  const platform = options.platform ?? process.platform;
  const architecture = options.architecture ?? process.arch;
  const isRegularFile = options.isRegularFile ?? defaultIsRegularFile;
  const configuredPath = options.configuredPath?.trim();

  if (configuredPath) {
    if (!isAbsolute(configuredPath)) {
      throw new Error('configured yanshu.server.path must be absolute');
    }
    if (!isRegularFile(configuredPath)) {
      throw new Error('configured yanshu.server.path must identify a regular file');
    }
    return { command: configuredPath, source: 'configured' };
  }

  const target = platformTarget(platform, architecture);
  if (target) {
    const bundled = join(
      options.extensionPath,
      'server',
      target,
      serverExecutableName(platform),
    );
    if (isRegularFile(bundled)) {
      return { command: bundled, source: 'bundled' };
    }
  }

  return { command: serverExecutableName(platform), source: 'path' };
}

export function sanitizedServerEnvironment(
  environment: NodeJS.ProcessEnv = process.env,
): NodeJS.ProcessEnv {
  const sanitized: NodeJS.ProcessEnv = {};
  for (const [name, value] of Object.entries(environment)) {
    if (value !== undefined && !SENSITIVE_ENVIRONMENT_NAME.test(name)) {
      sanitized[name] = value;
    }
  }
  return sanitized;
}

function defaultIsRegularFile(path: string): boolean {
  try {
    return statSync(path).isFile();
  } catch {
    return false;
  }
}
