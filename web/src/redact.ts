// Secret-shaped text out of anything copied or logged to be read elsewhere:
// Copy diagnostics in the shell and the desktop host's log of exceptions.

// A secret-shaped run: the page token and anything like it (32+ hex), or a
// `token=`/`key=`/`secret=`/`password=` assignment. Such text never carries
// one, even when a message quoted it.
const SECRET_RUN = /\b[0-9a-f]{32,}\b/gi;
const SECRET_ASSIGNMENT = /\b(token|key|secret|password|passphrase)=([^\s&]+)/gi;

export function redact(text: string): string {
  return text.replace(SECRET_ASSIGNMENT, "$1=[redacted]").replace(SECRET_RUN, "[redacted]");
}
