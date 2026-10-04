/// <reference types="node" />
export type NativeCommandOwner = {
  receipt: string;
  home: string;
  executable: string;
  args: string[];
  subject: string;
  supervisorPid?: number;
  supervisorImage?: { path: string; source: string; digest: { size: number; sha256: string } };
  supervisorExited?: boolean;
  observed?: { phase: string; pid: number; birth: string; code: number; survivors: number; error: string } | null;
};
export function shell(): string;
export function digest(file: string): { size: number; sha256: string };
export function run(root: string, executable: string, args: string[], options?: {
  timeout?: number;
  maxBuffer?: number;
  subject?: string;
  env?: NodeJS.ProcessEnv;
  onOwner?: (owner: NativeCommandOwner) => void;
  onStderr?: (chunk: Buffer) => void;
}): Promise<{ stdout: string; stderr: string; owner: NativeCommandOwner }>;
