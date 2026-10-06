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
