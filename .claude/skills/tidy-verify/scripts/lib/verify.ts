import type { ExecResult } from "./procs.ts";

export interface RunningVerify {
  send(data: Uint8Array): Promise<boolean>;
  finish(): Promise<ExecResult>;
}

export function startVerify(cmd: string[]): RunningVerify {
  const proc = Bun.spawn(cmd, { stdin: "pipe", stdout: "pipe", stderr: "pipe" });
  const out = new Response(proc.stdout).arrayBuffer();
  const err = new Response(proc.stderr).arrayBuffer();
  let exited = false;
  void proc.exited.then(() => {
    exited = true;
  });
  return {
    async send(data) {
      if (exited) {
        return false;
      }
      try {
        await proc.stdin.write(data);
        await proc.stdin.end();
        return true;
      } catch {
        return false;
      }
    },
    async finish() {
      const [stdout, stderr, exit] = await Promise.all([out, err, proc.exited]);
      return { out: new Uint8Array(stdout), err: new Uint8Array(stderr), exit };
    },
  };
}

export function verifyExitCode(exit: number, sent: boolean): number {
  return !sent && exit === 0 ? 1 : exit;
}

export function verifyCommand(
  bin: string,
  src: string,
  out: string,
  tsv: string | null,
  report: string,
): string[] {
  const tsvArgs = tsv === null ? [] : ["--exif-tsv", tsv];
  return [bin, "--log-level=debug", "verify", src, "-o", out, ...tsvArgs, "--report", report];
}
