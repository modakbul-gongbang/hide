// Which Korean particle follows a machine's name in the core move's copy
// (`Mac mini로` but `집으로`, `Mac mini는` but `작업실은`). Catalogs keep one
// set of insertions in every language, so a message whose Korean particle
// varies has a second key for the other form (as `hideAi.firstRunConsonant`
// does); these say which key a name takes. A Hangul syllable says how it
// ends itself; a Latin letter or a digit is read as Korean reads it (`Mac`
// 맥, `box` 박스, `3` 삼).

/** How a name's last sound ends: a vowel, ㄹ, or another final consonant. */
type Ending = "vowel" | "rieul" | "consonant";

// Latin letters whose usual Korean reading closes the syllable (`Mac` 맥,
// `app` 앱, `Tim` 팀, `tin` 틴); the rest read as a vowel-final syllable
// (`box` 박스, `pad` 패드, `Pro` 프로).
const LATIN_CONSONANT = new Set(["b", "c", "k", "m", "n", "p", "q", "g"]);
// Digits as read in Sino-Korean: 영 일 이 삼 사 오 육 칠 팔 구.
const DIGIT: Record<string, Ending> = { "0": "consonant", "1": "rieul", "2": "vowel", "3": "consonant", "4": "vowel", "5": "vowel", "6": "consonant", "7": "rieul", "8": "rieul", "9": "vowel" };

function ending(name: string): Ending {
  const last = Array.from(name.trim()).filter((letter) => /[\p{L}\p{N}]/u.test(letter)).at(-1);
  if (last === undefined) return "vowel";
  const code = last.charCodeAt(0);
  if (code >= 0xac00 && code <= 0xd7a3) {
    const final = (code - 0xac00) % 28;
    return final === 0 ? "vowel" : final === 8 ? "rieul" : "consonant";
  }
  const digit = DIGIT[last];
  if (digit) return digit;
  const letter = last.toLowerCase();
  if (letter === "l") return "rieul";
  return LATIN_CONSONANT.has(letter) ? "consonant" : "vowel";
}

/** Whether `name` takes `으로` rather than `로`: a final consonant other than ㄹ. */
export function takesEuro(name: string): boolean {
  return ending(name) === "consonant";
}

/** Whether `name` takes `은` rather than `는`: any final consonant, ㄹ included. */
export function takesEun(name: string): boolean {
  return ending(name) !== "vowel";
}
