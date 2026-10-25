/** Tiny safe math evaluator: + - * / % ^ ( ) and decimals. Returns null if not a valid expression. */
export function evalExpr(src: string): string | null {
  const s = src.replace(/\s+/g, "").replace(/×/g, "*").replace(/÷/g, "/");
  if (!/^[\d+\-*/%^().]+$/.test(s) || !/\d/.test(s) || !/[+\-*/%^]/.test(s.replace(/^[-+]/, "")))
    return null;
  let i = 0;
  const fail = (): never => {
    throw new Error("x");
  };
  const expr = (): number => {
    let v = term();
    while (s[i] === "+" || s[i] === "-") v = s[i++] === "+" ? v + term() : v - term();
    return v;
  };
  const term = (): number => {
    let v = pow();
    while (s[i] === "*" || s[i] === "/" || s[i] === "%") {
      const o = s[i++],
        r = pow();
      v = o === "*" ? v * r : o === "/" ? v / r : v % r;
    }
    return v;
  };
  const pow = (): number => {
    const b = unary();
    return s[i] === "^" ? (i++, Math.pow(b, pow())) : b;
  };
  const unary = (): number => {
    if (s[i] === "-") return (i++, -unary());
    if (s[i] === "+") return (i++, unary());
    return atom();
  };
  const atom = (): number => {
    if (s[i] === "(") {
      i++;
      const v = expr();
      if (s[i++] !== ")") fail();
      return v;
    }
    const m = /^(\d+\.?\d*|\.\d+)/.exec(s.slice(i));
    if (!m) return fail();
    i += m[0].length;
    return parseFloat(m[0]);
  };
  try {
    const r = expr();
    if (i !== s.length || !Number.isFinite(r)) return null;
    return String(Number(r.toPrecision(12)));
  } catch {
    return null;
  }
}
