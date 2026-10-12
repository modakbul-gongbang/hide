import { describe, expect, it } from "vitest";
import { takesEun, takesEuro } from "./koParticle";

describe("the Korean particle after a machine's name", () => {
  it("takes 으로 only after a final consonant other than ㄹ, as Korean reads the name", () => {
    for (const name of ["Mac mini", "build-box", "MacBook Pro", "서버", "작업실", "mini-7"]) expect(takesEuro(name), name).toBe(false);
    for (const name of ["이 Mac", "집", "mini-3", "dev-vm"]) expect(takesEuro(name), name).toBe(true);
  });

  it("takes 은 after any final consonant, ㄹ included", () => {
    for (const name of ["Mac mini", "build-box", "서버"]) expect(takesEun(name), name).toBe(false);
    for (const name of ["이 Mac", "작업실", "mini-7"]) expect(takesEun(name), name).toBe(true);
  });
});
