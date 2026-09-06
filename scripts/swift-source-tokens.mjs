// Swift quoted strings may contain interpolation with nested quoted strings.
// Keep their source spelling, including interpolation, so changing a label's
// expression also changes the inventory. Comments never become copy.
export function quoted(source, start) {
  const triple = source.startsWith('"""', start);
  const delimiter = triple ? '"""' : '"';
  let at = start + delimiter.length;
  while (at < source.length) {
    if (source.startsWith(delimiter, at)) {
      return {value: source.slice(start + delimiter.length, at), end: at + delimiter.length};
    }
    if (source.startsWith('\\(', at)) {
      let depth = 1;
      at += 2;
      while (depth && at < source.length) {
        if (source[at] === '"') at = quoted(source, at).end;
        else {
          if (source[at] === '(') depth++;
          if (source[at] === ')') depth--;
          at++;
        }
      }
    } else at += source[at] === '\\' ? 2 : 1;
  }
  throw new Error(`Unterminated Swift string at ${start}`);
}

export function tokens(source) {
  const result = [];
  for (let at = 0; at < source.length;) {
    if (/\s/.test(source[at])) { at++; continue; }
    if (source.startsWith('//', at)) {
      const end = source.indexOf('\n', at); at = end < 0 ? source.length : end; continue;
    }
    if (source.startsWith('/*', at)) {
      let depth = 1; at += 2;
      while (depth && at < source.length) {
        if (source.startsWith('/*', at)) { depth++; at += 2; }
        else if (source.startsWith('*/', at)) { depth--; at += 2; }
        else at++;
      }
      continue;
    }
    if (source[at] === '"') {
      const item = quoted(source, at); result.push({string: item.value}); at = item.end; continue;
    }
    const number = /^(?:[0-9]+(?:_[0-9]+)*(?:\.[0-9]+)?(?:[eE][+-]?[0-9]+)?)/.exec(source.slice(at));
    if (number) { result.push(number[0]); at += number[0].length; continue; }
    const word = /^[A-Za-z_][A-Za-z_0-9]*/.exec(source.slice(at));
    result.push(word ? word[0] : source[at]); at += word ? word[0].length : 1;
  }
  return result;
}

